//! Сокет демона (абстрактный unix-сокет «zhord») и цикл сбора.

use crate::report::{self, Query};
use crate::sampler::Daemon;
use crate::sys;
use crate::util;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD: &str = env!("ZHOR_BUILD");
pub const SOCK: &str = "zhord";

static STOP: AtomicBool = AtomicBool::new(false);
static SNAP_RUNNING: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

pub fn bind() -> std::io::Result<UnixListener> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        #[cfg(target_os = "android")]
        use std::os::android::net::SocketAddrExt;
        #[cfg(target_os = "linux")]
        use std::os::linux::net::SocketAddrExt;
        let addr = std::os::unix::net::SocketAddr::from_abstract_name(SOCK.as_bytes())?;
        UnixListener::bind_addr(&addr)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let p = format!("/tmp/{SOCK}.sock");
        if UnixStream::connect(&p).is_err() {
            let _ = std::fs::remove_file(&p);
        }
        UnixListener::bind(p)
    }
}

pub fn connect() -> std::io::Result<UnixStream> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        #[cfg(target_os = "android")]
        use std::os::android::net::SocketAddrExt;
        #[cfg(target_os = "linux")]
        use std::os::linux::net::SocketAddrExt;
        let addr = std::os::unix::net::SocketAddr::from_abstract_name(SOCK.as_bytes())?;
        UnixStream::connect_addr(&addr)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        UnixStream::connect(format!("/tmp/{SOCK}.sock"))
    }
}

/// Запрос к демону: одна строка JSON туда, ответ до EOF обратно.
pub fn request(req: &Value, timeout_s: u64) -> std::io::Result<String> {
    let mut s = connect()?;
    s.set_read_timeout(Some(Duration::from_secs(timeout_s)))?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    s.write_all(line.as_bytes())?;
    s.shutdown(std::net::Shutdown::Write)?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

fn log(dir: &PathBuf, msg: &str) {
    let p = dir.join("zhord.log");
    if std::fs::metadata(&p).map(|m| m.len() > 1 << 20).unwrap_or(false) {
        let _ = std::fs::rename(&p, dir.join("zhord.log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{} {}", report::fmt_time(util::real_ms()), msg);
    }
}

/// Скромный приоритет: профилировщик не должен заметно тратить сам.
fn lower_priority() {
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
    let pid = std::process::id().to_string();
    for p in ["/dev/cpuset/background/tasks", "/dev/cpuctl/background/tasks"] {
        let _ = std::fs::write(p, &pid);
    }
}

/// Системный снимок + история событий Android из того же вывода batterystats.
/// Разбор и запись истории — под её собственным замком, демон не блокируется.
fn take_snap(d: &Arc<Mutex<Daemon>>, dir: &PathBuf, fresh: bool) -> sys::SysSnap {
    let now = util::real_ms();
    let (pn, hist, full, with_history) = match d.lock() {
        Ok(g) => {
            // полная история дорогая (~0,4 с CPU system_server): по расписанию (страховка буфера Android)
            // и по ручному обновлению — тогда она так же свежа, как и раньше
            let age = now - g.last_hist_real;
            let due = age >= g.config.hist_every_min as i64 * 60_000 || (fresh && age >= 60_000);
            (g.pid_names(), Some(g.hist.clone()), g.config.full_history, due)
        }
        Err(_) => (Default::default(), None, false, false),
    };
    // подробная история Android сбрасывается при перезагрузке — включаем снова (дёшево, идемпотентно)
    if full {
        crate::history::set_android_full_history(true);
    }
    let (s, dump) = sys::take_full(pn, with_history);
    if let Some(dump) = dump {
        ingest_history(d, dir, hist, dump, now);
    }
    s
}

/// Записать дамп истории; время чтения отмечаем, только если Android что-то отдал.
fn ingest_history(d: &Arc<Mutex<Daemon>>, dir: &PathBuf, hist: Option<Arc<Mutex<crate::history::HistStore>>>, dump: crate::history::Dump, now: i64) {
    if dump.records == 0 {
        return;
    }
    if let Ok(mut g) = d.lock() {
        g.last_hist_real = now;
    }
    let Some(hist) = hist else { return };
    let t0 = std::time::Instant::now();
    let (n, from) = (dump.ev.len(), dump.from);
    if let Ok(mut h) = hist.lock() {
        h.ingest(dump);
        h.last_cost_ms = t0.elapsed().as_millis() as u64;
        log(dir, &format!("история Android: {n} событий с {}, запись {} мс, в памяти {}", crate::report::fmt_time(from), h.last_cost_ms, h.ev.len()));
    };
}

/// Только история (без снимка): когда отчёт смотрят, а история старше 5 мин.
fn refresh_history(d: &Arc<Mutex<Daemon>>, dir: &PathBuf) {
    let now = util::real_ms();
    let (stale, hist) = match d.lock() {
        Ok(g) => (now - g.last_hist_real > 300_000, Some(g.hist.clone())),
        Err(_) => (false, None),
    };
    if !stale {
        return;
    }
    if let Some(t) = util::run_cmd("dumpsys", &["batterystats", "-c", "--history"], 60_000) {
        let dump = crate::history::parse_checkin(&t);
        drop(t);
        ingest_history(d, dir, hist, dump, now);
    }
}

/// Доставить тревоги сторожа приложению и записать их (вне замка демона: `cmd` — сотни мс).
fn spawn_deliver(d: Arc<Mutex<Daemon>>, dir: PathBuf, alerts: Vec<crate::watch::Alert>) {
    std::thread::spawn(move || {
        for mut a in alerts {
            let (ok, out) = crate::watch::deliver(&a);
            a.sent = ok;
            let tail = if ok { String::new() } else { format!(" — не доставлено: {out}") };
            log(&dir, &format!("сторож: {} «{}»{tail}", a.key, a.title));
            if let Ok(mut g) = d.lock() {
                g.watch.record(a);
            }
        }
    });
}

fn spawn_snapshot(d: Arc<Mutex<Daemon>>, dir: PathBuf) {
    if SNAP_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let s = take_snap(&d, &dir, false);
        log(&dir, &format!("снимок batterystats: {} мс, uid в usage: {}", s.cost_ms, s.usage.uids.len()));
        if let Ok(mut g) = d.lock() {
            g.add_snap(s);
        }
        SNAP_RUNNING.store(false, Ordering::SeqCst);
    });
}

