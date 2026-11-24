//! Батарея, экран, сон, источники пробуждения ядра.

use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Battery {
    /// Ток разряда, мА (>0 — разряд, <0 — заряд).
    pub draw_ma: f64,
    pub voltage_mv: f64,
    /// Кулоновский счётчик, мкА·ч (если есть).
    pub charge_uah: Option<i64>,
    pub level: u8,
    pub temp_c: f64,
    pub status: String,
    pub plugged: bool,
}

pub struct BatteryReader {
    base: String,
    supplies: Vec<String>,
    /// true — current_now в мкА, false — в мА (Samsung и др.).
    micro: Option<bool>,
}

impl BatteryReader {
    pub fn new() -> Self {
        let root = "/sys/class/power_supply";
        let mut base = format!("{root}/battery");
        let mut supplies = Vec::new();
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path().to_string_lossy().into_owned();
                let ty = util::read_str(format!("{p}/type")).unwrap_or_default();
                if ty == "Battery" && !std::path::Path::new(&base).exists() {
                    base = p.clone();
                }
                if matches!(ty.as_str(), "USB" | "Mains" | "Wireless" | "USB_PD" | "USB_C" | "USB_DCP" | "USB_CDP") || p.ends_with("/usb") || p.ends_with("/wireless") || p.ends_with("/ac") {
                    supplies.push(p);
                }
            }
        }
        BatteryReader { base, supplies, micro: None }
    }

    pub fn read(&mut self) -> Battery {
        let b = &self.base;
        let raw_cur = util::read_i64(format!("{b}/current_now")).unwrap_or(0);
        let micro = *self.micro.get_or_insert_with(|| raw_cur.abs() > 30_000);
        if !micro && raw_cur.abs() > 30_000 {
            self.micro = Some(true);
        }
        let ma = if self.micro == Some(true) { raw_cur as f64 / 1000.0 } else { raw_cur as f64 };
        let status = util::read_str(format!("{b}/status")).unwrap_or_default();
        let online = self.supplies.iter().any(|s| util::read_i64(format!("{s}/online")).unwrap_or(0) > 0);
        let plugged = online || status == "Charging" || status == "Full";
        // Знак у производителей разный: определяем по состоянию.
        let draw_ma = if status == "Charging" { -ma.abs() } else { ma.abs() };
        let uv = util::read_i64(format!("{b}/voltage_now")).unwrap_or(0);
        let voltage_mv = if uv > 100_000 { uv as f64 / 1000.0 } else { uv as f64 };
        let charge_uah = util::read_i64(format!("{b}/charge_counter")).filter(|&v| v > 0);
        let level = util::read_i64(format!("{b}/capacity")).unwrap_or(0).clamp(0, 100) as u8;
        let temp_c = util::read_i64(format!("{b}/temp")).unwrap_or(0) as f64 / 10.0;
        Battery { draw_ma, voltage_mv, charge_uah, level, temp_c, status, plugged }
    }

    pub fn design_capacity_mah(&self) -> Option<f64> {
        let v = util::read_i64(format!("{}/charge_full_design", self.base))?;
        Some(if v > 100_000 { v as f64 / 1000.0 } else { v as f64 })
    }
    pub fn full_capacity_mah(&self) -> Option<f64> {
        let v = util::read_i64(format!("{}/charge_full", self.base))?;
        Some(if v > 100_000 { v as f64 / 1000.0 } else { v as f64 })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub on: bool,
    /// Яркость 0..1
    pub bright: f64,
}

pub struct ScreenReader {
    dpms: Option<String>,
    backlight: Option<(String, f64)>,
    /// (0.8.1) Состояние дисплея по мнению Android — см. `read`.
    state: Option<crate::sys::prop::Prop>,
    state_tried_ms: u64,
}

/// Свойство, куда Android пишет состояние основного дисплея (Display.STATE_*): 1 — выключен,
/// 2 — включён, 3/4 — Always-On Display (doze / doze_suspend), 5 — VR, 6 — включён, но без обновления.
const SCREEN_STATE_PROP: &str = "debug.tracing.screen_state";
/// Свойства нет — ищем снова не чаще раза в минуту (только в «своём» файле, это дёшево).
const STATE_RETRY_MS: u64 = 60_000;

