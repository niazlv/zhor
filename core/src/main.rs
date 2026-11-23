//! zhord — ядро «Жора»: root-профилировщик энергопотребления Android.
//!
//!   zhord start                 запустить демон в фоне
//!   zhord run                   демон на переднем плане
//!   zhord stop | status | live
//!   zhord report [пресет] [--json] [--fresh] [--uid N]
//!   zhord alerts | alert-test   тревоги сторожа
//!   zhord compare [rest|action МС] [--uid N]   сравнение покоя до/после
//!   zhord query '<json>'        сырой запрос к демону
//!   zhord oneshot [секунд]      замер без демона: кто тратит прямо сейчас

mod data;
mod explain;
mod history;
mod procfs;
mod profile;
mod report;
mod sampler;
mod server;
mod store;
mod sys;
mod sysfs;
mod util;
mod watch;
mod compare;

use serde_json::json;
use std::path::PathBuf;

const DEFAULT_DIR: &str = "/data/adb/zhor";

fn usage() -> ! {
    eprintln!(
        "zhord {} — профилировщик энергии Android (root)\n\
         \n  zhord start [--dir D]       демон в фоне\
         \n  zhord run [--dir D]         демон на переднем плане\
         \n  zhord stop | status | live\
         \n  zhord report [since_unplug|screen_off|charge|1h|3h|6h|12h|24h|48h|all] [--json] [--fresh] [--uid N]\
         \n  zhord alerts                тревоги сторожа\
         \n  zhord alert-test            проверочная тревога в приложение\
         \n  zhord watch-check [--days N] [--fast]  прогон правил сторожа по записям (только чтение)\
         \n  zhord compare [rest | action МС] [--uid N]   сравнение покоя до/после\
         \n  zhord query '<json>'\
         \n  zhord oneshot [секунд]      замер без демона\
         \n  zhord parse usage|checkin|suspend ФАЙЛ   (отладка разборщиков)\
         \n  zhord history-check [ФАЙЛ]  разбор истории Android (вывод batterystats -c)",
        server::VERSION
    );
    std::process::exit(2)
}

fn arg_val(args: &[String], key: &str) -> Option<String> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1).cloned())
}

fn daemonize(dir: &PathBuf) {
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            eprintln!("fork не удался");
            std::process::exit(1);
        }
        if pid > 0 {
            // родитель: дождаться, что демон поднялся
            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if server::request(&json!({"cmd": "ping"}), 2).is_ok() {
                    println!("zhord запущен");
                    std::process::exit(0);
                }
            }
            eprintln!("демон не ответил за 5 с — см. {}/zhord.log", dir.display());
            std::process::exit(1);
        }
        libc::setsid();
        if libc::fork() > 0 {
            libc::_exit(0);
        }
        let _ = std::env::set_current_dir("/");
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if devnull >= 0 {
            libc::dup2(devnull, 0);
            libc::dup2(devnull, 1);
            libc::dup2(devnull, 2);
        }
    }
}

