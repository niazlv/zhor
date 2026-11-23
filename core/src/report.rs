//! Отчёт за интервал: кто, где, сколько и почему.

use crate::explain;
use crate::sampler::{Daemon, Event};
use crate::store::{Bucket, WsRec};
use crate::sys::{self, SysSnap};
use crate::util::{self, fmt_dur, mas_to_mah};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Query {
    /// since_unplug | screen_off | charge | 1h | 3h | 6h | 12h | 24h | 48h | all
    #[serde(default)]
    pub range: Option<String>,
    #[serde(default)]
    pub from: Option<i64>,
    #[serde(default)]
    pub to: Option<i64>,
    /// снять свежий системный снимок перед отчётом
    #[serde(default)]
    pub fresh: bool,
    /// только этот uid (детали приложения)
    #[serde(default)]
    pub uid: Option<u32>,
    #[serde(default)]
    pub max_apps: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ThrRep {
    pub name: String,
    pub cpu_ms: u64,
    pub mah: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ProcRep {
    pub name: String,
    pub uid: u32,
    pub cpu_ms: u64,
    pub mah: f64,
    pub mah_screen_off: f64,
    pub starts: u32,
    /// изолированный процесс (рендер WebView, песочница), отнесённый к приложению-хозяину
    pub isolated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_uid: Option<u32>,
    pub threads: Vec<ThrRep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Item {
    pub name: String,
    pub count: u64,
    pub ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppRep {
    pub uid: u32,
    pub label: String,
    pub packages: Vec<String>,
    pub kind: &'static str,
    /// итог: CPU по нашей модели + прочие компоненты из модели Android
    pub mah: f64,
    pub cpu_ms: u64,
    pub cpu_mah: f64,
    pub cpu_mah_screen_off: f64,
    /// компоненты (мА·ч) из batterystats за окно: gnss, mobile_radio, wifi, screen…
    pub components: BTreeMap<String, f64>,
    pub component_ms: BTreeMap<String, u64>,
    /// fg/bg/fgs/cached мА·ч из batterystats
    pub states: BTreeMap<String, f64>,
    pub sys_mah: f64,
    pub processes: Vec<ProcRep>,
    pub wakelocks: Vec<Item>,
    pub alarms: Vec<Item>,
    pub jobs: Vec<Item>,
    pub syncs: Vec<Item>,
    pub sensors: Vec<Item>,
    pub native_wakelocks: Vec<Item>,
    pub net: Option<sys::checkin::Net>,
    pub gpu_ms: u64,
    pub crashes: u64,
    pub anrs: u64,
    /// сколько CPU system_server ушло на это приложение (binder-вызовы + очереди от его имени), мс и ≈мА·ч
    #[serde(skip_serializing_if = "crate::util::is_zero_f")]
    pub system_ms: f64,
    #[serde(skip_serializing_if = "crate::util::is_zero_f")]
    pub system_mah: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub system_calls: Vec<SysCallRep>,
    pub why: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SysCallRep {
    /// «AccountManagerService#getUserData»
    pub call: String,
    pub cpu_ms: f64,
    pub calls: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SysCallerRep {
    pub uid: u32,
    pub label: String,
    pub packages: Vec<String>,
    pub kind: &'static str,
    /// CPU system_server: binder-вызовы этого uid / работа очередей от его имени / всего, мс
    pub binder_ms: f64,
    pub queue_ms: f64,
    pub cpu_ms: f64,
    /// ≈мА·ч по средней цене CPU system_server за окно
    pub mah: f64,
    /// доля от всей работы system_server (0..1)
    pub share: f64,
    pub calls: u64,
    pub top_calls: Vec<SysCallRep>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SysMsgRep {
    pub handler: String,
    pub message: String,
    pub cpu_ms: f64,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SysQueueRep {
    pub thread: String,
    pub cpu_ms: f64,
    pub share: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<&'static str>,
    pub top: Vec<SysMsgRep>,
}

/// «За кого работает system»: CPU system_server по вызывающим приложениям и внутренним очередям.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SysWorkRep {
    pub from: i64,
    pub to: i64,
    pub exact: bool,
    pub note: String,
    /// всего CPU system_server: на binder-вызовы / в очередях, мс (выборка Android, масштабирована)
    pub binder_ms: f64,
    pub queue_ms: f64,
    /// очереди, работавшие «за саму систему» (work source −1), мс
    pub own_ms: f64,
    /// мА·с на 1 мс CPU system_server (из записей ядра за окно) — перевод в мА·ч
    pub mas_per_cpu_ms: f64,
    pub mah_total: f64,
    pub callers: Vec<SysCallerRep>,
    pub queues: Vec<SysQueueRep>,
    /// включённые службы спецвозможностей: из-за них все приложения шлют события в system_server
    pub a11y: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Totals {
    pub span_ms: u64,
    pub awake_ms: u64,
    pub suspend_ms: u64,
    /// экран включён / выключен — только без зарядки: экран вкл. + экран выкл. + зарядка = весь период
    pub screen_on_ms: u64,
    pub screen_off_ms: u64,
    /// глубокий сон в покое (экран выключен, без зарядки); на зарядке телефон и не обязан спать
    pub screen_off_suspend_ms: u64,
    pub plugged_ms: u64,
    /// по кулоновскому счётчику (только без зарядки)
    pub measured_mah: f64,
    pub measured_avg_ma: f64,
    pub measured_screen_on_ma: f64,
    pub measured_screen_off_ma: f64,
    pub measured_source: &'static str,
    pub model_cpu_mah: f64,
    pub model_cpu_idle_mah: f64,
    pub model_screen_mah: f64,
    pub model_total_mah: f64,
    /// измерено − модель CPU/экрана: радио, GPS, Wi-Fi, камера, периферия…
    pub other_mah: f64,
    pub level_start: u8,
    pub level_end: u8,
    pub pct_per_hour: f64,
    pub pct_per_hour_screen_off: f64,
    pub deep_sleep_pct: f64,
    pub suspends: u64,
    pub suspend_fails: u64,
    pub gpu_busy_pct: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ClusterRep {
    pub policy: String,
    pub cpus: Vec<u32>,
    pub cpu_ms: u64,
    pub mah: f64,
    /// [кГц, мс]
    pub residency: Vec<(u64, u64)>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct KernelRep {
    pub wakeup_sources: Vec<WsItem>,
    pub resume_reasons: Vec<Item>,
    pub wakeup_reasons: Vec<Item>,
    pub kernel_wakelocks: Vec<Item>,
    pub native_wakelocks: Vec<Item>,
    pub suspend_abort_reasons: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct WsItem {
    pub name: String,
    pub wakeups: u64,
    pub activations: u64,
    pub held_ms: u64,
    pub prevent_suspend_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TimePoint {
    pub t0: i64,
    pub t1: i64,
    pub screen: bool,
    pub plugged: bool,
    pub level: u8,
    pub meas_ma: f64,
    pub model_ma: f64,
    pub cpu_ma: f64,
    pub awake_pct: f64,
    pub suspend_pct: f64,
    /// крупнейший потребитель (подпись)
    pub top: String,
    /// топ-3 потребителя CPU в этой точке, средний мА
    pub tops: Vec<TopRep>,
    /// только в отчёте по одному приложению (`uid`): его CPU в этой точке, средний мА
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_ma: Option<f64>,
    /// и его самый затратный процесс в этой точке
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_top: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TopRep {
    pub uid: u32,
    pub label: String,
    pub ma: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SysWindow {
    /// разряд за окно по данным Android (кулоновский счётчик в batterystats), мА·ч
    pub actual_mah: f64,
    pub from: i64,
    pub to: i64,
    pub exact: bool,
    pub note: String,
    pub global: BTreeMap<String, f64>,
    pub global_ms: BTreeMap<String, u64>,
    pub computed_mah: f64,
    pub doze_ms: u64,
    pub partial_wakelock_ms: u64,
    pub mobile_radio_active_ms: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Insight {
    /// bad | warn | info | good
    pub level: &'static str,
    pub title: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// sleep | apps | kernel | cpu | battery
    pub section: &'static str,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Report {
    /// с какого момента есть записи ядра в этом интервале (процессы, потоки, ток)
    pub recorded_from: i64,
    pub from: i64,
    pub to: i64,
    pub label: String,
    pub generated: i64,
    pub device: BTreeMap<String, String>,
    pub totals: Totals,
    pub apps: Vec<AppRep>,
    pub kernel: KernelRep,
    pub clusters: Vec<ClusterRep>,
    pub timeline: Vec<TimePoint>,
    pub sys: Option<SysWindow>,
    pub notes: Vec<String>,
    pub events: Vec<Event>,
    pub insights: Vec<Insight>,
    /// за кого работает system_server (binder_calls_stats + looper_stats за окно)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_work: Option<SysWorkRep>,
    /// история событий Android за окно (см. history.rs); при `uid` — только это приложение
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<crate::history::HistRep>,
}

/// Начало и конец интервала по пресету.
pub fn resolve_range(d: &Daemon, q: &Query, latest_snap: Option<&SysSnap>) -> (i64, i64, String) {
    let now = util::real_ms();
    let to = q.to.unwrap_or(now);
    if let Some(f) = q.from {
        return (f, to, "свой интервал".into());
    }
    let preset = q.range.as_deref().unwrap_or("since_unplug");
    let hours = |h: i64| (now - h * 3_600_000, now, format!("последние {h} ч"));
    match preset {
        "1h" => hours(1),
        "3h" => hours(3),
        "6h" => hours(6),
        "12h" => hours(12),
        "24h" => hours(24),
        "48h" => hours(48),
        "all" => (d.store.oldest().unwrap_or(d.started), now, "всё время записи".into()),
        "charge" => {
            let f = latest_snap.map(|s| s.checkin.start_clock_ms).filter(|&t| t > 0).unwrap_or(d.started);
            (f, now, "с последней полной зарядки (batterystats)".into())
        }
        "screen_off" => {
            // последний период с выключенным экраном
            let ev: Vec<&Event> = d.events.iter().filter(|e| e.kind == "screen_off" || e.kind == "screen_on").collect();
            let cur_off = d.current().map(|(_, s)| !s.on).unwrap_or(false);
            if cur_off {
                if let Some(e) = ev.iter().rev().find(|e| e.kind == "screen_off") {
                    return (e.t, now, "текущий период с выключенным экраном".into());
                }
            } else if let Some(i) = ev.iter().rposition(|e| e.kind == "screen_off") {
                let end = ev.get(i + 1).map(|e| e.t).unwrap_or(now);
                return (ev[i].t, end, "последний период с выключенным экраном".into());
            }
            hours(1)
        }
        _ => {
            // since_unplug
            let plugged_now = d.current().map(|(b, _)| b.plugged).unwrap_or(false);
            if !plugged_now {
                if let Some(e) = d.events.iter().rev().find(|e| e.kind == "unplug") {
                    return (e.t, now, "с отключения зарядки".into());
                }
            }
            let f = latest_snap.map(|s| s.checkin.start_clock_ms).filter(|&t| t > 0).unwrap_or(d.store.oldest().unwrap_or(d.started));
            (f.max(now - 48 * 3_600_000), now, "с последней зарядки".into())
        }
    }
}

#[derive(Default)]
struct PAgg {
    uid: u32,
    host: Option<u32>,
    kernel: bool,
    t: f64,
    e: f64,
    e_off: f64,
    st: f64,
    th: HashMap<String, (f64, f64)>,
}

/// pkg → uid (для изолированных процессов по имени).
fn pkg_index(pkgs: &BTreeMap<u32, Vec<String>>) -> HashMap<(u32, String), u32> {
    let mut m = HashMap::new();
    for (uid, ps) in pkgs {
        for p in ps {
            m.insert((uid / 100_000, p.clone()), *uid);
        }
    }
    m
}

fn owner_uid(uid: u32, name: &str, host: Option<u32>, idx: &HashMap<(u32, String), u32>) -> u32 {
    if let Some(h) = host {
        return h;
    }
    if explain::uid_kind(uid) == "isolated" {
        let pkg = name.split(':').next().unwrap_or(name);
        // по имени — только собственные песочницы приложения (com.app:sandboxed…);
        // рендер WebView без известного хозяина самому WebView не приписываем
        if !pkg.contains("webview") {
            if let Some(&u) = idx.get(&(uid / 100_000, pkg.to_string())) {
                return u;
            }
        }
    }
    uid
}

pub fn label_for(uid: u32, pkgs: &BTreeMap<u32, Vec<String>>) -> (String, Vec<String>) {
    let ps = pkgs.get(&uid).cloned().unwrap_or_default();
    let user = uid / 100_000;
    let suffix = if user > 0 { format!(" (польз. {user})") } else { String::new() };
    if let Some(a) = explain::aid_name(uid) {
        return (format!("{a}{suffix}"), ps);
    }
    if ps.len() == 1 {
        return (format!("{}{suffix}", ps[0]), ps);
    }
    if !ps.is_empty() {
        return (format!("{} +{}{suffix}", ps[0], ps.len() - 1), ps);
    }
    if explain::uid_kind(uid) == "isolated" {
        return (format!("изолированный процесс {uid}"), ps);
    }
    (format!("uid {uid}"), ps)
}

fn top_items<I: IntoIterator<Item = Item>>(it: I, n: usize) -> Vec<Item> {
    let mut v: Vec<Item> = it.into_iter().filter(|i| i.ms > 0 || i.count > 0).collect();
    v.sort_by(|a, b| b.ms.cmp(&a.ms).then(b.count.cmp(&a.count)));
    v.truncate(n);
    v
}

/// Выбор пары снимков вокруг интервала.
fn sys_window(snaps: &[&SysSnap], from: i64, to: i64) -> Option<(SysSnap, SysWindow)> {
    if snaps.is_empty() {
        return None;
    }
    let b = snaps.iter().rev().find(|s| s.t <= to + 120_000).copied().or_else(|| snaps.last().copied())?;
    let a = snaps.iter().rev().find(|s| s.t <= from + 120_000 && s.t < b.t).copied();
    let (delta, exact, note, wf) = match a {
        Some(a) => {
            let exact = (a.t - from).abs() < 300_000;
            let note = if exact {
                String::new()
            } else {
                format!("Системные данные — с {} (ближайший снимок до начала интервала).", fmt_time(a.t))
            };
            (sys::delta(a, b), exact, note, a.t)
        }
        None => {
            // нет снимка до начала: берём абсолют «с зарядки», если интервал от неё
            let st = b.checkin.start_clock_ms;
            if st > 0 && (st - from).abs() < 600_000 {
                let mut x = b.clone();
                // счётчики службы засыпания и GPU — «с загрузки»: годятся, только если загрузка внутри окна
                let boot_wall = b.t - b.boot_ms as i64;
                if boot_wall < from - 600_000 {
                    x.suspend = Default::default();
                    x.gpu_ms.clear();
                }
                (x, true, String::new(), st)
            } else if let Some(first) = snaps.iter().find(|s| s.t < b.t) {
                let note = format!("Системные данные — с {} (раньше снимков нет).", fmt_time(first.t));
                (sys::delta(first, b), false, note, first.t)
            } else {
                let note = "Системные данные — «с последней зарядки» (снимок пока один).".to_string();
                let mut x = b.clone();
                x.suspend = Default::default();
                x.gpu_ms.clear();
                (x, false, note, st)
            }
        }
    };
    let mut w = SysWindow { from: wf, to: b.t, exact, note, ..Default::default() };
    for (k, c) in &delta.usage.global {
        w.global.insert(k.clone(), c.mah);
        if c.ms > 0 {
            w.global_ms.insert(k.clone(), c.ms);
        }
    }
    w.computed_mah = delta.usage.computed;
    w.actual_mah = (delta.usage.actual_lo + delta.usage.actual_hi) / 2.0;
    w.doze_ms = delta.checkin.doze_full_ms + delta.checkin.doze_light_ms;
    w.partial_wakelock_ms = delta.checkin.partial_wl_total_ms;
    w.mobile_radio_active_ms = delta.checkin.mobile_radio_active_ms;
    Some((delta, w))
}

pub fn fmt_time(t: i64) -> String {
    // локальное время телефона
    let secs = t / 1000;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt: libc::time_t = secs as libc::time_t;
    unsafe {
        libc::localtime_r(&tt, &mut tm);
    }
    format!("{:02}.{:02} {:02}:{:02}", tm.tm_mday, tm.tm_mon + 1, tm.tm_hour, tm.tm_min)
}

pub fn build(d: &Daemon, q: &Query, extra_snap: Option<&SysSnap>) -> Report {
    let mut snaps: Vec<&SysSnap> = d.snaps.iter().collect();
    if let Some(s) = extra_snap {
        snaps.push(s);
    }
    snaps.sort_by_key(|s| s.t);
    let (from, to, label) = resolve_range(d, q, snaps.last().copied());
    let mut buckets = d.store.range(from, to);
    if let Some(ob) = d.open_snapshot() {
        if ob.t1 > from && ob.t0 < to {
            buckets.push(ob);
        }
    }
    let pkgs = &d.pkgs;
    let idx = pkg_index(pkgs);
    let recorded_from = buckets.iter().map(|b| b.t0).min().unwrap_or(to).max(from);
    let mut rep = Report { recorded_from, from, to, label, generated: util::real_ms(), ..Default::default() };
    if recorded_from > from + 5 * 60_000 && recorded_from < to {
        rep.notes.push(format!(
            "Ядро записывает с {}: процессы, потоки и ток — только с этого момента; данные Android (batterystats) — за весь интервал.",
            fmt_time(recorded_from)
        ));
    }
    rep.device.insert("profile".into(), d.pp.source.clone());
    if let Some(c) = d.full_mah {
        rep.device.insert("battery_full_mah".into(), format!("{c:.0}"));
    }
    if let Some(c) = d.design_mah {
        rep.device.insert("battery_design_mah".into(), format!("{c:.0}"));
    }
    rep.device.insert("daemon_since".into(), d.started.to_string());
    rep.device.insert("scan_cost_us".into(), d.last_scan_cost_us.to_string());

    // ---- агрегация корзин ----
    let tot = &mut rep.totals;
    let mut procs: HashMap<(u32, String, Option<u32>), PAgg> = HashMap::new();
    let mut ws: HashMap<String, WsRec> = HashMap::new();
    let mut rr: HashMap<String, f64> = HashMap::new();
    let nclusters = d.pp.clusters.len();
    let mut freq: Vec<Vec<f64>> = d.pp.clusters.iter().map(|c| vec![0.0; c.freqs_khz.len()]).collect();
    let (mut cc_uah, mut cc_ok_ms, mut cur_unpl, mut unpl_ms) = (0f64, 0u64, 0f64, 0u64);
    let (mut on_mas_meas, mut on_ms_meas, mut off_mas_meas, mut off_ms_meas) = (0f64, 0u64, 0f64, 0u64);
    let (mut m_cpu, mut m_base, mut m_scr) = (0f64, 0f64, 0f64);
    let (mut gpu_w, mut gpu_ms) = (0f64, 0f64);
    let mut level_drop_on = 0f64;
    let mut level_drop_off = 0f64;
    let mut timeline_src: Vec<(Bucket, f64)> = Vec::new();
    for b in &buckets {
        let len = (b.t1 - b.t0).max(1) as f64;
        let ov = ((b.t1.min(to) - b.t0.max(from)) as f64 / len).clamp(0.0, 1.0);
        if ov <= 0.0 {
            continue;
        }
        let span = b.span as f64 * ov;
        tot.span_ms += span as u64;
        tot.awake_ms += (b.awake as f64 * ov) as u64;
        tot.suspend_ms += (b.susp as f64 * ov) as u64;
        if b.plug {
            // на зарядке ни экран, ни сон в расход батареи не входят — считаем только как «зарядку»
        } else if b.scr {
            tot.screen_on_ms += span as u64;
        } else {
            tot.screen_off_ms += span as u64;
            tot.screen_off_suspend_ms += (b.susp as f64 * ov) as u64;
        }
        if b.plug {
            tot.plugged_ms += span as u64;
        } else {
            unpl_ms += span as u64;
            if b.cc_ok {
                cc_uah += b.cc_uah as f64 * ov;
                cc_ok_ms += span as u64;
            }
            cur_unpl += b.cur_mas * ov;
            let meas_mas = if b.cc_ok { b.cc_uah as f64 * 3.6 * ov } else { b.cur_mas * ov };
            if b.scr {
                on_mas_meas += meas_mas;
                on_ms_meas += span as u64;
                level_drop_on += (b.lvl0 as f64 - b.lvl1 as f64) * ov;
            } else {
                off_mas_meas += meas_mas;
                off_ms_meas += span as u64;
                level_drop_off += (b.lvl0 as f64 - b.lvl1 as f64) * ov;
            }
            m_cpu += b.cpu_mas * ov;
            m_base += b.base_mas * ov;
            m_scr += b.scr_mas * ov;
        }
        if tot.level_start == 0 {
            tot.level_start = b.lvl0;
        }
        tot.level_end = b.lvl1;
        tot.suspends += (b.sok as f64 * ov) as u64;
        tot.suspend_fails += (b.sfail as f64 * ov) as u64;
        // старые сборки ядра складывали оконные значения kgsl как счётчики — в их корзинах бывает > 1
        gpu_w += b.gpu.clamp(0.0, 1.0) * b.awake as f64 * ov;
        gpu_ms += b.awake as f64 * ov;
        for p in &b.procs {
            let a = procs.entry((p.uid, p.n.clone(), p.h)).or_default();
            a.uid = p.uid;
            a.host = p.h;
            a.kernel = p.k;
            a.t += p.t as f64 * ov;
            a.e += p.e * ov;
            if !b.scr {
                a.e_off += p.e * ov;
            }
            a.st += p.st as f64 * ov;
            for t in &p.th {
                let x = a.th.entry(t.n.clone()).or_default();
                x.0 += t.t as f64 * ov;
                x.1 += t.e * ov;
            }
        }
        for w in &b.ws {
            let x = ws.entry(w.n.clone()).or_insert_with(|| WsRec { n: w.n.clone(), ..Default::default() });
            x.ac += (w.ac as f64 * ov) as u64;
            x.wk += (w.wk as f64 * ov) as u64;
            x.ms += (w.ms as f64 * ov) as u64;
            x.pv += (w.pv as f64 * ov) as u64;
        }
        for (r, c) in &b.rr {
            *rr.entry(r.clone()).or_default() += *c as f64 * ov;
        }
        for (ci, v) in b.freq.iter().enumerate().take(nclusters) {
            for (i, x) in v.iter().enumerate() {
                if let Some(dst) = freq[ci].get_mut(i) {
                    *dst += *x as f64 * ov;
                }
            }
        }
        timeline_src.push((b.clone(), ov));
    }
    let tot = &mut rep.totals;
    if cc_ok_ms > 0 && cc_ok_ms * 10 >= unpl_ms * 9 {
        tot.measured_mah = cc_uah / 1000.0;
        tot.measured_source = "кулоновский счётчик батареи";
    } else if unpl_ms > 0 {
        tot.measured_mah = mas_to_mah(cur_unpl);
        tot.measured_source = "интеграл тока (только бодрствование)";
    } else {
        tot.measured_source = "нет данных без зарядки";
    }
    if unpl_ms > 0 {
        tot.measured_avg_ma = tot.measured_mah / (unpl_ms as f64 / 3_600_000.0);
        let h = unpl_ms as f64 / 3_600_000.0;
        tot.pct_per_hour = (level_drop_on + level_drop_off) / h;
    }
    if on_ms_meas > 0 {
        tot.measured_screen_on_ma = on_mas_meas / (on_ms_meas as f64 / 1000.0);
    }
    if off_ms_meas > 0 {
        tot.measured_screen_off_ma = off_mas_meas / (off_ms_meas as f64 / 1000.0);
        tot.pct_per_hour_screen_off = level_drop_off / (off_ms_meas as f64 / 3_600_000.0);
    }
    tot.model_cpu_mah = mas_to_mah(m_cpu);
    tot.model_cpu_idle_mah = mas_to_mah(m_base);
    tot.model_screen_mah = mas_to_mah(m_scr);
    tot.model_total_mah = tot.model_cpu_mah + tot.model_cpu_idle_mah + tot.model_screen_mah;
    tot.other_mah = (tot.measured_mah - tot.model_total_mah).max(0.0);
    if tot.screen_off_ms > 0 {
        tot.deep_sleep_pct = 100.0 * tot.screen_off_suspend_ms as f64 / tot.screen_off_ms as f64;
    }
    if gpu_ms > 0.0 {
        tot.gpu_busy_pct = 100.0 * gpu_w / gpu_ms;
    }
    if tot.model_total_mah > tot.measured_mah * 1.3 && tot.measured_mah > 1.0 {
        rep.notes.push("Модель CPU даёт больше, чем ушло из батареи: коэффициенты производителя завышены. Сравнивайте приложения между собой — доли верны.".into());
    }

    // ---- кластеры ----
    for (ci, c) in d.pp.clusters.iter().enumerate() {
        let tick_ms = 10.0;
        let mut cr = ClusterRep { policy: c.policy.clone(), cpus: c.cpus.clone(), ..Default::default() };
        for (i, &t) in freq[ci].iter().enumerate() {
            let ms = (t * tick_ms) as u64;
            cr.cpu_ms += ms;
            cr.mah += mas_to_mah(t * tick_ms / 1000.0 * c.ma.get(i).copied().unwrap_or(0.0));
            cr.residency.push((c.freqs_khz[i], ms));
        }
        rep.clusters.push(cr);
    }

    // ---- системный снимок за окно ----
    let sw = sys_window(&snaps, from, to);
    let delta = sw.as_ref().map(|(d, _)| d);
    if let Some((_, w)) = &sw {
        if !w.note.is_empty() {
            rep.notes.push(w.note.clone());
        }
    }

    // ---- за кого работает system ----
    // цена 1 мс CPU system_server по нашим записям за окно (мА·с/мс); нет записей — средний ток кластера
    let (ss_e, ss_t) = procs
        .iter()
        .filter(|((uid, name, _), _)| uid % 100_000 == 1000 && name == "system_server")
        .fold((0.0, 0.0), |acc, (_, a)| (acc.0 + a.e, acc.1 + a.t));
    let k = if ss_t > 1000.0 { ss_e / ss_t } else { d.pp.mid_ma(0) / 1000.0 };
    rep.system_work = system_work(&snaps, from, to, k, pkgs);
    let sys_by_uid: HashMap<u32, (f64, f64, Vec<SysCallRep>)> = rep
        .system_work
        .as_ref()
        .map(|w| w.callers.iter().map(|c| (c.uid, (c.cpu_ms, c.mah, c.top_calls.clone()))).collect())
        .unwrap_or_default();

    // ---- приложения ----
    let mut apps: HashMap<u32, AppRep> = HashMap::new();
    for ((uid, name, host), a) in procs {
        let owner = owner_uid(uid, &name, host, &idx);
        let app = apps.entry(owner).or_insert_with(|| {
            let (label, packages) = label_for(owner, pkgs);
            AppRep { uid: owner, label, packages, kind: explain::uid_kind(owner), ..Default::default() }
        });
        let mut threads: Vec<ThrRep> = a
            .th
            .into_iter()
            .map(|(n, (t, e))| ThrRep { explain: explain::thread(&n), name: n, cpu_ms: t as u64, mah: mas_to_mah(e) })
            .filter(|t| t.cpu_ms > 0)
            .collect();
        threads.sort_by(|x, y| y.mah.partial_cmp(&x.mah).unwrap_or(std::cmp::Ordering::Equal));
        threads.truncate(25);
        let isolated = explain::uid_kind(uid) == "isolated";
        let pr = ProcRep {
            explain: explain::process(&name),
            name: if owner != uid {
                if name.starts_with("com.google.android.webview") || name.starts_with("com.android.webview") {
                    format!("WebView: {} [uid {uid}]", name.split(':').nth(1).unwrap_or(&name))
                } else {
                    format!("{name} [uid {uid}]")
                }
            } else {
                name
            },
            uid,
            isolated,
            host_uid: if owner != uid { Some(owner) } else { None },
            cpu_ms: a.t as u64,
            mah: mas_to_mah(a.e),
            mah_screen_off: mas_to_mah(a.e_off),
            starts: a.st.round() as u32,
            threads,
        };
        app.cpu_ms += pr.cpu_ms;
        app.cpu_mah += pr.mah;
        app.cpu_mah_screen_off += pr.mah_screen_off;
        app.processes.push(pr);
    }
    if let Some(dl) = delta {
        // приложения, которых нет в нашем учёте CPU, но есть в batterystats
        for (&uid, u) in &dl.usage.uids {
            if u.total > 0.001 && !apps.contains_key(&uid) {
                let (label, packages) = label_for(uid, pkgs);
                apps.insert(uid, AppRep { uid, label, packages, kind: explain::uid_kind(uid), ..Default::default() });
            }
        }
    }
    for app in apps.values_mut() {
        app.processes.sort_by(|x, y| y.mah.partial_cmp(&x.mah).unwrap_or(std::cmp::Ordering::Equal));
        fill_sys(app, delta);
        if let Some((ms, mah, calls)) = sys_by_uid.get(&app.uid) {
            app.system_ms = *ms;
            app.system_mah = *mah;
            app.system_calls = calls.clone();
        }
        let other: f64 = app.components.iter().filter(|(k, _)| k.as_str() != "cpu").map(|(_, v)| *v).sum();
        app.mah = app.cpu_mah + other;
        if app.cpu_mah == 0.0 {
            if let Some(c) = app.components.get("cpu") {
                app.mah += c;
            }
        }
        app.why = why(app, &rep.totals);
        if app.uid % 100_000 == 1000 {
            if let Some(w) = &rep.system_work {
                let top: Vec<String> = w
                    .callers
                    .iter()
                    .filter(|c| c.uid % 100_000 != 1000)
                    .take(4)
                    .map(|c| format!("{} {:.0}%", c.label, 100.0 * c.share))
                    .collect();
                if !top.is_empty() {
                    app.why.insert(
                        0,
                        format!(
                            "system_server в основном работает по заказу других: {}. Подробно — «За кого работает system».",
                            top.join(", ")
                        ),
                    );
                }
            }
        }
    }
    let mut apps: Vec<AppRep> = apps.into_values().filter(|a| a.mah > 0.0005 || a.cpu_ms > 0).collect();
    apps.sort_by(|x, y| y.mah.partial_cmp(&x.mah).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(u) = q.uid {
        apps.retain(|a| a.uid == u);
    } else if let Some(n) = q.max_apps {
        apps.truncate(n);
    }
    rep.apps = apps;

    // ---- ядро и сон ----
    let mut wsv: Vec<WsItem> = ws
        .into_values()
        .filter(|w| w.ms > 0 || w.wk > 0 || w.ac > 0)
        .map(|w| WsItem { explain: explain::wakeup_source(&w.n), name: w.n, wakeups: w.wk, activations: w.ac, held_ms: w.ms, prevent_suspend_ms: w.pv })
        .collect();
    wsv.sort_by(|a, b| (b.held_ms + b.wakeups * 1000).cmp(&(a.held_ms + a.wakeups * 1000)));
    wsv.truncate(60);
    rep.kernel.wakeup_sources = wsv;
    rep.kernel.resume_reasons = top_items(
        rr.into_iter().map(|(r, c)| Item { explain: explain::wakeup_reason(&r), name: r, count: c.round() as u64, ms: 0 }),
        30,
    );
    if let Some(dl) = delta {
        rep.kernel.wakeup_reasons = top_items(
            dl.checkin.wakeup_reasons.iter().map(|(r, t)| Item { explain: explain::wakeup_reason(r), name: r.clone(), count: t.count, ms: t.ms }),
            40,
        );
        rep.kernel.kernel_wakelocks = top_items(
            dl.checkin.kernel_wl.iter().map(|(r, t)| Item {
                explain: explain::wakeup_source(r).map(|s| s.to_string()),
                name: if r.is_empty() { "<без имени>".into() } else { r.clone() },
                count: t.count,
                ms: t.ms,
            }),
            40,
        );
        rep.kernel.native_wakelocks = top_items(
            dl.suspend.native.values().map(|w| {
                let proc = dl.pid_names.get(&w.pid).map(|(_, n)| n.clone()).unwrap_or_else(|| "?".into());
                Item { name: format!("{} — {} (pid {})", w.name, proc, w.pid), count: w.count, ms: w.ms, explain: None }
            }),
            40,
        );
        rep.kernel.suspend_abort_reasons =
            top_items(dl.suspend.wakeups.iter().map(|(r, c)| Item { explain: explain::wakeup_reason(r), name: r.clone(), count: *c, ms: 0 }), 20);
    }
    rep.sys = sw.map(|(_, w)| w);

    // ---- лента ----
    let owner = |uid: u32, name: &str, host: Option<u32>| owner_uid(uid, name, host, &idx);
    let label = |uid: u32| label_for(uid, pkgs).0;
    rep.timeline = timeline(&timeline_src, 240, &owner, &label, q.uid);
    rep.events = d.events.iter().filter(|e| e.t >= from && e.t <= to).cloned().collect();
    rep.history = d.hist.lock().ok().and_then(|mut h| crate::history::report(&mut h, from, to, q.uid, &label));
    rep.insights = insights(&rep);
    if rep.totals.span_ms == 0 {
        rep.notes.push("За этот интервал записей демона нет (демон не работал). Системные данные — из снимков batterystats.".into());
    }
    rep
}

fn fill_sys(app: &mut AppRep, delta: Option<&SysSnap>) {
    let Some(dl) = delta else { return };
    let uid = app.uid;
    if let Some(u) = dl.usage.uids.get(&uid) {
        app.sys_mah = u.total;
        for (k, c) in &u.comps {
            if !k.contains(':') && c.mah > 0.0 {
                app.components.insert(k.clone(), c.mah);
                if c.ms > 0 {
                    app.component_ms.insert(k.clone(), c.ms);
                }
            }
        }
        for (k, c) in &u.states {
            app.states.insert(k.clone(), c.mah);
        }
    }
    if let Some(c) = dl.checkin.uids.get(&uid) {
        app.wakelocks = top_items(
            c.wakelocks.iter().map(|(n, w)| Item { explain: explain::wakelock(n), name: n.clone(), count: w.count + w.full_count, ms: w.ms + w.full_ms }),
            30,
        );
        let mut alarms: BTreeMap<String, u64> = c.alarms.clone();
        for (p, n) in &c.apk_wakeups {
            if alarms.is_empty() {
                alarms.insert(format!("{p} (всего)"), *n);
            }
        }
        app.alarms = top_items(alarms.into_iter().map(|(n, k)| Item { explain: explain::wakelock(&n), name: n, count: k, ms: 0 }), 30);
        app.alarms.sort_by(|a, b| b.count.cmp(&a.count));
        app.jobs = top_items(c.jobs.iter().map(|(n, t)| Item { name: n.clone(), count: t.count, ms: t.ms, explain: None }), 30);
        app.syncs = top_items(c.syncs.iter().map(|(n, t)| Item { name: n.clone(), count: t.count, ms: t.ms, explain: None }), 20);
        app.sensors = top_items(c.sensors.iter().map(|(h, t)| Item { name: explain::sensor(h), count: t.count, ms: t.ms, explain: None }), 20);
        let n = &c.net;
        if n.mobile_rx + n.mobile_tx + n.wifi_rx + n.wifi_tx > 0 {
            app.net = Some(n.clone());
        }
        app.crashes = c.procs.values().map(|p| p.crashes).sum();
        app.anrs = c.procs.values().map(|p| p.anrs).sum();
    }
    app.native_wakelocks = top_items(
        dl.suspend.native.values().filter(|w| dl.pid_names.get(&w.pid).map(|(u, _)| *u == uid).unwrap_or(false)).map(|w| Item {
            name: format!("{} (pid {})", w.name, w.pid),
            count: w.count,
            ms: w.ms,
            explain: None,
        }),
        20,
    );
    app.gpu_ms = dl.gpu_ms.get(&uid).copied().unwrap_or(0);
}

fn why(a: &AppRep, t: &Totals) -> Vec<String> {
    let mut w = Vec::new();
    let fmt_b = |b: u64| -> String {
        if b > 1 << 20 {
            format!("{:.1} МБ", b as f64 / 1048576.0)
        } else {
            format!("{} КБ", b / 1024)
        }
    };
    // крупные статьи расхода — по убыванию
    let mut big: Vec<(f64, String)> = Vec::new();
    if a.cpu_mah > 0.05 {
        if let Some(p) = a.processes.first() {
            let thr = p.threads.iter().find(|t| !t.name.starts_with('<') || t.name == "<main>").map(|t| {
                let e = t.explain.map(|e| format!(" — {}", lower_first(e.trim_end_matches('.')))).unwrap_or_default();
                format!(", больше всего в потоке «{}»{}", t.name, e)
            });
            big.push((
                a.cpu_mah,
                format!("Процессор: {:.1} мА·ч за {} CPU-времени; процесс «{}»{}.", a.cpu_mah, fmt_dur(a.cpu_ms), p.name, thr.unwrap_or_default()),
            ));
        }
    } else if let Some(c) = a.components.get("cpu").filter(|c| **c > 0.05) {
        big.push((*c, format!("Процессор (модель Android): {c:.1} мА·ч. Ядро ещё не записало, какие процессы и потоки.")));
    }
    for (k, v) in &a.components {
        if k == "cpu" || *v < 0.1 {
            continue;
        }
        let ms = a.component_ms.get(k).map(|m| format!(" ({})", fmt_dur(*m))).unwrap_or_default();
        let s = match k.as_str() {
            "screen" => format!("Экран, пока приложение было на первом плане: {v:.1} мА·ч{ms}."),
            "gnss" => format!("GPS: {v:.1} мА·ч{ms}."),
            "mobile_radio" => format!("Мобильная связь (радио активно из-за трафика приложения): {v:.1} мА·ч."),
            "wifi" => format!("Wi-Fi трафик и сканирования: {v:.1} мА·ч."),
            "wakelock" => format!("Не давал уснуть (wakelock): {v:.1} мА·ч{ms}."),
            other => format!("{}: {v:.1} мА·ч{ms}.", explain::component(other)),
        };
        big.push((*v, s));
    }
    big.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
    w.extend(big.into_iter().map(|(_, s)| s));
    if a.cpu_mah_screen_off > 0.3 * a.cpu_mah && a.cpu_mah_screen_off > 0.05 {
        w.push(format!("Работал при выключенном экране: {:.1} мА·ч ({:.0}% своего CPU).", a.cpu_mah_screen_off, 100.0 * a.cpu_mah_screen_off / a.cpu_mah));
    }
    let starts: u32 = a.processes.iter().map(|p| p.starts).sum();
    if starts >= 10 {
        w.push(format!("Процессы запускались {starts} раз — частые перезапуски (будится пушем/будильником или падает)."));
    }
    let bg: f64 = ["bg", "fgs", "cached"].iter().filter_map(|k| a.states.get(*k)).sum();
    if let Some(fg) = a.states.get("fg") {
        if bg > *fg && bg > 0.5 {
            w.push(format!("Большая часть расхода в фоне: {:.1} мА·ч против {:.1} мА·ч на экране.", bg, fg));
        }
    }
    if let Some(wl) = a.wakelocks.first() {
        if wl.ms > 60_000 || wl.count > 50 {
            w.push(format!("Wakelock «{}»: {} раз, всего {}.", wl.name, wl.count, fmt_dur(wl.ms)));
        }
    }
    let alarms: u64 = a.alarms.iter().map(|x| x.count).sum();
    if alarms >= 10 {
        let top = a.alarms.first().map(|x| format!(", чаще всего «{}»", x.name)).unwrap_or_default();
        w.push(format!("Будил телефон будильниками {alarms} раз{top}."));
    }
    let jobs: u64 = a.jobs.iter().map(|x| x.count).sum();
    if jobs >= 10 {
        let jms: u64 = a.jobs.iter().map(|x| x.ms).sum();
        w.push(format!("Фоновые задания: {jobs} запусков, {} работы.", fmt_dur(jms)));
    }
    let syncs: u64 = a.syncs.iter().map(|x| x.count).sum();
    if syncs >= 5 {
        w.push(format!("Синхронизация аккаунтов: {syncs} раз."));
    }
    if let Some(n) = &a.net {
        let total = n.mobile_rx + n.mobile_tx + n.wifi_rx + n.wifi_tx;
        let bgt = n.mobile_bg_rx + n.mobile_bg_tx + n.wifi_bg_rx + n.wifi_bg_tx;
        if total > 5 << 20 {
            w.push(format!(
                "Сеть: {} (мобильная {}, Wi-Fi {}), в фоне {}.",
                fmt_b(total),
                fmt_b(n.mobile_rx + n.mobile_tx),
                fmt_b(n.wifi_rx + n.wifi_tx),
                fmt_b(bgt)
            ));
        }
        if n.mobile_wakeups + n.wifi_wakeups > 20 {
            w.push(format!("Входящие пакеты будили телефон {} раз.", n.mobile_wakeups + n.wifi_wakeups));
        }
    }
    if a.gpu_ms > 60_000 {
        w.push(format!("GPU занят {}.", fmt_dur(a.gpu_ms)));
    }
    if a.system_mah >= 0.2 || a.system_ms >= 10_000.0 {
        let top = a
            .system_calls
            .first()
            .map(|c| {
                let e = c.explain.map(|e| format!(" — {}", lower_first(e.trim_end_matches('.')))).unwrap_or_default();
                format!(", чаще всего «{}» ({} вызовов){}", c.call, c.calls, e)
            })
            .unwrap_or_default();
        w.push(format!(
            "Нагружал систему: system_server работал на него {} CPU (≈{:.1} мА·ч, считаются в «Android OS»){}.",
            fmt_dur(a.system_ms as u64),
            a.system_mah,
            top
        ));
    }
    if a.crashes + a.anrs > 0 {
        w.push(format!("Падения: {}, зависания (ANR): {}.", a.crashes, a.anrs));
    }
    if let Some(n) = a.native_wakelocks.first() {
        if n.ms > 30_000 {
            w.push(format!("Нативный wakelock «{}»: {}.", n.name, fmt_dur(n.ms)));
        }
    }
    let _ = t;
    w
}

#[derive(Default)]
struct PtAcc {
    p: TimePoint,
    meas: f64,
    model: f64,
    cpu: f64,
    span_s: f64,
    awake_s: f64,
    susp_s: f64,
    by_uid: HashMap<u32, f64>,
    /// приложение в фокусе: мА·с и по процессам
    app_e: f64,
    app_procs: HashMap<String, f64>,
}

/// Лента: соседние корзины одного состояния (экран/зарядка) склеиваются до ~max_points точек.
/// В каждой точке — замер, модель и топ-3 потребителя (по приложению-хозяину);
/// с `focus` — ещё CPU этого приложения и его самый затратный процесс.
fn timeline(
    src: &[(Bucket, f64)],
    max_points: usize,
    owner: &dyn Fn(u32, &str, Option<u32>) -> u32,
    label: &dyn Fn(u32) -> String,
    focus: Option<u32>,
) -> Vec<TimePoint> {
    let mut pts: Vec<TimePoint> = Vec::new();
    let total_span: i64 = src.iter().map(|(b, _)| b.t1 - b.t0).sum();
    let target = (total_span / max_points as i64).max(60_000);
    let flush = |a: PtAcc, pts: &mut Vec<TimePoint>| {
        let mut p = a.p;
        let s = a.span_s.max(0.001);
        p.meas_ma = a.meas / s;
        p.model_ma = a.model / s;
        p.cpu_ma = a.cpu / s;
        p.awake_pct = 100.0 * a.awake_s / s;
        p.suspend_pct = 100.0 * a.susp_s / s;
        let mut v: Vec<(u32, f64)> = a.by_uid.into_iter().collect();
        v.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
        p.tops = v.iter().take(3).map(|(u, e)| TopRep { uid: *u, label: label(*u), ma: e / s }).collect();
        p.top = p.tops.first().map(|t| t.label.clone()).unwrap_or_default();
        if focus.is_some() {
            p.app_ma = Some(a.app_e / s);
            p.app_top = a
                .app_procs
                .iter()
                .filter(|(_, e)| **e > 0.0)
                .max_by(|x, y| x.1.partial_cmp(y.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(n, _)| n.clone());
        }
        pts.push(p);
    };
    let mut acc: Option<PtAcc> = None;
    for (b, ov) in src {
        let span_s = b.span as f64 / 1000.0 * ov;
        let meas = if b.cc_ok && b.cc_uah.abs() >= 3000 { b.cc_uah as f64 * 3.6 } else { b.cur_mas } * ov;
        let same = acc
            .as_ref()
            .map(|a| a.p.screen == b.scr && a.p.plugged == b.plug && (b.t1 - a.p.t0) <= target)
            .unwrap_or(false);
        if !same {
            if let Some(a) = acc.take() {
                flush(a, &mut pts);
            }
            acc = Some(PtAcc {
                p: TimePoint { t0: b.t0, t1: b.t1, screen: b.scr, plugged: b.plug, level: b.lvl1, ..Default::default() },
                ..Default::default()
            });
        }
        let a = acc.as_mut().unwrap();
        a.p.t1 = b.t1;
        a.p.level = b.lvl1;
        a.meas += meas;
        a.model += (b.cpu_mas + b.base_mas + b.scr_mas) * ov;
        a.cpu += b.cpu_mas * ov;
        a.span_s += span_s;
        a.awake_s += b.awake as f64 / 1000.0 * ov;
        a.susp_s += b.susp as f64 / 1000.0 * ov;
        for p in &b.procs {
            let o = owner(p.uid, &p.n, p.h);
            *a.by_uid.entry(o).or_default() += p.e * ov;
            if Some(o) == focus {
                a.app_e += p.e * ov;
                *a.app_procs.entry(p.n.clone()).or_default() += p.e * ov;
            }
        }
    }
    if let Some(a) = acc.take() {
        flush(a, &mut pts);
    }
    pts
}

/// «2026-11-22 17:44:37» (местное время телефона) → мс.
fn parse_local_time(s: &str) -> Option<i64> {
    let (d, t) = s.trim().split_once(' ')?;
    let mut dp = d.split('-').map(|x| x.parse::<i32>().ok());
    let mut tp = t.split(':').map(|x| x.parse::<i32>().ok());
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = dp.next()?? - 1900;
    tm.tm_mon = dp.next()?? - 1;
    tm.tm_mday = dp.next()??;
    tm.tm_hour = tp.next()??;
    tm.tm_min = tp.next()??;
    tm.tm_sec = tp.next()??;
    tm.tm_isdst = -1;
    let secs = unsafe { libc::mktime(&mut tm) };
    if secs < 0 { None } else { Some(secs as i64 * 1000) }
}

/// Окно для «за кого работает system»: разность двух снимков, где есть эта статистика.
fn system_work(snaps: &[&SysSnap], from: i64, to: i64, mas_per_ms: f64, pkgs: &BTreeMap<u32, Vec<String>>) -> Option<SysWorkRep> {
    let have: Vec<&SysSnap> = snaps.iter().copied().filter(|s| !s.work.binder_start.is_empty()).collect();
    let b = have.iter().rev().find(|s| s.t <= to + 120_000).copied().or_else(|| have.last().copied())?;
    let a = have.iter().rev().find(|s| s.t <= from + 120_000 && s.t < b.t).copied();
    let (w, wfrom, exact, note) = match a {
        Some(a) => {
            let exact = (a.t - from).abs() < 300_000;
            let note = if exact { String::new() } else { format!("Разбор system — с {} (ближайший снимок).", fmt_time(a.t)) };
            (crate::sys::binder::delta(&a.work, &b.work), a.t, exact, note)
        }
        None => {
            // статистика Android копится с «Start time»: если это внутри окна — берём как есть
            let st = parse_local_time(&b.work.binder_start).unwrap_or(0);
            if st >= from - 600_000 && st > 0 {
                (b.work.clone(), st, true, String::new())
            } else if let Some(first) = have.iter().find(|s| s.t < b.t) {
                let note = format!("Разбор system — с {} (раньше снимков с ним нет).", fmt_time(first.t));
                (crate::sys::binder::delta(&first.work, &b.work), first.t, false, note)
            } else {
                return None;
            }
        }
    };
    let queue_ms: f64 = w.looper_by_uid.values().map(|x| x.cpu_ms).sum();
    let own_ms = w.looper_by_uid.get("-1").map(|x| x.cpu_ms).unwrap_or(0.0);
    let total = (w.binder_total_ms + queue_ms).max(0.001);
    let mut callers: HashMap<u32, SysCallerRep> = HashMap::new();
    for (k, c) in &w.callers {
        let Ok(uid) = k.parse::<u32>() else { continue };
        let e = callers.entry(uid).or_default();
        e.uid = uid;
        e.binder_ms += c.cpu_ms;
        e.calls += c.calls;
    }
    for (k, q) in &w.looper_by_uid {
        let Ok(uid) = k.parse::<u32>() else { continue };
        let e = callers.entry(uid).or_default();
        e.uid = uid;
        e.queue_ms += q.cpu_ms;
    }
    let mut calls_by: HashMap<u32, Vec<SysCallRep>> = HashMap::new();
    for (k, c) in &w.calls {
        let Some((u, call)) = k.split_once('|') else { continue };
        let Ok(uid) = u.parse::<u32>() else { continue };
        calls_by.entry(uid).or_default().push(SysCallRep { explain: explain::binder_call(call), call: call.to_string(), cpu_ms: c.cpu_ms, calls: c.count });
    }
    let mut callers: Vec<SysCallerRep> = callers
        .into_values()
        .map(|mut c| {
            c.cpu_ms = c.binder_ms + c.queue_ms;
            c.mah = mas_to_mah(c.cpu_ms * mas_per_ms);
            c.share = c.cpu_ms / total;
            let (label, packages) = label_for(c.uid, pkgs);
            c.label = label;
            c.packages = packages;
            c.kind = explain::uid_kind(c.uid);
            let mut tc = calls_by.remove(&c.uid).unwrap_or_default();
            tc.sort_by(|x, y| y.cpu_ms.partial_cmp(&x.cpu_ms).unwrap_or(std::cmp::Ordering::Equal));
            tc.truncate(8);
            c.top_calls = tc;
            c
        })
        .filter(|c| c.cpu_ms >= 50.0)
        .collect();
    callers.sort_by(|x, y| y.cpu_ms.partial_cmp(&x.cpu_ms).unwrap_or(std::cmp::Ordering::Equal));
    callers.truncate(40);
    // очереди: по потокам
    let mut qs: HashMap<String, SysQueueRep> = HashMap::new();
    for (k, x) in &w.looper {
        let mut it = k.splitn(3, '|');
        let (th, h, m) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
        let q = qs.entry(th.to_string()).or_insert_with(|| SysQueueRep { thread: th.to_string(), explain: explain::system_queue(th), ..Default::default() });
        q.cpu_ms += x.cpu_ms;
        q.top.push(SysMsgRep { handler: h.to_string(), message: m.to_string(), cpu_ms: x.cpu_ms, count: x.count });
    }
    let mut queues: Vec<SysQueueRep> = qs
        .into_values()
        .map(|mut q| {
            q.share = q.cpu_ms / queue_ms.max(0.001);
            q.top.sort_by(|x, y| y.cpu_ms.partial_cmp(&x.cpu_ms).unwrap_or(std::cmp::Ordering::Equal));
            q.top.truncate(5);
            q
        })
        .collect();
    queues.sort_by(|x, y| y.cpu_ms.partial_cmp(&x.cpu_ms).unwrap_or(std::cmp::Ordering::Equal));
    queues.truncate(20);
    Some(SysWorkRep {
        from: wfrom,
        to: b.t,
        exact,
        note,
        binder_ms: w.binder_total_ms,
        queue_ms,
        own_ms,
        mas_per_cpu_ms: mas_per_ms,
        mah_total: mas_to_mah(total * mas_per_ms),
        callers,
        queues,
        a11y: b.work.a11y.clone(),
    })
}

/// База для долей приложений: факт по счётчику, но не меньше суммы приложений. Факт считается только
/// без зарядки, а расход приложений — за весь период; на зарядке иначе выходят доли в тысячи процентов.
pub fn share_base(r: &Report) -> f64 {
    r.totals.measured_mah.max(r.apps.iter().map(|a| a.mah).sum::<f64>())
}

/// Выводы: что главное за интервал, с оценкой «хорошо/плохо».
fn insights(r: &Report) -> Vec<Insight> {
    let mut out = Vec::new();
    let t = &r.totals;
    let ins = |level, section, title: String, text: String, uid: Option<u32>| Insight { level, title, text, uid, section };
    let hours = |ms: u64| ms as f64 / 3_600_000.0;

    // сон
    if t.screen_off_ms >= 10 * 60_000 {
        let ws = r.kernel.wakeup_sources.iter().max_by_key(|w| w.prevent_suspend_ms.max(w.held_ms));
        let abort = r
            .kernel
            .wakeup_reasons
            .iter()
            .filter(|w| w.name.starts_with("Abort"))
            .max_by_key(|w| w.count)
            .or_else(|| r.kernel.suspend_abort_reasons.first());
        let mut text = format!(
            "Без зарядки с выключенным экраном телефон {} из {} провёл в глубоком сне ({:.0}%). ",
            fmt_dur(t.screen_off_suspend_ms),
            fmt_dur(t.screen_off_ms),
            t.deep_sleep_pct
        );
        if let Some(w) = ws {
            text += &format!("Дольше всех не давал спать «{}» ({}, будил {} раз). ", w.name, fmt_dur(w.held_ms.max(w.prevent_suspend_ms)), w.wakeups);
        }
        if let Some(a) = abort {
            text += &format!("Засыпание чаще всего срывалось так: «{}» ({}×).", a.name.trim_start_matches("Abort: "), a.count);
        }
        let (level, title) = if t.deep_sleep_pct >= 90.0 {
            ("good", "Телефон хорошо спит")
        } else if t.deep_sleep_pct >= 70.0 {
            ("warn", "Сон с помехами")
        } else {
            ("bad", "Телефон почти не спит")
        };
        out.push(ins(level, "sleep", title.into(), text, None));
    }
    // разряд при выключенном экране
    if t.screen_off_ms >= 30 * 60_000 && t.pct_per_hour_screen_off > 0.0 {
        let p = t.pct_per_hour_screen_off;
        let (level, title) = if p < 1.0 {
            ("good", "Скромный расход в покое")
        } else if p <= 3.0 {
            ("warn", "Заметный расход в покое")
        } else {
            ("bad", "Батарея тает в покое")
        };
        out.push(ins(
            level,
            "battery",
            title.into(),
            format!(
                "С выключенным экраном уходило {:.1}% в час (~{:.0} мА). Норма для современного телефона — меньше 1%/ч (10–30 мА).",
                p, t.measured_screen_off_ma
            ),
            None,
        ));
    }
    // главный потребитель
    if let Some(a) = r.apps.first() {
        if a.mah > 0.5 {
            let base = share_base(r);
            let share = if base > 1.0 { format!(" — {:.0}% израсходованного", 100.0 * a.mah / base) } else { String::new() };
            out.push(ins(
                "info",
                "apps",
                format!("Больше всех: {}", a.label),
                format!("{:.1} мА·ч{}. {}", a.mah, share, a.why.first().cloned().unwrap_or_default()),
                Some(a.uid),
            ));
        }
    }
    // фон при выключенном экране
    if let Some(a) = r.apps.iter().filter(|a| a.kind == "app").max_by(|x, y| x.cpu_mah_screen_off.partial_cmp(&y.cpu_mah_screen_off).unwrap_or(std::cmp::Ordering::Equal)) {
        let off_total: f64 = r.apps.iter().map(|a| a.cpu_mah_screen_off).sum();
        if a.cpu_mah_screen_off > 1.0 && a.cpu_mah_screen_off > 0.1 * off_total {
            out.push(ins(
                "warn",
                "apps",
                format!("Работает в фоне: {}", a.label),
                format!(
                    "При выключенном экране этому приложению досталось {:.1} мА·ч процессора ({:.0}% всего фонового CPU). Если оно не должно работать в фоне — ограничьте ему фоновую активность в настройках батареи.",
                    a.cpu_mah_screen_off,
                    100.0 * a.cpu_mah_screen_off / off_total.max(0.001)
                ),
                Some(a.uid),
            ));
        }
    }
    // частые пробуждения
    let awake_h = hours(t.span_ms).max(0.01);
    if let Some(w) = r.kernel.wakeup_sources.iter().max_by_key(|w| w.wakeups) {
        let per_h = w.wakeups as f64 / awake_h;
        if w.wakeups >= 50 && per_h > 120.0 {
            out.push(ins(
                if per_h > 600.0 { "bad" } else { "warn" },
                "kernel",
                format!("Частые пробуждения: {}", w.name),
                format!(
                    "Будил процессор {} раз (~{:.0} в час). {}",
                    w.wakeups,
                    per_h,
                    w.explain.unwrap_or("Каждое пробуждение — десятки–сотни миллисекунд работы процессора и радио.")
                ),
                None,
            ));
        }
    }
    // служба засыпания перегружена
    let cpu_total: f64 = r.apps.iter().map(|a| a.cpu_mah).sum();
    if let Some((a, p)) = r.apps.iter().flat_map(|a| a.processes.iter().map(move |p| (a, p))).find(|(_, p)| p.name.starts_with("android.system.suspend")) {
        if p.mah > 0.5 && p.mah > 0.05 * cpu_total {
            out.push(ins(
                "warn",
                "kernel",
                "Служба засыпания перегружена".into(),
                format!(
                    "android.system.suspend-service потратил {:.1} мА·ч ({:.0}% всего CPU): кто-то очень часто берёт и отпускает wakelock или засыпание постоянно срывается. Смотрите «Сон и ядро» → причины срывов и wakelock приложений.",
                    p.mah,
                    100.0 * p.mah / cpu_total.max(0.001)
                ),
                Some(a.uid),
            ));
        }
    }
    // нехватка памяти
    if let Some(k) = r.apps.iter().find(|a| a.kind == "kernel") {
        let swap: f64 = k.processes.iter().flat_map(|p| p.threads.iter()).filter(|t| t.name.starts_with("kswapd") || t.name.starts_with("kcompactd")).map(|t| t.mah).sum();
        if swap > 0.5 && swap > 0.03 * cpu_total {
            out.push(ins(
                "warn",
                "cpu",
                "Не хватает оперативной памяти".into(),
                format!("Ядро потратило {swap:.1} мА·ч на выгрузку и уплотнение памяти (kswapd/kcompactd). Помогает закрыть тяжёлые приложения или ограничить фоновые."),
                Some(k.uid),
            ));
        }
    }
    // WebView у приложений
    for a in r.apps.iter().take(15) {
        let wv: f64 = a.processes.iter().filter(|p| p.isolated && p.host_uid.is_some()).map(|p| p.mah).sum();
        if wv > 0.5 && wv > 0.3 * a.cpu_mah {
            out.push(ins(
                "info",
                "apps",
                format!("{} тратит через WebView", a.label),
                format!("{wv:.1} мА·ч ушло на встроенные веб-страницы приложения (реклама, мини-приложения, встроенный браузер) — их рендеры работают в отдельных песочницах."),
                Some(a.uid),
            ));
        }
    }
    // частые перезапуски
    if let Some(a) = r.apps.iter().filter(|a| a.kind == "app").max_by_key(|a| a.processes.iter().map(|p| p.starts).sum::<u32>()) {
        let st: u32 = a.processes.iter().map(|p| p.starts).sum();
        if st >= 30 {
            out.push(ins(
                "warn",
                "apps",
                format!("Постоянно перезапускается: {}", a.label),
                format!("Процессы приложения запускались {st} раз: его будят пуши/будильники или система убивает и оно поднимается снова. Каждый запуск — секунды работы процессора."),
                Some(a.uid),
            ));
        }
    }
    // прочее
    if t.measured_mah > 5.0 && t.other_mah > 0.4 * t.measured_mah {
        let g = r.sys.as_ref().and_then(|s| s.global.iter().filter(|(k, _)| k.as_str() != "cpu" && k.as_str() != "screen").max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(k, v)| (k.clone(), *v)));
        out.push(ins(
            "info",
            "battery",
            "Много уходит не на процессор и экран".into(),
            format!(
                "{:.0}% расхода не объясняется CPU и экраном{}",
                100.0 * t.other_mah / t.measured_mah,
                g.map(|(k, v)| format!(" — по модели Android больше всего {} ({:.1} мА·ч).", explain::component(&k), v)).unwrap_or_else(|| ": модем, Wi-Fi, GPS, камера, динамик, GPU.".into())
            ),
            None,
        ));
    }
    if t.model_total_mah > t.measured_mah * 1.3 && t.measured_mah > 3.0 {
        out.push(ins(
            "info",
            "battery",
            "Модель завышена".into(),
            "Коэффициенты производителя дают больше, чем ушло из батареи. Абсолютные мА·ч процессов завышены, но доли между ними верны.".into(),
            None,
        ));
    }
    if let Some(sw) = &r.system_work {
        if let Some(c) = sw.callers.iter().find(|c| c.uid % 100_000 != 1000 && c.uid != 0) {
            if (c.share >= 0.10 && c.mah >= 1.0) || c.mah >= 10.0 {
                let call = c
                    .top_calls
                    .first()
                    .map(|x| format!(" Больше всего — «{}» ({} вызовов){}.", x.call, x.calls, x.explain.map(|e| format!(": {}", lower_first(e.trim_end_matches('.')))).unwrap_or_default()))
                    .unwrap_or_default();
                out.push(ins(
                    "warn",
                    "apps",
                    format!("Нагружает систему: {}", c.label),
                    format!(
                        "{:.0}% работы system_server — по заказу этого приложения: {} CPU, ≈{:.1} мА·ч (они видны в «Android OS», а не у приложения).{}",
                        100.0 * c.share,
                        fmt_dur(c.cpu_ms as u64),
                        c.mah,
                        call
                    ),
                    Some(c.uid),
                ));
            }
        }
        let a11y_ms: f64 = sw.callers.iter().flat_map(|c| c.top_calls.iter()).filter(|x| x.call.ends_with("#sendAccessibilityEvent")).map(|x| x.cpu_ms).sum();
        if !sw.a11y.is_empty() && a11y_ms >= 20_000.0 {
            out.push(ins(
                "info",
                "apps",
                "Спецвозможности нагружают систему".into(),
                format!(
                    "Включены службы спецвозможностей ({}), поэтому все приложения шлют системе события о каждом изменении экрана: {} CPU system_server. Если служба не нужна — выключите её в настройках спецвозможностей.",
                    sw.a11y.iter().map(|x| x.split('/').next().unwrap_or(x)).collect::<Vec<_>>().join(", "),
                    fmt_dur(a11y_ms as u64)
                ),
                None,
            ));
        }
    }
    if t.gpu_busy_pct > 30.0 {
        out.push(ins("info", "cpu", "GPU загружен".into(), format!("GPU был занят {:.0}% времени бодрствования — игры, видео, тяжёлые анимации.", t.gpu_busy_pct), None));
    }
    out
}

// ---------- текст для CLI ----------

pub fn render_text(r: &Report) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let t = &r.totals;
    let _ = writeln!(s, "══ Жор: {} ({} — {}) ══", r.label, fmt_time(r.from), fmt_time(r.to));
    let _ = writeln!(
        s,
        "Интервал {}: экран вкл {}, выкл {} (глубокий сон {:.0}% времени выкл.), на зарядке {}",
        fmt_dur(t.span_ms),
        fmt_dur(t.screen_on_ms),
        fmt_dur(t.screen_off_ms),
        t.deep_sleep_pct,
        fmt_dur(t.plugged_ms)
    );
    let _ = writeln!(
        s,
        "Израсходовано: {:.0} мА·ч, в среднем {:.0} мА ({}); экран вкл ~{:.0} мА, выкл ~{:.0} мА; {:.1}%/ч, в выкл. {:.1}%/ч",
        t.measured_mah, t.measured_avg_ma, t.measured_source, t.measured_screen_on_ma, t.measured_screen_off_ma, t.pct_per_hour, t.pct_per_hour_screen_off
    );
    let _ = writeln!(
        s,
        "Модель: CPU {:.1} + CPU-простой {:.1} + экран {:.1} = {:.1} мА·ч; прочее (радио/GPS/Wi-Fi/периферия) ≈ {:.1} мА·ч; засыпаний {} (неудачных {}), GPU занят {:.0}%",
        t.model_cpu_mah, t.model_cpu_idle_mah, t.model_screen_mah, t.model_total_mah, t.other_mah, t.suspends, t.suspend_fails, t.gpu_busy_pct
    );
    for n in &r.notes {
        let _ = writeln!(s, "  ⚠ {n}");
    }
    if !r.insights.is_empty() {
        let _ = writeln!(s, "\n── Главное ──");
        for i in &r.insights {
            let mark = match i.level {
                "bad" => "✖",
                "warn" => "⚠",
                "good" => "✔",
                _ => "•",
            };
            let _ = writeln!(s, "{mark} {}: {}", i.title, i.text);
        }
    }
    let _ = writeln!(s, "\n── Кто жрёт ──");
    for (i, a) in r.apps.iter().take(15).enumerate() {
        let comps: Vec<String> = a
            .components
            .iter()
            .filter(|(k, v)| k.as_str() != "cpu" && **v >= 0.05)
            .map(|(k, v)| format!("{} {:.1}", explain::component(k), v))
            .collect();
        let _ = writeln!(
            s,
            "{:>2}. {:<42} {:>7.1} мА·ч  CPU {:>6.1} ({}){}",
            i + 1,
            trunc(&a.label, 42),
            a.mah,
            a.cpu_mah,
            fmt_dur(a.cpu_ms),
            if comps.is_empty() { String::new() } else { format!("  [{}]", comps.join(", ")) }
        );
        for p in a.processes.iter().take(3) {
            if p.mah < 0.01 {
                continue;
            }
            let thr: Vec<String> = p.threads.iter().take(4).map(|t| format!("{} {:.2}", t.name, t.mah)).collect();
            let _ = writeln!(s, "      └ {} {:.2} мА·ч{}: {}", trunc(&p.name, 50), p.mah, if p.starts > 0 { format!(", запусков {}", p.starts) } else { String::new() }, thr.join(" · "));
        }
        for w in a.why.iter().filter(|w| !w.starts_with("Процессор:")).take(4) {
            let _ = writeln!(s, "      • {w}");
        }
    }
    if let Some(sw) = &r.system_work {
        let _ = writeln!(
            s,
            "\n── За кого работает system ({} — {}): вызовы {} + очереди {} CPU, ≈{:.1} мА·ч ──",
            fmt_time(sw.from),
            fmt_time(sw.to),
            fmt_dur(sw.binder_ms as u64),
            fmt_dur(sw.queue_ms as u64),
            sw.mah_total
        );
        for c in sw.callers.iter().take(8) {
            let call = c.top_calls.first().map(|x| format!(" · {} ×{}", x.call, x.calls)).unwrap_or_default();
            let who = if c.uid == 0 && c.label.starts_with("root") { "root" } else { c.label.as_str() };
            let _ = writeln!(s, "  {:>4.0}%  {:>10}  ≈{:>5.1} мА·ч  {}{}", 100.0 * c.share, fmt_dur(c.cpu_ms as u64), c.mah, trunc(who, 40), call);
        }
        if sw.own_ms > 0.0 {
            let _ = writeln!(s, "  сама система (очереди без заказчика): {}", fmt_dur(sw.own_ms as u64));
        }
        for q in sw.queues.iter().take(5) {
            let _ = writeln!(s, "  очередь {:<28} {:>10}  {}", trunc(&q.thread, 28), fmt_dur(q.cpu_ms as u64), q.explain.unwrap_or(""));
        }
        if !sw.a11y.is_empty() {
            let _ = writeln!(s, "  спецвозможности включены: {}", sw.a11y.join(", "));
        }
    }
    let _ = writeln!(s, "\n── Ядро: источники пробуждения ──");
    for w in r.kernel.wakeup_sources.iter().take(10) {
        let _ = writeln!(
            s,
            "  {:<40} будил {:>4}  активаций {:>6}  держал {:>10}  {}",
            trunc(&w.name, 40),
            w.wakeups,
            w.activations,
            fmt_dur(w.held_ms),
            w.explain.unwrap_or("")
        );
    }
    if !r.kernel.wakeup_reasons.is_empty() {
        let _ = writeln!(s, "\n── Причины пробуждений (batterystats) ──");
        for w in r.kernel.wakeup_reasons.iter().take(10) {
            let _ = writeln!(s, "  {:>5}×  {}  {}", w.count, trunc(&w.name, 60), w.explain.clone().unwrap_or_default());
        }
    } else if !r.kernel.resume_reasons.is_empty() {
        let _ = writeln!(s, "\n── Причины пробуждений (последние, ядро) ──");
        for w in r.kernel.resume_reasons.iter().take(10) {
            let _ = writeln!(s, "  {:>5}×  {}  {}", w.count, w.name, w.explain.clone().unwrap_or_default());
        }
    }
    if !r.kernel.native_wakelocks.is_empty() {
        let _ = writeln!(s, "\n── Нативные wakelock ──");
        for w in r.kernel.native_wakelocks.iter().take(8) {
            let _ = writeln!(s, "  {:<60} {:>6}×  {}", trunc(&w.name, 60), w.count, fmt_dur(w.ms));
        }
    }
    let _ = writeln!(s, "\n── CPU по кластерам ──");
    for c in &r.clusters {
        let top: Vec<String> = {
            let mut v = c.residency.clone();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.iter().take(3).filter(|x| x.1 > 0).map(|(f, ms)| format!("{} МГц {}", f / 1000, fmt_dur(*ms))).collect()
        };
        let _ = writeln!(s, "  {} (CPU {:?}): {} CPU-времени, {:.1} мА·ч; чаще: {}", c.policy, c.cpus, fmt_dur(c.cpu_ms), c.mah, top.join(", "));
    }
    if let Some(h) = &r.history {
        let _ = writeln!(s, "\n── История Android (с {}) ──", fmt_time(h.from));
        for n in &h.notes {
            let _ = writeln!(s, "  ⚠ {n}");
        }
        for l in &h.lanes {
            let _ = writeln!(s, "  {:<32} {:>6}×  {}", l.label, l.count, fmt_dur(l.ms));
        }
        if !h.items.is_empty() {
            let _ = writeln!(s, "  Чаще и дольше всего:");
        }
        for it in h.items.iter().take(15) {
            let who = it.app.as_deref().map(|a| format!("{} — ", trunc(a, 28))).unwrap_or_default();
            let _ = writeln!(s, "  {:>6}× {:>10}  {}{}: {}", it.count, fmt_dur(it.ms), who, it.label, trunc(&it.name, 70));
        }
        if !h.events.is_empty() {
            let _ = writeln!(s, "  События ({} из {}), последние:", h.events.len(), h.events_total);
            for e in h.events.iter().rev().take(15) {
                let k = crate::history::Kind::from_name(e.kind).map(|k| k.label()).unwrap_or(e.kind);
                let who = e.app.as_deref().map(|a| format!("{} — ", trunc(a, 28))).unwrap_or_default();
                let dur = if e.t1 > e.t0 { format!(" ({})", fmt_dur((e.t1 - e.t0) as u64)) } else { String::new() };
                let _ = writeln!(s, "  {} {}{}: {}{}{}", fmt_time_s(e.t0), who, k, trunc(&e.name, 60), dur, if e.open { ", идёт" } else { "" });
            }
        }
    }
    s
}

/// «23.11 04:12:05» — местное время с секундами.
fn fmt_time_s(t: i64) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt: libc::time_t = (t / 1000) as libc::time_t;
    unsafe {
        libc::localtime_r(&tt, &mut tm);
    }
    format!("{:02}.{:02} {:02}:{:02}:{:02}", tm.tm_mday, tm.tm_mon + 1, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut o: String = s.chars().take(n - 1).collect();
        o.push('…');
        o
    }
}