/// Значение `debug.tracing.screen_state` → экран включён? None — непонятное значение.
pub fn screen_state_on(v: &str) -> Option<bool> {
    match v.trim() {
        "2" | "5" | "6" => Some(true),
        "1" | "3" | "4" => Some(false),
        _ => None,
    }
}

impl ScreenReader {
    pub fn new() -> Self {
        let mut dpms = None;
        if let Ok(rd) = std::fs::read_dir("/sys/class/drm") {
            let mut c: Vec<String> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path().to_string_lossy().into_owned())
                .filter(|p| p.contains("-DSI-") || p.contains("-eDP-"))
                .collect();
            c.sort();
            for p in c {
                let f = format!("{p}/dpms");
                if std::path::Path::new(&f).exists() {
                    dpms = Some(f);
                    break;
                }
            }
        }
        let mut backlight = None;
        if let Ok(rd) = std::fs::read_dir("/sys/class/backlight") {
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path().to_string_lossy().into_owned();
                let max = util::read_i64(format!("{p}/max_brightness")).unwrap_or(0);
                if max > 0 {
                    backlight = Some((format!("{p}/brightness"), max as f64));
                    break;
                }
            }
        }
        let state = crate::sys::prop::Prop::find(SCREEN_STATE_PROP, true);
        ScreenReader { dpms, backlight, state, state_tried_ms: util::mono_ms() }
    }

    /// Включён ли экран. Главный источник — состояние дисплея, которое пишет сам Android: при
    /// Always-On Display панель остаётся под питанием (`dpms` = On, подсветка > 0), и по sysfs
    /// AOD не отличить от включённого экрана. sysfs — запасной путь, если свойства нет.
    pub fn read(&mut self) -> Screen {
        let bright = self
            .backlight
            .as_ref()
            .and_then(|(p, max)| util::read_i64(p).map(|v| (v as f64 / max).clamp(0.0, 1.0)))
            .unwrap_or(0.0);
        if self.state.is_none() && util::mono_ms().saturating_sub(self.state_tried_ms) >= STATE_RETRY_MS {
            self.state = crate::sys::prop::Prop::find(SCREEN_STATE_PROP, false);
            self.state_tried_ms = util::mono_ms();
        }
        if let Some(on) = self.state.as_ref().and_then(|p| p.get()).and_then(|v| screen_state_on(&v)) {
            return Screen { on, bright };
        }
        let on = match &self.dpms {
            Some(p) => util::read_str(p).map(|s| s == "On").unwrap_or(bright > 0.0),
            None => bright > 0.0,
        };
        Screen { on, bright }
    }
}

/// Счётчики источника пробуждения (/sys/class/wakeup/wakeupN).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct WsCounters {
    pub active_count: u64,
    pub wakeup_count: u64,
    pub total_ms: u64,
    pub prevent_ms: u64,
}

pub struct WakeupSources {
    dirs: Vec<(String, String)>,
    prev: HashMap<String, WsCounters>,
    last_enum_ms: u64,
    /// Подписи источников epoll («[timerfd] system_server · смена часов») по номеру wakeupN.
    owners: HashMap<u64, String>,
    /// Номера источников epoll, для которых сняты `owners`: пока набор тот же, /proc не сканируем.
    owners_ids: Vec<u64>,
    owners_tries: u8,
}

impl WakeupSources {
    pub fn new() -> Self {
        let mut w = WakeupSources {
            dirs: Vec::new(),
            prev: HashMap::new(),
            last_enum_ms: 0,
            owners: HashMap::new(),
            owners_ids: Vec::new(),
            owners_tries: 0,
        };
        w.enumerate(0);
        w
    }

