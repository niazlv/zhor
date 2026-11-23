//! Снимки системной статистики Android (batterystats, suspend, GPU) и разности между ними.

pub mod binder;
pub mod checkin;
pub mod native;
pub mod pb;
pub mod usage;

use crate::util;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SysSnap {
    /// настенное время снимка, мс
    pub t: i64,
    pub boot_ms: u64,
    pub usage: usage::UsageSnap,
    pub checkin: checkin::CheckinSnap,
    pub suspend: native::SuspendSnap,
    pub gpu_ms: BTreeMap<u32, u64>,
    /// pid → (uid, имя процесса) на момент снимка — для нативных wakelock
    pub pid_names: BTreeMap<u32, (u32, String)>,
    /// за кого работал system_server (binder_calls_stats, looper_stats) — накопительно;
    /// разность по окну считает report через binder::delta
    #[serde(default)]
    pub work: binder::SysWork,
    /// время съёма, мс (сколько стоило)
    pub cost_ms: u64,
}

/// Снимает всё. Дорого (~2–3 с system_server), поэтому редко: при смене экрана,
/// раз в час и по запросу. История событий Android — из того же вывода `batterystats -c`
/// (строки «9,h,…»), возвращается отдельно: в снимок (snaps.jsonl, JSON-разность) не попадает.
pub fn take_full(pid_names: BTreeMap<u32, (u32, String)>, with_history: bool) -> (SysSnap, Option<crate::history::Dump>) {
    let t0 = std::time::Instant::now();
    let usage = util::run_cmd("dumpsys", &["batterystats", "--usage"], 30_000).map(|t| usage::parse(&t)).unwrap_or_default();
    // Сводка — из `--proto` (~100 мс system_server, 270 КБ) вместо `-c` (~450 мс, 13 МБ с историей).
    // История — отдельным `-c --history` и только когда нужна (раз в несколько часов): буфер
    // Android держит ~20–45 ч. Если `--proto` не разобрался — по-старому из `-c`.
    let proto = util::run_cmd_bytes("dumpsys", &["batterystats", "--proto"], 30_000).and_then(|b| checkin::parse_proto(&b));
    let (checkin, hist) = match proto {
        Some(ck) => {
            let hist = if with_history {
                util::run_cmd("dumpsys", &["batterystats", "-c", "--history"], 60_000).map(|t| crate::history::parse_checkin(&t))
            } else {
                None
            };
            (ck, hist)
        }
        None => {
            let ck = util::run_cmd("dumpsys", &["batterystats", "-c"], 60_000);
            let checkin = ck.as_deref().map(checkin::parse).unwrap_or_default();
            let hist = ck.as_deref().map(crate::history::parse_checkin);
            (checkin, hist)
        }
    };
    let suspend = util::run_cmd("dumpsys", &["suspend_control_internal"], 15_000).map(|t| native::parse_suspend(&t)).unwrap_or_default();
    let gpu_ms = util::run_cmd("dumpsys", &["gpu", "--gpuwork"], 15_000).map(|t| native::parse_gpuwork(&t)).unwrap_or_default();
    let work = binder::take();
    // имена нужны только для pid из нативных wakelock
    let wanted: std::collections::BTreeSet<u32> = suspend.native.values().map(|w| w.pid).collect();
    let pid_names: BTreeMap<u32, (u32, String)> = pid_names.into_iter().filter(|(p, _)| wanted.contains(p)).collect();
    let snap = SysSnap {
        t: util::real_ms(),
        boot_ms: util::boot_ms(),
        usage,
        checkin,
        suspend,
        gpu_ms,
        pid_names,
        work,
        cost_ms: t0.elapsed().as_millis() as u64,
    };
    (snap, hist)
}

/// Рекурсивная разность JSON: числа вычитаются (не меньше нуля), строки/прочее — из `b`.
fn sub_value(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
            let mut out = serde_json::Map::new();
            for (k, vb) in mb {
                match ma.get(k) {
                    Some(va) => {
                        let d = sub_value(va, vb);
                        out.insert(k.clone(), d);
                    }
                    None => {
                        out.insert(k.clone(), vb.clone());
                    }
                }
            }
            Value::Object(out)
        }
        (Value::Array(xa), Value::Array(xb)) if xa.len() == xb.len() && xb.iter().all(|x| x.is_number()) => {
            Value::Array(xa.iter().zip(xb).map(|(x, y)| sub_value(x, y)).collect())
        }
        (Value::Number(x), Value::Number(y)) => {
            if let (Some(x), Some(y)) = (x.as_u64(), y.as_u64()) {
                Value::from(y.saturating_sub(x))
            } else {
                let d = y.as_f64().unwrap_or(0.0) - x.as_f64().unwrap_or(0.0);
                Value::from(if d > 0.0 { d } else { 0.0 })
            }
        }
        _ => b.clone(),
    }
}