pub fn run_daemon(dir: PathBuf) -> i32 {
    let listener = match bind() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("zhord: сокет занят — демон уже работает? ({e})");
            return 1;
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    lower_priority();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("zhord.pid"), std::process::id().to_string());
    log(&dir, &format!("старт v{VERSION} (сборка {BUILD}), pid {}", std::process::id()));
    let d = Arc::new(Mutex::new(Daemon::new(dir.clone())));
    {
        let g = d.lock().unwrap();
        log(&dir, &format!("профиль: {}, кластеров {}", g.pp.source, g.pp.clusters.len()));
    }

    // приём запросов
    {
        let d = d.clone();
        let dir = dir.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { continue };
                let d = d.clone();
                let dir = dir.clone();
                std::thread::spawn(move || handle(conn, d, dir));
            }
        });
    }

    // цикл сбора
    loop {
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        let iv = {
            let mut g = d.lock().unwrap();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.tick()));
            if r.is_err() {
                log(&dir, "паника в шаге сбора — пропуск");
            }
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.watch_eval())) {
                Ok(alerts) if !alerts.is_empty() => spawn_deliver(d.clone(), dir.clone(), alerts),
                Ok(_) => {}
                Err(_) => log(&dir, "паника в стороже — пропуск"),
            }
            if g.snap_due(util::real_ms()) && !SNAP_RUNNING.load(Ordering::SeqCst) {
                g.want_snap = false;
                drop(g);
                spawn_snapshot(d.clone(), dir.clone());
                d.lock().unwrap().interval_ms()
            } else {
                g.interval_ms()
            }
        };
        // спим кусками, чтобы быстро реагировать на stop
        let mut left = iv;
        while left > 0 && !STOP.load(Ordering::SeqCst) {
            let s = left.min(1000);
            std::thread::sleep(Duration::from_millis(s));
            left -= s;
        }
    }
    if let Ok(mut g) = d.lock() {
        g.flush();
    }
    log(&dir, "остановлен");
    0
}

#[derive(Deserialize)]
struct Req {
    cmd: String,
    #[serde(flatten)]
    q: Query,
}