    fn enumerate(&mut self, now: u64) {
        self.last_enum_ms = now;
        self.dirs.clear();
        let Ok(rd) = std::fs::read_dir("/sys/class/wakeup") else { return };
        // по номеру, а не по строке пути: wakeup76 раньше wakeup100, номера «#N» не скачут
        let mut v: Vec<(u64, String, String)> = rd
            .filter_map(|e| e.ok())
            .map(|e| {
                let id = e.file_name().to_str().and_then(|s| s.strip_prefix("wakeup")).and_then(|s| s.parse().ok());
                let p = e.path().to_string_lossy().into_owned();
                let mut name = util::read_str(format!("{p}/name")).unwrap_or_default();
                if name.is_empty() {
                    name = "<без имени>".into();
                }
                (id.unwrap_or(u64::MAX), p, name)
            })
            .collect();
        v.sort();
        let epoll: Vec<(u64, &str)> = v.iter().filter(|x| is_epoll_ws(&x.2)).map(|x| (x.0, x.2.as_str())).collect();
        let ids: Vec<u64> = epoll.iter().map(|x| x.0).collect();
        // счёт не сошёлся (демон как раз перезапускался) — ещё пара попыток на следующих перечислениях
        let incomplete = self.owners.len() < self.owners_ids.len() && self.owners_tries < 3;
        if ids != self.owners_ids || incomplete {
            self.owners_tries = if ids != self.owners_ids { 1 } else { self.owners_tries + 1 };
            self.owners = if ids.is_empty() { HashMap::new() } else { match_owners(&epoll, &scan_epoll_wakeups()) };
            self.owners_ids = ids;
        }
        let mut seen: HashMap<String, u32> = HashMap::new();
        for (id, p, name) in v {
            let base = self.owners.get(&id).cloned().unwrap_or(name);
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            let key = if *n > 1 { format!("{base} #{n}") } else { base };
            self.dirs.push((p, key));
        }
    }

    /// Дельты счётчиков с прошлого вызова (первый вызов — база, пусто).
    pub fn sample(&mut self, mono_ms: u64) -> Vec<(String, WsCounters)> {
        if mono_ms.saturating_sub(self.last_enum_ms) > 600_000 {
            self.enumerate(mono_ms);
        }
        let first = self.prev.is_empty();
        let mut out = Vec::new();
        for (p, key) in &self.dirs {
            let rd = |f: &str| util::read_u64(format!("{p}/{f}")).unwrap_or(0);
            let c = WsCounters {
                active_count: rd("active_count"),
                wakeup_count: rd("wakeup_count"),
                total_ms: rd("total_time_ms"),
                prevent_ms: rd("prevent_suspend_time_ms"),
            };
            if let Some(o) = self.prev.get(key) {
                let d = WsCounters {
                    active_count: c.active_count.saturating_sub(o.active_count),
                    wakeup_count: c.wakeup_count.saturating_sub(o.wakeup_count),
                    total_ms: c.total_ms.saturating_sub(o.total_ms),
                    prevent_ms: c.prevent_ms.saturating_sub(o.prevent_ms),
                };
                if d != WsCounters::default() {
                    out.push((key.clone(), d));
                }
            }
            self.prev.insert(key.clone(), c);
        }
        if first {
            out.clear();
        }
        out
    }
}

// ── Владельцы источников пробуждения epoll ──────────────────────────────────────────────────────
//
// Файл, добавленный в epoll с флагом EPOLLWAKEUP, получает свой источник пробуждения. Ядро называет
// его именем файла («[timerfd]», «[eventfd]») и не говорит, чей он, — отсюда девять одинаковых
// «[timerfd]». Владельца восстанавливаем по порядку: номера wakeupN ядро выдаёт по возрастанию, а
// такие таймеры заводят привилегированные демоны при старте. Записи epoll с EPOLLWAKEUP из
// /proc/*/fdinfo сортируем по (старт процесса, pid, fd) и сопоставляем источникам с тем же именем
// по номеру — только если их ровно столько же; иначе подпись не ставим, остаётся «#N».
// Проверено на Xiaomi 15 Ultra: 9 «[timerfd]» = 6 таймеров AlarmManager (system_server) +
// 2 audioserver + 1 HAL; у таймера смены часов wakeup_count совпал с числом засыпаний.

