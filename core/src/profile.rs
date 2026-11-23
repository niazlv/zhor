//! Профиль мощности: коэффициенты производителя (`dumpsys batterystats --power-profile`)
//! и раскладка кластеров CPU из cpufreq.

use crate::util;
use serde::Serialize;
use std::collections::BTreeMap;

/// Кластер CPU (cpufreq policy) с таблицей «частота → мА на ядро».
#[derive(Debug, Clone, Serialize)]
pub struct Cluster {
    pub policy: String,
    pub cpus: Vec<u32>,
    /// Частоты ядра (кГц) в порядке time_in_state.
    pub freqs_khz: Vec<u64>,
    /// мА на одно ядро на каждой частоте freqs_khz (с учётом доли кластера).
    pub ma: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PowerProfile {
    pub source: String,
    pub clusters: Vec<Cluster>,
    pub cpu_active_ma: f64,
    pub cpu_idle_ma: f64,
    pub screen_on_ma: f64,
    pub screen_full_ma: f64,
    pub ambient_ma: f64,
    pub capacity_mah: f64,
    pub raw: BTreeMap<String, Vec<f64>>,
}

impl PowerProfile {
    /// Средний ток одного ядра кластера, если частота неизвестна (середина таблицы).
    pub fn mid_ma(&self, cluster: usize) -> f64 {
        let c = &self.clusters[cluster];
        if c.ma.is_empty() {
            return 100.0;
        }
        c.ma[c.ma.len() / 2]
    }

    fn get1(&self, key: &str) -> Option<f64> {
        self.raw.get(key).and_then(|v| v.first().copied())
    }
}

/// Разбирает вывод `dumpsys batterystats --power-profile`.
pub fn parse_dumpsys(text: &str) -> BTreeMap<String, Vec<f64>> {
    let mut raw = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once('=') else { continue };
        let k = k.trim();
        let v = v.trim();
        if k.is_empty() || k.contains(' ') || k.contains(':') {
            continue;
        }
        let vals: Vec<f64> = if let Some(inner) = v.strip_prefix('[') {
            inner
                .trim_end_matches(']')
                .split(',')
                .filter_map(|x| x.trim().parse().ok())
                .collect()
        } else {
            v.split_whitespace().next().and_then(|x| x.parse().ok()).into_iter().collect()
        };
        if !vals.is_empty() {
            raw.insert(k.to_string(), vals);
        }
    }
    raw
}

/// Читает cpufreq policy из sysfs: (имя, cpus, частоты по time_in_state).
pub fn read_policies(root: &str) -> Vec<(String, Vec<u32>, Vec<u64>)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else { return out };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("policy"))
        .collect();
    names.sort_by_key(|n| n[6..].parse::<u32>().unwrap_or(0));
    for n in names {
        let base = format!("{root}/{n}");
        let cpus: Vec<u32> = util::read_str(format!("{base}/related_cpus"))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        let freqs: Vec<u64> = util::read_str(format!("{base}/stats/time_in_state"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_whitespace().next()?.parse().ok())
            .collect();
        out.push((n, cpus, freqs));
    }
    out
}

/// Линейная интерполяция мА по частоте из таблицы профиля.
fn interp(speeds: &[f64], power: &[f64], f: f64) -> f64 {
    let n = speeds.len().min(power.len());
    if n == 0 {
        return 0.0;
    }
    if f <= speeds[0] {
        return power[0] * (f / speeds[0]).max(0.3);
    }
    for i in 1..n {
        if f <= speeds[i] {
            let t = (f - speeds[i - 1]) / (speeds[i] - speeds[i - 1]).max(1.0);
            return power[i - 1] + t * (power[i] - power[i - 1]);
        }
    }
    power[n - 1] * (f / speeds[n - 1])
}