fn raw_req(line: &str) -> Value {
    serde_json::from_str(line).unwrap_or(Value::Null)
}

/// uid процесса на том конце сокета. Абстрактный сокет не защищён правами файла,
/// поэтому принимаем только root (приложение ходит через su).
fn peer_uid(conn: &UnixStream) -> Option<u32> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::fd::AsRawFd;
        let mut cred = libc::ucred { pid: 0, uid: u32::MAX, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(conn.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len)
        };
        if rc == 0 { Some(cred.uid) } else { None }
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = conn;
        Some(0)
    }
}

fn handle(conn: UnixStream, d: Arc<Mutex<Daemon>>, dir: PathBuf) {
    if peer_uid(&conn) != Some(0) {
        let mut w = conn;
        let _ = w.write_all(b"{\"error\":\"only root\"}\n");
        return;
    }
    let mut r = BufReader::new(conn.try_clone().unwrap());
    let mut line = String::new();
    if r.read_line(&mut line).is_err() {
        return;
    }
    let mut w = conn;
    let resp: String = match serde_json::from_str::<Req>(&line) {
        Err(e) => json!({"error": format!("плохой запрос: {e}")}).to_string(),
        Ok(req) => match req.cmd.as_str() {
            "ping" | "status" => {
                let g = d.lock().unwrap();
                let hist = g.hist.lock().ok().map(|h| {
                    json!({"events_mem": h.ev.len(), "mem_bytes": h.mem_bytes(), "covered_from": h.covered_from, "android_from": h.dump_from, "android_to": h.dump_to, "cost_ms": h.last_cost_ms})
                });
                json!({
                    "hist": hist,
                    "ok": true, "version": VERSION, "build": BUILD.parse::<u64>().unwrap_or(0), "pid": std::process::id(), "started": g.started,
                    "profile": g.pp.source, "clusters": g.pp.clusters.len(),
                    "buckets": g.store.mem.len(), "snaps": g.snaps.len(),
                    "scan_cost_us": g.last_scan_cost_us, "cost": g.cost, "cpu_total_us": g.cpu_total_us, "interval_ms": g.interval_ms(), "dir": g.dir,
                    "oldest": g.store.oldest(),
                })
                .to_string()
            }
            "live" => {
                let mut g = d.lock().unwrap();
                // пока «Сейчас» открыт (опрос каждые 2–3 с), ядро шагает раз в 2 с
                g.live_until = util::mono_ms() + 15_000;
                let cur = g.current();
                json!({
                    "battery": cur.as_ref().map(|c| &c.0),
                    "screen": cur.as_ref().map(|c| c.1),
                    "series": g.live,
                    "scan_cost_us": g.last_scan_cost_us,
                })
                .to_string()
            }
            "profile" => serde_json::to_string(&d.lock().unwrap().pp).unwrap_or_default(),
            "events" => serde_json::to_string(&d.lock().unwrap().events).unwrap_or_default(),
            "snap" => {
                // один съём за раз: параллельный отчёт или снимок дождётся данных, а не снимет второй раз
                if SNAP_RUNNING.swap(true, Ordering::SeqCst) {
                    json!({"ok": false, "busy": true}).to_string()
                } else {
                    let s = take_snap(&d, &dir, true);
                    let cost = s.cost_ms;
                    d.lock().unwrap().add_snap(s);
                    SNAP_RUNNING.store(false, Ordering::SeqCst);
                    json!({"ok": true, "cost_ms": cost}).to_string()
                }
            }
            "report" | "report_text" => {
                if req.q.fresh {
                    let stale = d.lock().unwrap().snaps.back().map(|s| util::real_ms() - s.t > 60_000).unwrap_or(true);
                    if stale && !SNAP_RUNNING.swap(true, Ordering::SeqCst) {
                        let s = take_snap(&d, &dir, true);
                        d.lock().unwrap().add_snap(s);
                        SNAP_RUNNING.store(false, Ordering::SeqCst);
                    }
                }
                if !req.q.fresh && !SNAP_RUNNING.swap(true, Ordering::SeqCst) {
                    refresh_history(&d, &dir);
                    SNAP_RUNNING.store(false, Ordering::SeqCst);
                }
                let g = d.lock().unwrap();
                let rep = report::build(&g, &req.q, None);
                if req.cmd == "report_text" {
                    report::render_text(&rep)
                } else {
                    serde_json::to_string(&rep).unwrap_or_default()
                }
            }
            "config" => {
                let raw = raw_req(&line);
                let mut g = d.lock().unwrap();
                if raw.get("set").map(|v| v.is_object()).unwrap_or(false) {
                    let c = g.config.merged(&raw["set"]);
                    g.set_config(c);
                    log(&dir, "настройки изменены");
                }
                serde_json::to_string(&g.config).unwrap_or_default()
            }
            "storage" => crate::data::storage(&d.lock().unwrap()).to_string(),
            "export" => {
                let raw = raw_req(&line);
                let g = d.lock().unwrap();
                crate::data::export(&g, &raw, &req.q).to_string()
            }
            "prune" => {
                let raw = raw_req(&line);
                let mut g = d.lock().unwrap();
                let r = crate::data::prune(&mut g, &raw);
                log(&dir, &format!("очистка данных: {r}"));
                r.to_string()
            }
            "alerts" => {
                let raw = raw_req(&line);
                let limit = raw["limit"].as_u64().unwrap_or(100) as usize;
                let since = raw["since"].as_i64().unwrap_or(0);
                let g = d.lock().unwrap();
                let list: Vec<&crate::watch::Alert> = g.watch.alerts.iter().rev().filter(|a| a.t >= since).take(limit).collect();
                json!({
                    "alerts": list, "watch": g.config.watch, "mute": g.config.watch_mute,
                    "last_eval": g.watch.last_eval, "cost_us": g.watch.last_cost_us,
                })
                .to_string()
            }
            "alert_test" => {
                // проверочная в журнал тревог не пишется — только в журнал ядра
                let a = crate::watch::test_alert(util::real_ms());
                let (ok, out) = crate::watch::deliver(&a);
                log(&dir, &format!("сторож: проверка — {}", if ok { "доставлено" } else { "не доставлено" }));
                json!({"ok": ok, "out": out}).to_string()
            }
            "mute" => {
                let raw = raw_req(&line);
                match raw["key"].as_str().filter(|k| !k.is_empty()) {
                    None => json!({"error": "нужен key"}).to_string(),
                    Some(key) => {
                        let on = raw["on"].as_bool().unwrap_or(true);
                        let mut g = d.lock().unwrap();
                        let mut c = g.config.clone();
                        c.watch_mute.retain(|k| k != key);
                        if on {
                            c.watch_mute.push(key.to_string());
                        }
                        g.set_config(c.clamped());
                        log(&dir, &format!("сторож: {} «{key}»", if on { "не напоминать" } else { "снова напоминать" }));
                        serde_json::to_string(&g.config).unwrap_or_default()
                    }
                }
            }
            "app_state" => {
                let raw = raw_req(&line);
                let uid = raw["uid"].as_u64().map(|u| u as u32);
                crate::compare::app_state(raw["pkg"].as_str().unwrap_or(""), uid).to_string()
            }
            "act" => {
                let raw = raw_req(&line);
                let uid = raw["uid"].as_u64().map(|u| u as u32);
                let (pkg, action) = (raw["pkg"].as_str().unwrap_or(""), raw["action"].as_str().unwrap_or(""));
                let r = crate::compare::act(&dir, pkg, uid, action);
                log(&dir, &format!("действие {action} {pkg}: {}", if r["ok"].as_bool().unwrap_or(false) { "ок" } else { "не вышло" }));
                r.to_string()
            }
            "actions" => {
                let raw = raw_req(&line);
                let uid = raw["uid"].as_u64().map(|u| u as u32);
                let keep = d.lock().unwrap().config.keep_days;
                json!({"actions": crate::compare::actions(&dir, uid, keep)}).to_string()
            }
            "compare" => {
                let raw = raw_req(&line);
                let g = d.lock().unwrap();
                crate::compare::compare(&g, &raw).to_string()
            }
            "stop" => {
                STOP.store(true, Ordering::SeqCst);
                log(&dir, "запрошена остановка");
                json!({"ok": true}).to_string()
            }
            other => json!({"error": format!("неизвестная команда {other}")}).to_string(),
        },
    };
    let _ = w.write_all(resp.as_bytes());
    let _ = w.write_all(b"\n");
}