const EPOLLWAKEUP: u32 = 1 << 29;
const TFD_TIMER_CANCEL_ON_SET: u32 = 2;

/// Источник пробуждения от epoll: имя анонимного файла в квадратных скобках.
fn is_epoll_ws(name: &str) -> bool {
    name.len() > 2 && name.starts_with('[') && name.ends_with(']')
}

/// Файл в epoll с EPOLLWAKEUP: под него ядро держит источник пробуждения `name`.
#[derive(Debug, Clone)]
struct EpWake {
    start: u64,
    pid: u32,
    fd: u32,
    name: String,
    label: String,
}

/// Строки «tfd: N events: X …» из fdinfo epoll → (fd, маска событий).
fn parse_epoll_fdinfo(info: &str) -> Vec<(u32, u32)> {
    info.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            if it.next()? != "tfd:" {
                return None;
            }
            let fd = it.next()?.parse().ok()?;
            if it.next()? != "events:" {
                return None;
            }
            Some((fd, u32::from_str_radix(it.next()?, 16).ok()?))
        })
        .collect()
}

/// Что за таймер, по fdinfo timerfd. У system_server это таймеры AlarmManager: по одному на тип
/// будильника и ещё один, который ядро дёргает при смене часов и на каждом выходе из сна.
fn timer_what(owner: &str, info: &str) -> &'static str {
    let mut clock = -1i64;
    let mut flags = 0u32;
    for l in info.lines() {
        if let Some(v) = l.strip_prefix("clockid:") {
            clock = v.trim().parse().unwrap_or(-1);
        } else if let Some(v) = l.strip_prefix("settime flags:") {
            flags = u32::from_str_radix(v.trim(), 8).unwrap_or(0);
        }
    }
    let am = owner == "system_server";
    match clock {
        _ if flags & TFD_TIMER_CANCEL_ON_SET != 0 => "смена часов",
        9 if am => "будильники с пробуждением",
        9 => "будит",
        8 if am => "будильники по часам с пробуждением",
        8 => "будит по часам",
        7 if am => "будильники без пробуждения",
        0 if am => "будильники по часам без пробуждения",
        7 | 0 | 1 => "таймер без пробуждения",
        _ => "таймер",
    }
}

/// Время старта процесса (тики) и его имя: первое слово cmdline без пути, иначе comm.
fn proc_owner(pid: u32) -> (u64, String) {
    let stat = util::read_str(format!("/proc/{pid}/stat")).unwrap_or_default();
    let (comm, rest) = match (stat.find('('), stat.rfind(')')) {
        (Some(l), Some(r)) if l < r => (&stat[l + 1..r], stat.get(r + 2..).unwrap_or("")),
        _ => ("", ""),
    };
    // поля после ')': 0=state … 19=starttime
    let start = rest.split(' ').nth(19).and_then(|s| s.parse().ok()).unwrap_or(0);
    let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let first = String::from_utf8_lossy(cmd.split(|&c| c == 0).next().unwrap_or(&[])).trim().to_string();
    let first = first.split_whitespace().next().unwrap_or("");
    let name = first.rsplit('/').next().unwrap_or("");
    (start, if name.is_empty() { comm.to_string() } else { name.to_string() })
}

