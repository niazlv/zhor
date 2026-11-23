//! «За кого работает system»: Android сам ведёт выборочную статистику работы system_server.
//!
//! * `dumpsys binder_calls_stats` — входящие binder-вызовы: кто (uid) вызвал, какой метод,
//!   сколько CPU system_server ушло. Пишется каждый N-й вызов, поэтому время масштабируем
//!   на `call_count / recorded_call_count`.
//! * `dumpsys looper_stats` — внутренние очереди system_server: поток, обработчик, сообщение,
//!   CPU; `work_source_uid` — от чьего имени работа (-1 — сама система).
//!
//! Счётчики накопительные с «Start time»; разность между снимками считает `sys::delta`.

use crate::sys::usage::parse_uid_label;
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Caller {
    pub label: String,
    #[serde(default, skip_serializing_if = "util::is_zero_f")]
    pub cpu_ms: f64,
    #[serde(default, skip_serializing_if = "util::is_zero")]
    pub calls: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Work {
    #[serde(default, skip_serializing_if = "util::is_zero_f")]
    pub cpu_ms: f64,
    #[serde(default, skip_serializing_if = "util::is_zero_f")]
    pub cpu_ms_off: f64,
    #[serde(default, skip_serializing_if = "util::is_zero")]
    pub count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SysWork {
    /// «Start time» статистики binder / looper — если сменилось, счётчики сброшены
    pub binder_start: String,
    pub looper_start: String,
    #[serde(default, skip_serializing_if = "util::is_zero_f")]
    pub binder_total_ms: f64,
    #[serde(default, skip_serializing_if = "util::is_zero")]
    pub binder_total_calls: u64,
    /// uid вызывающего → сводка (из Per-UID Summary)
    pub callers: BTreeMap<String, Caller>,
    /// «uid|Класс#метод» → CPU, вызовы
    pub calls: BTreeMap<String, Work>,
    /// «поток|Обработчик|сообщение» → CPU, сообщения
    pub looper: BTreeMap<String, Work>,
    /// uid, от чьего имени работали очереди (-1 → «system»)
    pub looper_by_uid: BTreeMap<String, Work>,
    /// включённые службы спецвозможностей (из-за них все приложения шлют события в system_server)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub a11y: Vec<String>,
}

/// «shared:com.google.uid.shared/u0a165» → (10165, «com.google.uid.shared»); «-1» → None.
pub fn parse_who(s: &str) -> Option<(u32, String)> {
    let s = s.trim();
    if s == "-1" || s.is_empty() {
        return None;
    }
    let (label, uid) = match s.rsplit_once('/') {
        Some((l, u)) => (l.trim_start_matches("shared:").to_string(), u),
        None => (String::new(), s),
    };
    let uid = parse_uid_label(uid)?;
    Some((uid, label))
}

/// «com.android.server.accounts.AccountManagerService#getUserData» → «AccountManagerService#getUserData».
pub fn short_call(s: &str) -> String {
    let (cls, method) = s.split_once('#').unwrap_or((s, ""));
    let cls = cls.rsplit('.').next().unwrap_or(cls);
    let cls = cls.replace("$Stub", "").replace("$IPackageManagerImpl", "");
    if method.is_empty() { cls } else { format!("{cls}#{method}") }
}

fn short_tail(s: &str, n: usize) -> String {
    let t = s.rsplit('.').next().unwrap_or(s);
    t.chars().take(n).collect()
}

fn csv(line: &str) -> Vec<String> {
    // в именах вызовов и сообщений запятых не бывает, но на всякий случай учитываем кавычки
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

fn f(v: &[String], i: usize) -> f64 {
    v.get(i).and_then(|x| x.trim().parse::<f64>().ok()).unwrap_or(0.0)
}

pub fn parse_binder(text: &str, w: &mut SysWork) {
    #[derive(PartialEq)]
    enum Sec {
        None,
        Raw,
        Summary,
    }
    let mut sec = Sec::None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Start time:") {
            w.binder_start = v.trim().to_string();
            continue;
        }
        if line.starts_with("Per-UID raw data") {
            sec = Sec::Raw;
            continue;
        }
        if line.starts_with("Per-UID Summary") {
            sec = Sec::Summary;
            continue;
        }
        if let Some(v) = line.trim().strip_prefix("Summary:") {
            for kv in v.split(',') {
                if let Some((k, val)) = kv.trim().split_once('=') {
                    match k {
                        "total_cpu_time" => w.binder_total_ms = val.parse().unwrap_or(0.0),
                        "calls_count" => w.binder_total_calls = val.parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
            continue;
        }
        if !line.starts_with("    ") {
            if !line.trim().is_empty() {
                sec = Sec::None;
            }
            continue;
        }
        match sec {
            Sec::Raw => {
                // package/uid, worksource, call_desc, screen_interactive, cpu_us, max_cpu, latency, max_lat,
                // exceptions, max_req, max_reply, recorded_call_count, call_count
                let v = csv(line.trim());
                if v.len() < 13 {
                    continue;
                }
                // работу приписываем worksource (от чьего имени), иначе — вызвавшему
                let who = parse_who(&v[1]).or_else(|| parse_who(&v[0]));
                let Some((uid, _)) = who else { continue };
                let (rec, cnt) = (f(&v, 11), f(&v, 12));
                if rec <= 0.0 {
                    continue;
                }
                let cpu_ms = f(&v, 4) * cnt / rec / 1000.0;
                let e = w.calls.entry(format!("{uid}|{}", short_call(&v[2]))).or_default();
                e.cpu_ms += cpu_ms;
                if v[3].trim() == "false" {
                    e.cpu_ms_off += cpu_ms;
                }
                e.count += cnt as u64;
            }
            Sec::Summary => {
                // cpu_time(ms), %, recorded, call_count, package/uid
                let t: Vec<&str> = line.split_whitespace().collect();
                if t.len() < 5 {
                    continue;
                }
                let Some((uid, label)) = parse_who(t[4]) else { continue };
                let c = w.callers.entry(uid.to_string()).or_default();
                c.label = label;
                c.cpu_ms += t[0].parse::<f64>().unwrap_or(0.0);
                c.calls += t[3].parse::<u64>().unwrap_or(0);
            }
            Sec::None => {}
        }
    }
}

pub fn parse_looper(text: &str, w: &mut SysWork) {
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Start time:") {
            w.looper_start = v.trim().to_string();
            continue;
        }
        if line.starts_with("work_source_uid") || !line.contains(',') {
            continue;
        }
        // work_source_uid, thread_name, handler_class, message_name, is_interactive, message_count,
        // recorded_message_count, total_latency_micros, max_latency_micros, total_cpu_micros, ...
        let v = csv(line.trim());
        if v.len() < 10 {
            continue;
        }
        let (cnt, rec) = (f(&v, 5), f(&v, 6));
        if rec <= 0.0 {
            continue;
        }
        let cpu_ms = f(&v, 9) * cnt / rec / 1000.0;
        let off = v[4].trim() == "false";
        let thread = if v[1].is_empty() { "<прочее>".to_string() } else { util::normalize_name(&v[1]) };
        let key = format!("{thread}|{}|{}", short_tail(&v[2], 48), short_tail(&v[3], 60));
        let e = w.looper.entry(key).or_default();
        e.cpu_ms += cpu_ms;
        if off {
            e.cpu_ms_off += cpu_ms;
        }
        e.count += cnt as u64;
        let ws = parse_who(&v[0]).map(|(u, _)| u.to_string()).unwrap_or_else(|| "-1".into());
        let e = w.looper_by_uid.entry(ws).or_default();
        e.cpu_ms += cpu_ms;
        if off {
            e.cpu_ms_off += cpu_ms;
        }
        e.count += cnt as u64;
    }
    trim(w);
}

/// Мелочь не храним: снимок пишется на диск целиком. Записи, перешедшие порог между
/// снимками, войдут в разность целиком — ошибка не больше порога на запись.
pub fn trim(w: &mut SysWork) {
    w.calls.retain(|_, x| x.cpu_ms >= 20.0 || x.count >= 5000);
    w.looper.retain(|_, x| x.cpu_ms >= 20.0);
    w.looper_by_uid.retain(|_, x| x.cpu_ms >= 10.0);
}

fn sub_work(a: &Work, b: &Work) -> Work {
    Work {
        cpu_ms: (b.cpu_ms - a.cpu_ms).max(0.0),
        cpu_ms_off: (b.cpu_ms_off - a.cpu_ms_off).max(0.0),
        count: b.count.saturating_sub(a.count),
    }
}

fn sub_map(a: &BTreeMap<String, Work>, b: &BTreeMap<String, Work>) -> BTreeMap<String, Work> {
    b.iter()
        .map(|(k, vb)| (k.clone(), a.get(k).map(|va| sub_work(va, vb)).unwrap_or_else(|| vb.clone())))
        .filter(|(_, w)| w.cpu_ms > 0.0 || w.count > 0)
        .collect()
}

/// Разность b − a. Если статистика сброшена (сменился Start time) — берём b как есть.
pub fn delta(a: &SysWork, b: &SysWork) -> SysWork {
    let mut out = b.clone();
    if !a.binder_start.is_empty() && a.binder_start == b.binder_start {
        out.binder_total_ms = (b.binder_total_ms - a.binder_total_ms).max(0.0);
        out.binder_total_calls = b.binder_total_calls.saturating_sub(a.binder_total_calls);
        out.calls = sub_map(&a.calls, &b.calls);
        out.callers = b
            .callers
            .iter()
            .map(|(k, vb)| {
                let va = a.callers.get(k);
                (
                    k.clone(),
                    Caller {
                        label: vb.label.clone(),
                        cpu_ms: (vb.cpu_ms - va.map(|x| x.cpu_ms).unwrap_or(0.0)).max(0.0),
                        calls: vb.calls.saturating_sub(va.map(|x| x.calls).unwrap_or(0)),
                    },
                )
            })
            .filter(|(_, c)| c.cpu_ms > 0.0)
            .collect();
    }
    if !a.looper_start.is_empty() && a.looper_start == b.looper_start {
        out.looper = sub_map(&a.looper, &b.looper);
        out.looper_by_uid = sub_map(&a.looper_by_uid, &b.looper_by_uid);
    }
    out
}

/// Снять оба дампа и список служб спецвозможностей (~50 мс).
pub fn take() -> SysWork {
    let mut w = SysWork::default();
    if let Some(t) = util::run_cmd("dumpsys", &["binder_calls_stats"], 15_000) {
        parse_binder(&t, &mut w);
    }
    if let Some(t) = util::run_cmd("dumpsys", &["looper_stats"], 15_000) {
        parse_looper(&t, &mut w);
    }
    trim(&mut w);
    if let Some(t) = util::run_cmd("settings", &["get", "secure", "enabled_accessibility_services"], 5_000) {
        let t = t.trim();
        if !t.is_empty() && t != "null" {
            w.a11y = t.split(':').map(|s| s.to_string()).filter(|s| !s.is_empty()).collect();
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    const B: &str = "Start time: 2026-11-22 17:44:37
On battery time (ms): 26270590
Per-UID raw data (top 90% by cpu time) (pid, package/uid, ...):
    shared:com.google.uid.shared/u0a165,shared:com.google.uid.shared/u0a165,com.android.server.accounts.AccountManagerService#getUserData,false,68759,5089,120178,17937,0,352,528,26,19608
    com.example.vpn/u0a358,com.example.vpn/u0a358,com.android.server.notification.NotificationManagerService$Stub#enqueueNotificationWithTag,true,10856,2805,11798,2872,0,4300,4,7,11235

Per-UID Summary (top 90% by cpu time) (cpu_time, % of total cpu_time, recorded_call_count, call_count, package/uid):
      202578  24%      276   272279 shared:com.google.uid.shared/u0a165
       76884   9%      562   547232 shared:android.uid.systemui/1000

  Summary: total_cpu_time=859188, calls_count=3727003, avg_call_cpu_time=224
";
    const L: &str = "Start time: 2026-11-22 17:44:37
work_source_uid,thread_name,handler_class,message_name,is_interactive,message_count,recorded_message_count,total_latency_micros,max_latency_micros,total_cpu_micros,max_cpu_micros,recorded_delay_message_count,total_delay_millis,max_delay_millis,exception_count
-1,UMMS-NormalFileReclaim,com.miui.server.MyHandler,0x1,false,100,10,0,0,50000,0,0,0,0,0
com.example.chat/u999a362,ConnectivityServiceThread,android.net.ConnectivityService$InternalHandler,0x15,true,1,1,2090,2090,20081,20081,1,0,0,0
";
    #[test]
    fn work_delta() {
        let mut a = SysWork::default();
        parse_binder(B, &mut a);
        let mut b = a.clone();
        b.callers.get_mut("10165").unwrap().cpu_ms += 1000.0;
        b.binder_total_ms += 1000.0;
        let d = delta(&a, &b);
        assert_eq!(d.callers.len(), 1);
        assert_eq!(d.callers["10165"].cpu_ms, 1000.0);
        assert_eq!(d.binder_total_ms, 1000.0);
        assert!(d.calls.is_empty());
        let mut c = b.clone();
        c.binder_start = "другой".into();
        assert_eq!(delta(&a, &c).binder_total_ms, c.binder_total_ms);
    }

    #[test]
    fn binder_and_looper() {
        let mut w = SysWork::default();
        parse_binder(B, &mut w);
        parse_looper(L, &mut w);
        assert_eq!(w.binder_start, "2026-11-22 17:44:37");
        assert_eq!(w.binder_total_ms, 859_188.0);
        let g = &w.callers["10165"];
        assert_eq!((g.label.as_str(), g.cpu_ms as u64, g.calls), ("com.google.uid.shared", 202_578, 272_279));
        assert_eq!(w.callers["1000"].calls, 547_232);
        let c = &w.calls["10165|AccountManagerService#getUserData"];
        assert!((c.cpu_ms - 68.759 * 19608.0 / 26.0).abs() < 1.0);
        assert_eq!(c.cpu_ms, c.cpu_ms_off);
        assert!(w.calls.contains_key("10358|NotificationManagerService#enqueueNotificationWithTag"));
        let u = &w.looper["UMMS-NormalFileReclaim|MyHandler|0x1"];
        assert_eq!(u.cpu_ms, 500.0);
        assert!(w.looper_by_uid.contains_key("-1"));
        assert!(w.looper_by_uid.contains_key("99910362")); // u999a362
        assert_eq!(parse_who("0"), Some((0, String::new())));
    }
}
