//! Разбор агрегатов `dumpsys batterystats -c` (формат checkin, версия 9).
//! Историю (строки «9,h,…») пропускаем — берём только сводные строки «i» и «l».
//! Здесь «почему»: wakelock по тегам, будильники, задания, синхронизации, датчики,
//! сеть, запуски процессов, wakelock ядра и причины пробуждения.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Wl {
    /// Частичный wakelock (держит CPU): число захватов, мс фактически, мс «по вине» (с делением), max мс
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub count: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub blamed_ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub max_ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub bg_ms: u64,
    /// Полный (держит экран)
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub full_ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub full_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimeCount {
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub count: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub bg_ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub bg_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcCk {
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub user_ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub sys_ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub fg_ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub starts: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub anrs: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub crashes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Net {
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub mobile_rx: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub mobile_tx: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub wifi_rx: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub wifi_tx: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub mobile_active_ms: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub mobile_active_count: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub mobile_wakeups: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub wifi_wakeups: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub mobile_bg_rx: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub mobile_bg_tx: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub wifi_bg_rx: u64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub wifi_bg_tx: u64,
}

impl Net {
    pub fn is_empty(&self) -> bool {
        self.mobile_rx + self.mobile_tx + self.wifi_rx + self.wifi_tx + self.mobile_active_ms + self.mobile_wakeups + self.wifi_wakeups == 0
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UidCk {
    #[serde(default, skip_serializing_if = "crate::util::is_empty_coll")]
    pub wakelocks: BTreeMap<String, Wl>,
    #[serde(default, skip_serializing_if = "crate::util::is_empty_coll")]
    pub jobs: BTreeMap<String, TimeCount>,
    #[serde(default, skip_serializing_if = "crate::util::is_empty_coll")]
    pub syncs: BTreeMap<String, TimeCount>,
    /// датчик (handle) → время/число; -10000 = GPS
    #[serde(default, skip_serializing_if = "crate::util::is_empty_coll")]
    pub sensors: BTreeMap<String, TimeCount>,
    /// тег будильника → число пробуждений
    #[serde(default, skip_serializing_if = "crate::util::is_empty_coll")]
    pub alarms: BTreeMap<String, u64>,
    /// пакет → пробуждения будильниками
    #[serde(default, skip_serializing_if = "crate::util::is_empty_coll")]
    pub apk_wakeups: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "crate::util::is_empty_coll")]
    pub procs: BTreeMap<String, ProcCk>,
    pub cpu_user_ms: u64,
    pub cpu_sys_ms: u64,
    #[serde(default, skip_serializing_if = "Net::is_empty")]
    pub net: Net,
    pub wifi_scans: u64,
    pub wifi_scan_ms: u64,
    /// top, fgs, fg, bg, top_sleeping, heavy, cached (мс)
    #[serde(default, skip_serializing_if = "crate::util::is_empty_coll")]
    pub states: Vec<u64>,
    pub fg_activity_ms: u64,
    pub fgs_ms: u64,
    pub vibrator_ms: u64,
    pub partial_wl_ms: u64,
    pub partial_wl_bg_ms: u64,
    pub mah: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckinSnap {
    pub battery_realtime_ms: u64,
    pub start_clock_ms: i64,
    pub screen_off_realtime_ms: u64,
    pub screen_doze_ms: u64,
    pub screen_on_ms: u64,
    pub interactive_ms: u64,
    pub partial_wl_total_ms: u64,
    pub mobile_radio_active_ms: u64,
    pub doze_full_ms: u64,
    pub doze_light_ms: u64,
    pub uid_pkgs: BTreeMap<u32, Vec<String>>,
    pub kernel_wl: BTreeMap<String, TimeCount>,
    pub wakeup_reasons: BTreeMap<String, TimeCount>,
    pub global_mah: BTreeMap<String, f64>,
    pub uids: BTreeMap<u32, UidCk>,
}

/// Разбивка по запятым с учётом кавычек; кавычки снимаются.
fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    for ch in line.chars() {
        match ch {
            '"' => q = !q,
            ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

fn n(v: &[String], i: usize) -> u64 {
    v.get(i).and_then(|x| x.parse::<f64>().ok()).map(|x| x.max(0.0) as u64).unwrap_or(0)
}
fn f(v: &[String], i: usize) -> f64 {
    v.get(i).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0)
}

pub fn parse(text: &str) -> CheckinSnap {
    let mut s = CheckinSnap::default();
    for line in text.lines() {
        // быстрый отсев истории: нужны «9,<uid>,i|l,…»
        let b = line.as_bytes();
        if b.len() < 6 || b[0] != b'9' || b[1] != b',' {
            continue;
        }
        let Some(p2) = line[2..].find(',') else { continue };
        let kind = &line[2 + p2 + 1..];
        if !(kind.starts_with("l,") || kind.starts_with("i,")) {
            continue;
        }
        let v = split_csv(line);
        if v.len() < 4 {
            continue;
        }
        let uid: u32 = match v[1].parse() {
            Ok(u) => u,
            Err(_) => continue,
        };
        let cat = v[3].as_str();
        match cat {
            "uid" => {
                if let (Some(u), Some(p)) = (v.get(4).and_then(|x| x.parse::<u32>().ok()), v.get(5)) {
                    s.uid_pkgs.entry(u).or_default().push(p.clone());
                }
            }
            "bt" => {
                s.battery_realtime_ms = n(&v, 5);
                s.start_clock_ms = f(&v, 9) as i64;
                s.screen_off_realtime_ms = n(&v, 10);
                s.screen_doze_ms = n(&v, 15);
            }
            "m" => {
                s.screen_on_ms = n(&v, 4);
                s.partial_wl_total_ms = n(&v, 7);
                s.mobile_radio_active_ms = n(&v, 8);
                s.interactive_ms = n(&v, 10);
                s.doze_full_ms = n(&v, 13);
                s.doze_light_ms = n(&v, 19);
            }
            "kwl" => {
                s.kernel_wl.insert(v.get(4).cloned().unwrap_or_default(), TimeCount { ms: n(&v, 5), count: n(&v, 6), ..Default::default() });
            }
            "wr" => {
                s.wakeup_reasons.insert(v.get(4).cloned().unwrap_or_default(), TimeCount { ms: n(&v, 5), count: n(&v, 6), ..Default::default() });
            }
            "pwi" => {
                let label = v.get(4).map(|x| x.as_str()).unwrap_or("");
                if label == "uid" {
                    s.uids.entry(uid).or_default().mah = f(&v, 5);
                } else {
                    s.global_mah.insert(label.to_string(), f(&v, 5));
                }
            }
            _ => parse_uid_line(s.uids.entry(uid).or_default(), cat, &v),
        }
    }
    s
}

fn parse_uid_line(u: &mut UidCk, cat: &str, v: &[String]) {
    match cat {
        "wl" => {
            // имя без кавычек может содержать запятые: ищем маркер «f»
            let Some(fi) = (5..v.len()).find(|&i| v[i] == "f") else { return };
            let name = v[4..fi - 1].join(",");
            let mut w = Wl { full_ms: n(v, fi - 1), full_count: n(v, fi + 1), ..Default::default() };
            if let Some(pi) = (fi + 1..v.len()).find(|&i| v[i] == "p") {
                w.blamed_ms = n(v, pi - 1);
                w.count = n(v, pi + 1);
                w.max_ms = n(v, pi + 3);
                w.ms = n(v, pi + 4);
                if w.ms == 0 {
                    w.ms = w.blamed_ms;
                }
            }
            if let Some(bi) = (fi + 1..v.len()).find(|&i| v[i] == "bp") {
                w.bg_ms = n(v, bi - 1);
            }
            u.wakelocks.insert(name, w);
        }
        "awl" => {
            u.partial_wl_ms = n(v, 4);
            u.partial_wl_bg_ms = n(v, 5);
        }
        "jb" | "sy" => {
            let tc = TimeCount { ms: n(v, 5), count: n(v, 6), bg_ms: n(v, 7), bg_count: n(v, 8) };
            let name = v.get(4).cloned().unwrap_or_default();
            if cat == "jb" {
                u.jobs.insert(name, tc);
            } else {
                u.syncs.insert(name, tc);
            }
        }
        "sr" => {
            let actual = n(v, 8);
            let tc = TimeCount { ms: if actual > 0 { actual } else { n(v, 5) }, count: n(v, 6), bg_count: n(v, 7), bg_ms: n(v, 9) };
            u.sensors.insert(v.get(4).cloned().unwrap_or_default(), tc);
        }
        "wua" => {
            if v.len() >= 6 {
                let name = v[4..v.len() - 1].join(",");
                *u.alarms.entry(name).or_default() += n(v, v.len() - 1);
            }
        }
        "apk" => {
            if let Some(pkg) = v.get(5) {
                u.apk_wakeups.insert(pkg.clone(), n(v, 4));
            }
        }
        "pr" => {
            let p = ProcCk { user_ms: n(v, 5), sys_ms: n(v, 6), fg_ms: n(v, 7), starts: n(v, 8), anrs: n(v, 9), crashes: n(v, 10) };
            u.procs.insert(v.get(4).cloned().unwrap_or_default(), p);
        }
        "cpu" => {
            u.cpu_user_ms = n(v, 4);
            u.cpu_sys_ms = n(v, 5);
        }
        "nt" => {
            u.net = Net {
                mobile_rx: n(v, 4),
                mobile_tx: n(v, 5),
                wifi_rx: n(v, 6),
                wifi_tx: n(v, 7),
                mobile_active_ms: n(v, 12) / 1000,
                mobile_active_count: n(v, 13),
                mobile_wakeups: n(v, 16),
                wifi_wakeups: n(v, 17),
                mobile_bg_rx: n(v, 18),
                mobile_bg_tx: n(v, 19),
                wifi_bg_rx: n(v, 20),
                wifi_bg_tx: n(v, 21),
            };
        }
        "wfl" => {
            u.wifi_scans = n(v, 7);
            u.wifi_scan_ms = n(v, 12);
        }
        "st" => {
            u.states = (4..11).map(|i| n(v, i)).collect();
        }
        "fg" => u.fg_activity_ms = n(v, 4),
        "fgs" => u.fgs_ms = n(v, 4),
        "vib" => u.vibrator_ms = n(v, 4),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const T: &str = r#"9,0,i,vers,36,216,CP2A.260605.016,CP2A.260605.016
9,hsp,0,1000,"sensor:0x33"
9,0,i,uid,10330,com.example.mail
9,0,l,bt,0,3242793,3170163,3242793,3170163,1795384601413,430673,358042,4673,4737000,4737000,10106
9,0,l,kwl,"qcom_rx_wakelock",28193,27,-1,-1
9,0,l,wr,"Abort: Pending Wakeup Sources: qcom_rx_wakelock",28193,27
9,1000,l,wl,AODUpdatePositionController:cpu,0,f,0,0,0,0,646,p,4,0,827,2937,2937,bp,4,0,827,2937,0,w,0,0,0,0
9,1000,l,wl,a,b,0,f,0,0,0,0,10,p,1,0,10,10,10,bp,1,0,10,10,0,w,0,0,0,0
9,10330,l,jb,"com.example.mail/.widget.WidgetUpdater$WidgetUpdateService",51,3,51,3
9,1000,l,wua,*walarm*:MiuiFullAodManager,2
9,0,l,pr,"kworker/u19:2-hal_register_write_wq",0,18850,0,0,0,0
9,1000,l,sr,11,1450,2,2,4349,4349
9,10429,l,pwi,uid,105,0,52.6,188
9,0,l,pwi,cpu,369,0,0,0
"#;
    #[test]
    fn checkin() {
        let s = parse(T);
        assert_eq!(s.battery_realtime_ms, 3_242_793);
        assert_eq!(s.start_clock_ms, 1_795_384_601_413);
        assert_eq!(s.uid_pkgs[&10330], vec!["com.example.mail"]);
        assert_eq!(s.kernel_wl["qcom_rx_wakelock"].count, 27);
        let w = &s.uids[&1000].wakelocks["AODUpdatePositionController:cpu"];
        assert_eq!((w.count, w.ms, w.blamed_ms, w.max_ms, w.bg_ms), (4, 2937, 646, 827, 2937));
        assert!(s.uids[&1000].wakelocks.contains_key("a,b"));
        assert_eq!(s.uids[&10330].jobs.values().next().unwrap().count, 3);
        assert_eq!(s.uids[&1000].alarms["*walarm*:MiuiFullAodManager"], 2);
        assert_eq!(s.uids[&0].procs["kworker/u19:2-hal_register_write_wq"].sys_ms, 18850);
        assert_eq!(s.uids[&1000].sensors["11"].ms, 4349);
        assert_eq!(s.uids[&10429].mah, 105.0);
        assert_eq!(s.global_mah["cpu"], 369.0);
    }
}

// ---------- то же из `dumpsys batterystats --proto` (сводка без истории) ----------
// Схема: frameworks/base/core/proto/android/os/batterystats.proto (BatteryStatsProto),
// обёртка — android/service/batterystats.proto (BatteryStatsServiceDumpProto { batterystats = 1 }).

use crate::sys::pb::{fields, Val};

#[derive(Default, Clone, Copy)]
struct Timer {
    duration_ms: u64,
    count: u64,
    max_ms: u64,
    total_ms: u64,
}

fn timer(b: &[u8]) -> Timer {
    let mut t = Timer::default();
    for (n, v) in fields(b) {
        match n {
            1 => t.duration_ms = v.int(),
            2 => t.count = v.int(),
            3 => t.max_ms = v.int(),
            5 => t.total_ms = v.int(),
            _ => {}
        }
    }
    t
}

fn name_and<'a>(b: &'a [u8]) -> (String, Vec<(u32, Val<'a>)>) {
    let mut name = String::new();
    let mut rest = Vec::new();
    for (n, v) in fields(b) {
        if n == 1 {
            name = v.str();
        } else {
            rest.push((n, v));
        }
    }
    (name, rest)
}

fn sipper_label(code: u64) -> Option<&'static str> {
    Some(match code {
        1 => "idle",
        2 => "cell",
        3 => "phone",
        4 => "wifi",
        5 => "blue",
        6 => "flashlight",
        7 => "scrn",
        9 => "unacc",
        10 => "over",
        11 => "camera",
        12 => "memory",
        13 => "ambi",
        _ => return None,
    })
}

fn parse_system(b: &[u8], s: &mut CheckinSnap) {
    for (n, v) in fields(b) {
        match n {
            1 => {
                for (k, x) in fields(v.bytes()) {
                    match k {
                        1 => s.start_clock_ms = x.i64(),
                        5 => s.battery_realtime_ms = x.int(),
                        7 => s.screen_off_realtime_ms = x.int(),
                        9 => s.screen_doze_ms = x.int(),
                        _ => {}
                    }
                }
            }
            14 | 22 => {
                let (name, rest) = name_and(v.bytes());
                let t = rest.iter().find(|(k, _)| *k == 2).map(|(_, x)| timer(x.bytes())).unwrap_or_default();
                let tc = TimeCount { ms: t.duration_ms, count: t.count, ..Default::default() };
                if n == 14 {
                    s.kernel_wl.insert(name, tc);
                } else {
                    s.wakeup_reasons.insert(name, tc);
                }
            }
            15 => {
                for (k, x) in fields(v.bytes()) {
                    match k {
                        1 => s.screen_on_ms = x.int(),
                        4 => s.partial_wl_total_ms = x.int(),
                        5 => s.mobile_radio_active_ms = x.int(),
                        9 => s.interactive_ms = x.int(),
                        12 => s.doze_full_ms = x.int(),
                        17 => s.doze_light_ms = x.int(),
                        _ => {}
                    }
                }
            }
            17 => {
                let (mut code, mut mah) = (0u64, 0f64);
                for (k, x) in fields(v.bytes()) {
                    match k {
                        1 => code = x.int(),
                        3 => mah = x.f64(),
                        _ => {}
                    }
                }
                if let Some(l) = sipper_label(code) {
                    *s.global_mah.entry(l.to_string()).or_default() += mah;
                }
            }
            _ => {}
        }
    }
}

fn parse_uid(b: &[u8], s: &mut CheckinSnap) {
    let mut uid: u32 = 0; // ProtoOutputStream не пишет нули: нет поля — uid 0
    let mut u = UidCk::default();
    let mut pkgs = Vec::new();
    for (n, v) in fields(b) {
        let b = v.bytes();
        match n {
            1 => uid = v.int() as u32,
            2 => {
                let (name, _) = name_and(b);
                if !name.is_empty() {
                    pkgs.push(name);
                }
            }
            7 => {
                for (k, x) in fields(b) {
                    match k {
                        1 => u.cpu_user_ms = x.int(),
                        2 => u.cpu_sys_ms = x.int(),
                        _ => {}
                    }
                }
            }
            11 => u.fg_activity_ms = timer(b).duration_ms,
            12 => u.fgs_ms = timer(b).duration_ms,
            13 => u.vibrator_ms = timer(b).duration_ms,
            15 | 22 => {
                let (name, rest) = name_and(b);
                let mut tc = TimeCount::default();
                for (k, x) in rest {
                    let t = timer(x.bytes());
                    match k {
                        2 => {
                            tc.ms = t.duration_ms;
                            tc.count = t.count;
                        }
                        3 => {
                            tc.bg_ms = t.duration_ms;
                            tc.bg_count = t.count;
                        }
                        _ => {}
                    }
                }
                if n == 15 {
                    u.jobs.insert(name, tc);
                } else {
                    u.syncs.insert(name, tc);
                }
            }
            17 => {
                let mut x = Net::default();
                for (k, y) in fields(b) {
                    let i = y.int();
                    match k {
                        1 => x.mobile_rx = i,
                        2 => x.mobile_tx = i,
                        3 => x.wifi_rx = i,
                        4 => x.wifi_tx = i,
                        11 => x.mobile_active_ms = i,
                        12 => x.mobile_active_count = i,
                        13 => x.mobile_wakeups = i,
                        14 => x.wifi_wakeups = i,
                        15 => x.mobile_bg_rx = i,
                        16 => x.mobile_bg_tx = i,
                        17 => x.wifi_bg_rx = i,
                        18 => x.wifi_bg_tx = i,
                        _ => {}
                    }
                }
                u.net = x;
            }
            18 => {
                for (k, x) in fields(b) {
                    if k == 1 {
                        u.mah = x.f64();
                    }
                }
            }
            19 => {
                let (name, rest) = name_and(b);
                let mut p = ProcCk::default();
                for (k, x) in rest {
                    let i = x.int();
                    match k {
                        2 => p.user_ms = i,
                        3 => p.sys_ms = i,
                        4 => p.fg_ms = i,
                        5 => p.starts = i,
                        6 => p.anrs = i,
                        7 => p.crashes = i,
                        _ => {}
                    }
                }
                u.procs.insert(name, p);
            }
            21 => {
                let mut id: i64 = 0;
                let mut tc = TimeCount::default();
                for (k, x) in fields(b) {
                    match k {
                        1 => id = x.int() as i32 as i64,
                        2 => {
                            let t = timer(x.bytes());
                            tc.ms = if t.total_ms > 0 { t.total_ms } else { t.duration_ms };
                            tc.count = t.count;
                        }
                        3 => {
                            let t = timer(x.bytes());
                            tc.bg_ms = if t.total_ms > 0 { t.total_ms } else { t.duration_ms };
                            tc.bg_count = t.count;
                        }
                        _ => {}
                    }
                }
                u.sensors.insert(id.to_string(), tc);
            }
            24 => {
                for (k, x) in fields(b) {
                    match k {
                        1 => u.partial_wl_ms = x.int(),
                        2 => u.partial_wl_bg_ms = x.int(),
                        _ => {}
                    }
                }
            }
            25 => {
                let (name, rest) = name_and(b);
                let mut w = Wl::default();
                for (k, x) in rest {
                    let t = timer(x.bytes());
                    match k {
                        2 => {
                            w.full_ms = t.duration_ms;
                            w.full_count = t.count;
                        }
                        3 => {
                            w.blamed_ms = t.duration_ms;
                            w.count = t.count;
                            w.max_ms = t.max_ms;
                            w.ms = if t.total_ms > 0 { t.total_ms } else { t.duration_ms };
                        }
                        4 => w.bg_ms = t.duration_ms,
                        _ => {}
                    }
                }
                u.wakelocks.insert(name, w);
            }
            26 => {
                let (name, rest) = name_and(b);
                let c = rest.iter().find(|(k, _)| *k == 2).map(|(_, x)| x.int()).unwrap_or(0);
                *u.alarms.entry(name).or_default() += c;
            }
            27 => {
                for (k, x) in fields(b) {
                    if k == 3 {
                        let t = timer(x.bytes());
                        u.wifi_scans = t.count;
                        u.wifi_scan_ms = if t.total_ms > 0 { t.total_ms } else { t.duration_ms };
                    }
                }
            }
            _ => {}
        }
    }
    if !pkgs.is_empty() {
        s.uid_pkgs.entry(uid).or_default().extend(pkgs);
    }
    let e = s.uids.entry(uid).or_default();
    // один uid может прийти дважды — сливаем
    e.wakelocks.extend(u.wakelocks);
    e.jobs.extend(u.jobs);
    e.syncs.extend(u.syncs);
    e.sensors.extend(u.sensors);
    e.alarms.extend(u.alarms);
    e.procs.extend(u.procs);
    e.cpu_user_ms += u.cpu_user_ms;
    e.cpu_sys_ms += u.cpu_sys_ms;
    if !u.net.is_empty() {
        e.net = u.net;
    }
    e.wifi_scans += u.wifi_scans;
    e.wifi_scan_ms += u.wifi_scan_ms;
    e.fg_activity_ms += u.fg_activity_ms;
    e.fgs_ms += u.fgs_ms;
    e.vibrator_ms += u.vibrator_ms;
    e.partial_wl_ms += u.partial_wl_ms;
    e.partial_wl_bg_ms += u.partial_wl_bg_ms;
    e.mah += u.mah;
}

/// Сводка из `dumpsys batterystats --proto`. None — не похоже на BatteryStatsServiceDumpProto.
pub fn parse_proto(bytes: &[u8]) -> Option<CheckinSnap> {
    let bs = fields(bytes).find(|(n, _)| *n == 1)?.1;
    let mut s = CheckinSnap::default();
    let mut any = false;
    for (n, v) in fields(bs.bytes()) {
        match n {
            5 => {
                parse_uid(v.bytes(), &mut s);
                any = true;
            }
            6 => parse_system(v.bytes(), &mut s),
            _ => {}
        }
    }
    for v in s.uid_pkgs.values_mut() {
        v.sort();
        v.dedup();
    }
    if any && s.battery_realtime_ms > 0 { Some(s) } else { None }
}