/// Все файлы в epoll с EPOLLWAKEUP по всем процессам. Читает fd всех процессов (~0,1–0,3 с), поэтому
/// зовётся только когда меняется набор источников epoll.
fn scan_epoll_wakeups() -> Vec<EpWake> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return out };
    for e in rd.filter_map(|e| e.ok()) {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else { continue };
        let mut owner: Option<(u64, String)> = None;
        // один экземпляр epoll под двумя номерами fd (dup) дал бы лишние записи
        let mut seen: Vec<Vec<(u32, u32)>> = Vec::new();
        for f in fds.filter_map(|f| f.ok()) {
            let Ok(t) = std::fs::read_link(f.path()) else { continue };
            if t.as_os_str() != "anon_inode:[eventpoll]" {
                continue;
            }
            let Some(fd) = f.file_name().to_str().map(str::to_string) else { continue };
            let Some(info) = util::read_str(format!("/proc/{pid}/fdinfo/{fd}")) else { continue };
            let items: Vec<(u32, u32)> = parse_epoll_fdinfo(&info).into_iter().filter(|x| x.1 & EPOLLWAKEUP != 0).collect();
            if items.is_empty() || seen.contains(&items) {
                continue;
            }
            for &(tfd, _) in &items {
                let Ok(target) = std::fs::read_link(format!("/proc/{pid}/fd/{tfd}")) else { continue };
                let target = target.to_string_lossy();
                let Some(name) = target.strip_prefix("anon_inode:") else { continue };
                let (start, who) = owner.get_or_insert_with(|| proc_owner(pid)).clone();
                let label = if name == "[timerfd]" {
                    let info = util::read_str(format!("/proc/{pid}/fdinfo/{tfd}")).unwrap_or_default();
                    format!("{name} {who} · {}", timer_what(&who, &info))
                } else {
                    format!("{name} {who}")
                };
                out.push(EpWake { start, pid, fd: tfd, name: name.to_string(), label });
            }
            seen.push(items);
        }
    }
    out
}

/// Подписи источникам epoll: `ws` — (номер wakeupN, имя) по возрастанию номера.
fn match_owners(ws: &[(u64, &str)], items: &[EpWake]) -> HashMap<u64, String> {
    let mut out = HashMap::new();
    let mut names: Vec<&str> = ws.iter().map(|x| x.1).collect();
    names.sort_unstable();
    names.dedup();
    for n in names {
        let ids: Vec<u64> = ws.iter().filter(|x| x.1 == n).map(|x| x.0).collect();
        let mut it: Vec<&EpWake> = items.iter().filter(|x| x.name == n).collect();
        if it.len() != ids.len() {
            continue; // счёт не сошёлся — порядок ничего не доказывает
        }
        it.sort_by_key(|x| (x.start, x.pid, x.fd));
        for (id, w) in ids.iter().zip(it) {
            out.insert(*id, w.label.clone());
        }
    }
    out
}

/// Сон: число удачных/неудачных засыпаний и последняя причина пробуждения.
pub struct SuspendReader {
    prev_ok: Option<u64>,
    prev_fail: Option<u64>,
    last_reason: String,
}

#[derive(Debug, Default, Clone)]
pub struct SuspendDelta {
    pub ok: u64,
    pub fail: u64,
    pub reason: Option<String>,
}

impl SuspendReader {
    pub fn new() -> Self {
        SuspendReader { prev_ok: None, prev_fail: None, last_reason: String::new() }
    }
    pub fn sample(&mut self) -> SuspendDelta {
        let ok = util::read_u64("/sys/power/suspend_stats/success").unwrap_or(0);
        let fail = util::read_u64("/sys/power/suspend_stats/fail").unwrap_or(0);
        let d_ok = self.prev_ok.map(|p| ok.saturating_sub(p)).unwrap_or(0);
        let d_fail = self.prev_fail.map(|p| fail.saturating_sub(p)).unwrap_or(0);
        self.prev_ok = Some(ok);
        self.prev_fail = Some(fail);
        let mut reason = None;
        if d_ok > 0 {
            let r = util::read_str("/sys/kernel/wakeup_reasons/last_resume_reason").unwrap_or_default();
            let r = r.lines().next().unwrap_or("").trim().to_string();
            if !r.is_empty() {
                self.last_reason = r.clone();
                reason = Some(r);
            }
        }
        SuspendDelta { ok: d_ok, fail: d_fail, reason }
    }
}