/// Разность снимков b − a. Если между ними batterystats сбросился (зарядка), часть
/// «с последней зарядки» берётся из b целиком.
pub fn delta(a: &SysSnap, b: &SysSnap) -> SysSnap {
    let reset = b.checkin.start_clock_ms != a.checkin.start_clock_ms
        || b.checkin.battery_realtime_ms < a.checkin.battery_realtime_ms;
    let reboot = b.boot_ms < a.boot_ms;
    let mut out = b.clone();
    if !reset {
        let va = serde_json::to_value(&a.usage).unwrap_or(Value::Null);
        let vb = serde_json::to_value(&b.usage).unwrap_or(Value::Null);
        out.usage = serde_json::from_value(sub_value(&va, &vb)).unwrap_or_default();
        let va = serde_json::to_value(&a.checkin).unwrap_or(Value::Null);
        let vb = serde_json::to_value(&b.checkin).unwrap_or(Value::Null);
        out.checkin = serde_json::from_value(sub_value(&va, &vb)).unwrap_or_default();
        // эти поля — не счётчики
        out.checkin.start_clock_ms = b.checkin.start_clock_ms;
        out.checkin.uid_pkgs = b.checkin.uid_pkgs.clone();
        out.usage.capacity = b.usage.capacity;
    }
    if !reboot {
        let va = serde_json::to_value(&a.suspend).unwrap_or(Value::Null);
        let vb = serde_json::to_value(&b.suspend).unwrap_or(Value::Null);
        out.suspend = serde_json::from_value(sub_value(&va, &vb)).unwrap_or_default();
        for (k, w) in out.suspend.native.iter_mut() {
            if let Some(o) = b.suspend.native.get(k) {
                w.name = o.name.clone();
                w.pid = o.pid;
                w.max_ms = o.max_ms;
            }
        }
        let mut g = BTreeMap::new();
        for (uid, v) in &b.gpu_ms {
            g.insert(*uid, v.saturating_sub(a.gpu_ms.get(uid).copied().unwrap_or(0)));
        }
        out.gpu_ms = g;
    }
    out.pid_names = a.pid_names.clone();
    for (k, v) in &b.pid_names {
        out.pid_names.insert(*k, v.clone());
    }
    out
}

/// pid изолированного процесса → uid приложения-хозяина.
/// В `dumpsys activity processes` процесс записан как `PID:имя/u0a309i76`: `a309` — ApplicationInfo
/// хозяина (для рендеров WebView это приложение, открывшее WebView), `i76` — изолированный uid.
/// (`activity lru` дешевле, но содержит не все изолированные процессы.)
pub fn isolated_hosts() -> BTreeMap<u32, u32> {
    let text = util::run_cmd("dumpsys", &["activity", "processes"], 15_000).unwrap_or_default();
    parse_isolated_hosts(&text)
}

pub fn parse_isolated_hosts(text: &str) -> BTreeMap<u32, u32> {
    let mut out = BTreeMap::new();
    for tok in text.split_whitespace() {
        let Some((pid, rest)) = tok.split_once(':') else { continue };
        let Ok(pid) = pid.parse::<u32>() else { continue };
        let Some(label) = rest.rsplit('/').next() else { continue };
        // label: u<user>a<app>i<iso>
        let Some(l) = label.strip_prefix('u') else { continue };
        let Some(apos) = l.find('a') else { continue };
        let Some(ipos) = l.rfind('i') else { continue };
        if ipos <= apos {
            continue;
        }
        let (Ok(user), Ok(app)) = (l[..apos].parse::<u32>(), l[apos + 1..ipos].parse::<u32>()) else { continue };
        out.insert(pid, user * 100_000 + 10_000 + app);
    }
    out
}

/// uid → пакеты из `cmd package list packages -U`.
pub fn packages() -> BTreeMap<u32, Vec<String>> {
    let mut out: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let Some(text) = util::run_cmd("cmd", &["package", "list", "packages", "-U"], 20_000) else { return out };
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("package:") else { continue };
        let Some((pkg, uids)) = rest.split_once(" uid:") else { continue };
        for u in uids.split(',') {
            if let Ok(uid) = u.trim().parse::<u32>() {
                out.entry(uid).or_default().push(pkg.trim().to_string());
            }
        }
    }
    for v in out.values_mut() {
        v.sort();
        v.dedup();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hosts() {
        let t = "  #200: prev +12 CACC --------- 2028:com.google.android.webview:sandboxed_process0:org.chromium.content.app.SandboxedProcessService0:0/u0a309i76 act:client\n  #12: fg  T  SVC  x 3827:system/1000\n  #5: prev 9073:com.google.android.webview:sandboxed_process0:x:0/u10a357i45 act:client\n";
        let h = parse_isolated_hosts(t);
        assert_eq!(h.get(&2028), Some(&10_309));
        assert_eq!(h.get(&9073), Some(&1_010_357));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn json_delta() {
        let a = serde_json::json!({"x": 5, "m": {"k": 1.5, "s": "a"}, "arr": [1, 2]});
        let b = serde_json::json!({"x": 7, "m": {"k": 2.0, "s": "b", "n": 3}, "arr": [4, 4]});
        let d = sub_value(&a, &b);
        assert_eq!(d["x"], 2);
        assert_eq!(d["m"]["k"], 0.5);
        assert_eq!(d["m"]["s"], "b");
        assert_eq!(d["m"]["n"], 3);
        assert_eq!(d["arr"], serde_json::json!([3, 2]));
    }
}