/// Собирает профиль: коэффициенты из dumpsys + реальные частоты ядра.
pub fn build(raw: BTreeMap<String, Vec<f64>>, source: &str, policies: &[(String, Vec<u32>, Vec<u64>)]) -> PowerProfile {
    let mut clusters = Vec::new();
    for (i, (name, cpus, freqs)) in policies.iter().enumerate() {
        let pidx = name[6..].parse::<usize>().unwrap_or(0);
        // Новые профили именуют по policy, старые — по cluster.
        let speeds = raw
            .get(&format!("cpu.core_speeds.cluster{i}"))
            .or_else(|| raw.get(&format!("cpu.core_speeds.policy{pidx}")))
            .or_else(|| raw.get(&format!("cpu.scaling_step_speeds.policy{pidx}")));
        let power = raw
            .get(&format!("cpu.core_power.cluster{i}"))
            .or_else(|| raw.get(&format!("cpu.core_power.policy{pidx}")))
            .or_else(|| raw.get(&format!("cpu.scaling_step_power.policy{pidx}")));
        let cluster_extra = raw
            .get(&format!("cpu.cluster_power.cluster{i}"))
            .or_else(|| raw.get(&format!("cpu.scaling_policy_power.policy{pidx}")))
            .and_then(|v| v.first().copied())
            .unwrap_or(0.0);
        let ncores = cpus.len().max(1) as f64;
        let ma: Vec<f64> = match (speeds, power) {
            (Some(s), Some(p)) if !s.is_empty() && !p.is_empty() => freqs
                .iter()
                .map(|&f| interp(s, p, f as f64) + cluster_extra / ncores)
                .collect(),
            _ => {
                // Профиля нет: грубая оценка ~ 40 мА + 0.12 мА/МГц.
                freqs.iter().map(|&f| 40.0 + 0.12 * f as f64 / 1000.0).collect()
            }
        };
        clusters.push(Cluster { policy: name.clone(), cpus: cpus.clone(), freqs_khz: freqs.clone(), ma });
    }
    let g = |k: &str| raw.get(k).and_then(|v| v.first().copied());
    let pp = PowerProfile {
        source: source.to_string(),
        clusters,
        cpu_active_ma: g("cpu.active").unwrap_or(0.0),
        cpu_idle_ma: g("cpu.idle").unwrap_or(0.0),
        screen_on_ma: g("screen.on.display0").or_else(|| g("screen.on")).unwrap_or(0.0),
        screen_full_ma: g("screen.full.display0").or_else(|| g("screen.full")).unwrap_or(0.0),
        ambient_ma: g("ambient.on.display0").or_else(|| g("ambient.on")).unwrap_or(0.0),
        capacity_mah: g("battery.capacity").unwrap_or(0.0),
        raw,
    };
    let _ = pp.get1("none");
    pp
}

/// Профиль с устройства: dumpsys, иначе — оценка.
pub fn load_device() -> PowerProfile {
    let policies = read_policies("/sys/devices/system/cpu/cpufreq");
    let text = util::run_cmd("dumpsys", &["batterystats", "--power-profile"], 15_000).unwrap_or_default();
    let raw = parse_dumpsys(&text);
    let src = if raw.contains_key("cpu.active") || raw.keys().any(|k| k.starts_with("cpu.core_power")) {
        "dumpsys batterystats --power-profile"
    } else {
        "оценка (профиль производителя недоступен)"
    };
    build(raw, src, &policies)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_and_interp() {
        let t = "Power Profile: \n    cpu.active=17.05 \n    cpu.core_power.cluster1=[168.06, 205.24, 248.53] \n    cpu.core_speeds.cluster1=[614400.0, 1017600.0, 1401600.0] \n    drain:SLEEP,RAT:DEFAULT=4.0\n";
        let raw = parse_dumpsys(t);
        assert_eq!(raw["cpu.active"], vec![17.05]);
        assert_eq!(raw["cpu.core_power.cluster1"].len(), 3);
        assert!(!raw.contains_key("drain:SLEEP,RAT:DEFAULT"));
        let pol = vec![
            ("policy0".to_string(), vec![0], vec![384000]),
            ("policy6".to_string(), vec![6, 7], vec![1017600, 1209600, 1401600]),
        ];
        let pp = build(raw, "t", &pol);
        let c = &pp.clusters[1];
        assert!((c.ma[0] - 205.24).abs() < 0.01);
        assert!(c.ma[1] > 205.24 && c.ma[1] < 248.53);
    }
}