/// Загрузка GPU (Adreno kgsl): (busy, total) из gpubusy.
pub fn gpu_busy() -> Option<(u64, u64)> {
    // kgsl отдаёт «занят / всего» за последнее окно (~1 с), а не накопительные счётчики: разность двух
    // чтений даёт мусор (сотни процентов). Складываем окна в свои накопительные суммы — шаг берёт
    // разность, как у прочих счётчиков, и получает долю текущего окна. Окно без работы (GPU спит,
    // всего = 0) идёт за единицу простоя, чтобы разность не обнулялась.
    use std::sync::atomic::{AtomicU64, Ordering};
    static BUSY: AtomicU64 = AtomicU64::new(0);
    static TOTAL: AtomicU64 = AtomicU64::new(0);
    let s = util::read_str("/sys/class/kgsl/kgsl-3d0/gpubusy")?;
    let mut it = s.split_whitespace();
    let a: u64 = it.next()?.parse().ok()?;
    let b: u64 = it.next()?.parse().ok()?;
    let total = b.max(1);
    let busy = a.min(total);
    Some((BUSY.fetch_add(busy, Ordering::Relaxed) + busy, TOTAL.fetch_add(total, Ordering::Relaxed) + total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_state_values() {
        assert_eq!(screen_state_on("2"), Some(true));
        assert_eq!(screen_state_on("4"), Some(false)); // Always-On Display
        assert_eq!(screen_state_on("3"), Some(false));
        assert_eq!(screen_state_on("1"), Some(false));
        assert_eq!(screen_state_on("0"), None);
        assert_eq!(screen_state_on(""), None);
    }

    // fdinfo с Xiaomi 15 Ultra (system_server, AlarmManager)
    const EP: &str = "pos:\t0\nflags:\t02\nmnt_id:\t13\nino:\t2052\n\
tfd:      308 events: 20000019 data:                1  pos:0 ino:804 sdev:e\n\
tfd:      309 events: 20000019 data:                2  pos:0 ino:804 sdev:e\n\
tfd:      307 events: 20000019 data:                0  pos:0 ino:804 sdev:e\n\
tfd:       40 events:       19 data:                9  pos:0 ino:804 sdev:e\n";

    #[test]
    fn epoll_fdinfo() {
        let v = parse_epoll_fdinfo(EP);
        assert_eq!(v, vec![(308, 0x2000_0019), (309, 0x2000_0019), (307, 0x2000_0019), (40, 0x19)]);
        assert_eq!(v.iter().filter(|x| x.1 & EPOLLWAKEUP != 0).count(), 3);
    }

    #[test]
    fn timer_kinds() {
        let t = |c: u32, f: &str| format!("clockid: {c}\nticks: 0\nsettime flags: {f}\nit_value: (0, 0)\n");
        assert_eq!(timer_what("system_server", &t(0, "03")), "смена часов");
        assert_eq!(timer_what("system_server", &t(9, "01")), "будильники с пробуждением");
        assert_eq!(timer_what("system_server", &t(7, "01")), "будильники без пробуждения");
        assert_eq!(timer_what("system_server", &t(1, "00")), "таймер без пробуждения");
        assert_eq!(timer_what("audioserver", &t(9, "01")), "будит");
        assert_eq!(timer_what("audioserver", &t(7, "00")), "таймер без пробуждения");
        assert!(is_epoll_ws("[timerfd]") && !is_epoll_ws("qcom_rx_wakelock") && !is_epoll_ws("[]"));
    }

    fn w(start: u64, pid: u32, fd: u32, label: &str) -> EpWake {
        EpWake { start, pid, fd, name: "[timerfd]".into(), label: label.into() }
    }

    #[test]
    fn owners_by_order() {
        let ws = [(76, "[timerfd]"), (81, "[timerfd]"), (100, "[timerfd]"), (101, "[eventfd]")];
        // порядок записей не важен: сортируются по (старт, pid, fd)
        let items = [w(6670, 3827, 312, "C"), w(6447, 2408, 7, "A"), w(6670, 3827, 307, "B")];
        let m = match_owners(&ws, &items);
        assert_eq!(m.get(&76).map(String::as_str), Some("A"));
        assert_eq!(m.get(&81).map(String::as_str), Some("B"));
        assert_eq!(m.get(&100).map(String::as_str), Some("C"));
        assert!(!m.contains_key(&101), "[eventfd] без записей — без подписи");
        // одной записи не хватает — подписей нет совсем, а не со сдвигом
        assert!(match_owners(&ws, &items[..2]).is_empty());
    }
}
