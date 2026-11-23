//! `dumpsys suspend_control_internal`: нативные wakelock с PID (демоны, HAL) и
//! сводка засыпаний; `dumpsys gpu --gpuwork`: время GPU по UID.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NativeWl {
    pub name: String,
    pub pid: u32,
    pub count: u64,
    pub ms: u64,
    pub max_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuspendSnap {
    /// ключ «имя|pid»
    pub native: BTreeMap<String, NativeWl>,
    /// причина неудачного засыпания/пробуждения → число
    pub wakeups: BTreeMap<String, u64>,
    pub attempts: u64,
    pub failed: u64,
    pub suspend_ms: u64,
    pub awake_between_ms: u64,
}

fn ms(s: &str) -> u64 {
    s.trim().trim_end_matches("ms").trim().parse().unwrap_or(0)
}

pub fn parse_suspend(text: &str) -> SuspendSnap {
    let mut s = SuspendSnap::default();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('|') {
            let cols: Vec<&str> = t.trim_matches('|').split('|').map(|c| c.trim()).collect();
            if cols.len() >= 7 && cols[2] == "Native" {
                let pid: u32 = cols[1].parse().unwrap_or(0);
                let name = if cols[0].is_empty() { "<без имени>".to_string() } else { cols[0].to_string() };
                let w = NativeWl { name: name.clone(), pid, count: cols[4].parse().unwrap_or(0), ms: ms(cols[5]), max_ms: ms(cols[6]) };
                s.native.insert(format!("{name}|{pid}"), w);
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("WakeupInfo{name: ") {
            if let Some((name, cnt)) = rest.rsplit_once(", count: ") {
                let c: u64 = cnt.trim_end_matches('}').trim().parse().unwrap_or(0);
                *s.wakeups.entry(name.to_string()).or_default() += c;
            }
            continue;
        }
        if let Some((k, v)) = t.split_once(':') {
            let v = v.trim();
            match k {
                "suspend attempts" => s.attempts = v.parse().unwrap_or(0),
                "failed suspends" => s.failed = v.parse().unwrap_or(0),
                "total suspend time" => s.suspend_ms = ms(v),
                "total sleep time between suspends" => s.awake_between_ms = ms(v),
                _ => {}
            }
        }
    }
    s
}

/// uid → активное время GPU, мс (с загрузки).
pub fn parse_gpuwork(text: &str) -> BTreeMap<u32, u64> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let c: Vec<&str> = line.split_whitespace().collect();
        if c.len() >= 3 {
            if let (Ok(uid), Ok(ns)) = (c[1].parse::<u32>(), c[2].parse::<u64>()) {
                *out.entry(uid).or_default() += ns / 1_000_000;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suspend() {
        let t = " | qms_event_Handler_wakeLock_    |   3193 | Native | Inactive |         3315 |      13975ms |        801ms |          --- |
 | IPA_WS                         |    --- | Kernel | Inactive |       262004 |      30347ms |         56ms |       262004 |
WakeupInfo{name: Abort: Pending Wakeup Sources: [timerfd], count: 95}
suspend attempts: 193
total suspend time: 88458 ms
";
        let s = parse_suspend(t);
        let w = &s.native["qms_event_Handler_wakeLock_|3193"];
        assert_eq!((w.count, w.ms, w.max_ms), (3315, 13975, 801));
        assert_eq!(s.native.len(), 1);
        assert_eq!(s.wakeups["Abort: Pending Wakeup Sources: [timerfd]"], 95);
        assert_eq!(s.attempts, 193);
        assert_eq!(s.suspend_ms, 88458);
        let g = parse_gpuwork("GPU work information.\ngpu_id uid total_active_duration_ns total_inactive_duration_ns\n1 1000 239492624702 16246734495405\n");
        assert_eq!(g[&1000], 239_492);
    }
}