fn oneshot(secs: u64) {
    let pp = profile::load_device();
    let mut sc = procfs::ProcScanner::new(&pp);
    sc.min_ticks = 1; // разовый замер: без отложенного учёта
    let mut br = sysfs::BatteryReader::new();
    let t_base = std::time::Instant::now();
    let _ = sc.scan(util::boot_ms());
    let base_cost = t_base.elapsed();
    // второй проход: потоки, активные с базы, получают свою базу time_in_state
    let _ = sc.scan(util::boot_ms());
    let t0 = util::boot_ms();
    let b0 = br.read();
    let t_scan = std::time::Instant::now();
    let warm = sc.scan(t0);
    let scan_cost = t_scan.elapsed();
    println!("тёплый проход: процессов {} потоков {} чтений time_in_state {}", warm.scanned_procs, warm.scanned_threads, warm.tis_reads);
    std::thread::sleep(std::time::Duration::from_secs(secs));
    let t1 = util::boot_ms();
    let t_full = std::time::Instant::now();
    let tick = sc.scan(t1);
    println!(
        "полный проход через {secs} с: {:.1} мс, процессов {} потоков {} чтений time_in_state {}",
        t_full.elapsed().as_secs_f64() * 1000.0,
        tick.scanned_procs,
        tick.scanned_threads,
        tick.tis_reads
    );
    let b1 = br.read();
    let dt = (t1 - t0) as f64 / 1000.0;
    println!("Профиль: {}; кластеров {}; time_in_state по потокам: {}", pp.source, pp.clusters.len(), if sc.tis_available { "да" } else { "НЕТ (оценка)" });
    println!(
        "За {:.1} с: батарея {:.0}→{:.0} мА ({}), {:.2} В; процессов {} потоков {}; проход сканера {:.1} мс (база {:.0} мс)",
        dt,
        b0.draw_ma,
        b1.draw_ma,
        b1.status,
        b1.voltage_mv / 1000.0,
        tick.scanned_procs,
        tick.scanned_threads,
        scan_cost.as_secs_f64() * 1000.0,
        base_cost.as_secs_f64() * 1000.0
    );
    let mut ps = tick.procs.clone();
    ps.sort_by(|a, b| b.mas.partial_cmp(&a.mas).unwrap());
    let total: f64 = ps.iter().map(|p| p.mas).sum();
    println!("Модель CPU: {:.0} мА в среднем\n", total / dt);
    println!("{:>8} {:>7}  {:>7}  {:<44} потоки (мА)", "мА", "CPU мс", "uid", "процесс");
    for p in ps.iter().take(20) {
        let mut th = p.threads.clone();
        th.sort_by(|a, b| b.mas.partial_cmp(&a.mas).unwrap());
        let ths: Vec<String> = th.iter().take(4).map(|t| format!("{} {:.0}", if t.main { "<main>" } else { &t.name }, t.mas / dt)).collect();
        let name: String = p.name.chars().take(44).collect();
        println!("{:>8.1} {:>7.0}  {:>7}  {:<44} {}", p.mas / dt, p.ticks as f64 * sc.tick_ms(), p.uid, name, ths.join(" · "));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
    }
    let dir = PathBuf::from(arg_val(&args, "--dir").unwrap_or_else(|| DEFAULT_DIR.to_string()));
    match args[1].as_str() {
        "run" | "daemon" => std::process::exit(server::run_daemon(dir)),
        "start" => {
            if let Ok(resp) = server::request(&json!({"cmd": "ping"}), 3) {
                let running: u64 = serde_json::from_str::<serde_json::Value>(&resp)
                    .ok()
                    .and_then(|v| v["build"].as_u64())
                    .unwrap_or(0);
                let mine: u64 = server::BUILD.parse().unwrap_or(0);
                if running >= mine {
                    println!("zhord уже работает (сборка {running})");
                    return;
                }
                // работает более старая сборка — заменить
                let _ = server::request(&json!({"cmd": "stop"}), 5);
                for _ in 0..100 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if server::request(&json!({"cmd": "ping"}), 1).is_err() {
                        break;
                    }
                }
                println!("остановлена старая сборка {running}");
            }
            daemonize(&dir);
            std::process::exit(server::run_daemon(dir));
        }
        "stop" => match server::request(&json!({"cmd": "stop"}), 5) {
            Ok(_) => println!("остановлен"),
            Err(_) => println!("демон не запущен"),
        },
        "status" | "ping" | "live" | "profile" | "events" | "snap" => match server::request(&json!({"cmd": args[1]}), 120) {
            Ok(s) => print!("{s}"),
            Err(e) => {
                eprintln!("демон не отвечает: {e}");
                std::process::exit(1)
            }
        },
        "alerts" => match server::request(&json!({"cmd": "alerts", "limit": 30}), 30) {
            Ok(s) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
                let list = v["alerts"].as_array().cloned().unwrap_or_default();
                println!("Сторож: {}; тревог: {}", if v["watch"].as_bool().unwrap_or(false) { "включён" } else { "выключен" }, list.len());
                for a in &list {
                    let mark = match a["level"].as_str() {
                        Some("bad") => "✖",
                        Some("warn") => "⚠",
                        _ => "•",
                    };
                    let sent = if a["sent"].as_bool().unwrap_or(false) { "" } else { " [не доставлено]" };
                    println!("\n{mark} {} — {}{sent}\n  {}", report::fmt_time(a["t"].as_i64().unwrap_or(0)), a["title"].as_str().unwrap_or(""), a["text"].as_str().unwrap_or(""));
                }
                if let Some(m) = v["mute"].as_array().filter(|m| !m.is_empty()) {
                    println!("\nНе напоминать: {}", m.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "));
                }
            }
            Err(e) => {
                eprintln!("демон не отвечает: {e}");
                std::process::exit(1)
            }
        },
        "watch-check" => {
            // прогон правил сторожа по записанным корзинам, как если бы он проверял их каждые 5 мин.
            // Только чтение: файлы корзин не трогает, тревоги никуда не шлёт.
            let dir = PathBuf::from(arg_val(&args, "--dir").unwrap_or_else(|| DEFAULT_DIR.into()));
            let days: i64 = arg_val(&args, "--days").and_then(|v| v.parse().ok()).unwrap_or(3);
            let cfg = data::Config::load(&dir);
            let cut = util::real_ms() - days * 86_400_000;
            let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("buckets")).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect()).unwrap_or_default();
            files.sort();
            let mut all: Vec<store::Bucket> = Vec::new();
            for f in files {
                let Ok(text) = std::fs::read_to_string(&f) else { continue };
                all.extend(text.lines().filter_map(|l| serde_json::from_str::<store::Bucket>(l).ok()).filter(|b| b.t1 >= cut));
            }
            all.sort_by_key(|b| b.t0);
            let pkgs = sys::packages();
            let br = sysfs::BatteryReader::new();
            let cap = br.full_capacity_mah().or(br.design_capacity_mah());
            let refs: Vec<&store::Bucket> = all.iter().collect();
            let mut w = watch::Watch::scratch();
            // --fast: сроки правил ×0,2 (30 мин покоя → 6) — посмотреть тексты на коротких записях
            if args.iter().any(|a| a == "--fast") {
                w.min_scale = 0.2;
            }
            let (mut last, mut n, mut cost) = (0i64, 0u32, 0u64);
            println!("корзин {}, сторож {}, пороги: сон < {} %, разряд > {} %/ч, пробуждений > {}/ч, фон > {} мА, жара ≥ {} °C",
                all.len(), if cfg.watch { "вкл" } else { "выкл" }, cfg.watch_sleep_pct, cfg.watch_drain_pct_h, cfg.watch_wakeups_h, cfg.watch_app_ma, cfg.watch_hot_c);
            for i in 0..refs.len() {
                let now = refs[i].t1;
                if now - last < watch::EVAL_EVERY_MS {
                    continue;
                }
                last = now;
                for a in w.eval(&refs[..=i], &cfg, cap, &pkgs, now) {
                    println!("\n{} [{}] {}\n  {}", report::fmt_time(now), a.level, a.title, a.text);
                }
                n += 1;
                cost = cost.max(w.last_cost_us);
            }
            println!("\nпроверок {n}, самая долгая {:.1} мс", cost as f64 / 1000.0);
        }
        "alert-test" | "compare" => {
            let req = if args[1] == "alert-test" {
                json!({"cmd": "alert_test"})
            } else {
                let mode = args.get(2).cloned().unwrap_or_else(|| "rest".into());
                let t: Option<i64> = args.get(3).and_then(|v| v.parse().ok());
                let uid: Option<u32> = arg_val(&args, "--uid").and_then(|v| v.parse().ok());
                json!({"cmd": "compare", "mode": mode, "t": t, "uid": uid})
            };
            match server::request(&req, 60) {
                Ok(s) => println!("{}", serde_json::from_str::<serde_json::Value>(&s).map(|v| serde_json::to_string_pretty(&v).unwrap_or_default()).unwrap_or(s)),
                Err(e) => {
                    eprintln!("демон не отвечает: {e}");
                    std::process::exit(1)
                }
            }
        }
        "report" => {
            let range = args.get(2).filter(|a| !a.starts_with("--")).cloned();
            let json_out = args.iter().any(|a| a == "--json");
            let fresh = args.iter().any(|a| a == "--fresh");
            let uid: Option<u32> = arg_val(&args, "--uid").and_then(|v| v.parse().ok());
            let req = json!({"cmd": if json_out {"report"} else {"report_text"}, "range": range, "fresh": fresh, "uid": uid});
            match server::request(&req, 180) {
                Ok(s) => print!("{s}"),
                Err(e) => {
                    eprintln!("демон не отвечает: {e}");
                    std::process::exit(1)
                }
            }
        }
        "history-check" => {
            // разбор истории Android: из файла (вывод `dumpsys batterystats -c`) или с телефона
            let t0 = std::time::Instant::now();
            let text = match args.get(2).filter(|a| !a.starts_with("--")) {
                Some(p) => std::fs::read_to_string(p).unwrap_or_default(),
                None => util::run_cmd("dumpsys", &["batterystats", "-c"], 60_000).unwrap_or_default(),
            };
            let t_read = t0.elapsed().as_millis();
            let t1 = std::time::Instant::now();
            let d = history::parse_checkin(&text);
            let t_parse = t1.elapsed().as_millis();
            let mut by: std::collections::BTreeMap<&str, (u64, i64, u64)> = Default::default();
            for e in &d.ev {
                let x = by.entry(e.k.name()).or_default();
                x.0 += 1;
                x.1 += e.t1 - e.t0;
                if e.open {
                    x.2 += 1;
                }
            }
            println!("записей {}, событий {}, концов без начала {}; с {} по {}", d.records, d.ev.len(), d.orphans.len(), report::fmt_time(d.from), report::fmt_time(d.to));
            println!("чтение {t_read} мс ({} КБ), разбор {t_parse} мс", text.len() / 1024);
            for (k, (n, ms, open)) in by {
                println!("  {k:<14} {n:>7}×  {:>12}  {}", util::fmt_dur(ms.max(0) as u64), if open > 0 { format!("идёт: {open}") } else { String::new() });
            }
            if args.iter().any(|a| a == "--top") {
                // самые длинные события каждого вида — для проверки сопоставления начала и конца
                for k in history::Kind::ALL {
                    let mut v: Vec<&history::Ev> = d.ev.iter().filter(|e| e.k == k).collect();
                    v.sort_by_key(|e| std::cmp::Reverse(e.t1 - e.t0));
                    for e in v.iter().take(3) {
                        println!("  {:<12} {} {:>12} uid {:>7} {}{}", k.name(), report::fmt_time(e.t0), util::fmt_dur((e.t1 - e.t0) as u64), e.u, &e.g[..e.g.len().min(70)], if e.open { " [идёт]" } else { "" });
                    }
                }
            }
        }
        "query" => {
            let Some(q) = args.get(2) else { usage() };
            let v: serde_json::Value = serde_json::from_str(q).unwrap_or_else(|e| {
                eprintln!("плохой JSON: {e}");
                std::process::exit(2)
            });
            match server::request(&v, 180) {
                Ok(s) => print!("{s}"),
                Err(e) => {
                    eprintln!("демон не отвечает: {e}");
                    std::process::exit(1)
                }
            }
        }
        "bench" => {
            // цена чтения разных файлов /proc на этом ядре, мкс на файл
            let pids: Vec<u32> = std::fs::read_dir("/proc").unwrap().filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect();
            let mut tasks = Vec::new();
            for &p in &pids {
                if let Ok(rd) = std::fs::read_dir(format!("/proc/{p}/task")) {
                    for e in rd.flatten() {
                        if let Some(t) = e.file_name().to_str().and_then(|x| x.parse::<u32>().ok()) {
                            tasks.push((p, t));
                        }
                    }
                }
            }
            let mut buf = Vec::with_capacity(4096);
            let mut run = |name: &str, paths: &[String]| {
                let c0 = util::thread_cpu_us();
                for p in paths {
                    util::read_into(p, &mut buf);
                }
                let c = util::thread_cpu_us() - c0;
                println!("{name:<28} {:>6} файлов  {:>8} мкс CPU  {:>6.1} мкс/файл", paths.len(), c, c as f64 / paths.len().max(1) as f64);
            };
            let ps: Vec<String> = pids.iter().map(|p| format!("/proc/{p}/stat")).collect();
            run("/proc/PID/stat", &ps);
            let ps: Vec<String> = pids.iter().map(|p| format!("/proc/{p}/schedstat")).collect();
            run("/proc/PID/schedstat", &ps);
            let ts: Vec<String> = tasks.iter().map(|(p, t)| format!("/proc/{p}/task/{t}/stat")).collect();
            run("task/TID/stat", &ts);
            let ts: Vec<String> = tasks.iter().map(|(p, t)| format!("/proc/{p}/task/{t}/schedstat")).collect();
            run("task/TID/schedstat", &ts);
            let ts: Vec<String> = tasks.iter().map(|(p, t)| format!("/proc/{p}/task/{t}/comm")).collect();
            run("task/TID/comm", &ts);
            let ts: Vec<String> = tasks.iter().take(2000).map(|(p, t)| format!("/proc/{p}/task/{t}/time_in_state")).collect();
            run("task/TID/time_in_state", &ts);
            let c0 = util::thread_cpu_us();
            let mut n = 0;
            for &p in &pids {
                if let Ok(rd) = std::fs::read_dir(format!("/proc/{p}/task")) {
                    n += rd.count();
                }
            }
            println!("{:<28} {:>6} каталогов {:>7} мкс CPU", "readdir task/", pids.len(), util::thread_cpu_us() - c0);
            let _ = n;
        }
        "oneshot" => oneshot(args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5)),
        "parse" => {
            let (Some(kind), Some(file)) = (args.get(2), args.get(3)) else { usage() };
            let text = String::from_utf8_lossy(&std::fs::read(file).expect("файл")).into_owned();
            let v = match kind.as_str() {
                "usage" => serde_json::to_string_pretty(&sys::usage::parse(&text)),
                "checkin" => serde_json::to_string_pretty(&sys::checkin::parse(&text)),
                "suspend" => serde_json::to_string_pretty(&sys::native::parse_suspend(&text)),
                "proto" => {
                    let bytes = std::fs::read(file).expect("файл");
                    serde_json::to_string_pretty(&sys::checkin::parse_proto(&bytes))
                }
                "binder" | "looper" => {
                    let mut w = sys::binder::SysWork::default();
                    if kind == "binder" { sys::binder::parse_binder(&text, &mut w) } else { sys::binder::parse_looper(&text, &mut w) }
                    sys::binder::trim(&mut w);
                    serde_json::to_string_pretty(&w)
                }
                _ => usage(),
            };
            println!("{}", v.unwrap_or_default());
        }
        "version" | "--version" => {
            if args.iter().any(|a| a == "--build") {
                println!("{}", server::BUILD)
            } else {
                println!("{} (сборка {})", server::VERSION, server::BUILD)
            }
        }
        _ => usage(),
    }
}
