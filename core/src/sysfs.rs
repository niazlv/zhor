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
        ScreenReader { dpms, backlight }
    }

    pub fn read(&self) -> Screen {
        let bright = self
            .backlight
            .as_ref()
            .and_then(|(p, max)| util::read_i64(p).map(|v| (v as f64 / max).clamp(0.0, 1.0)))
            .unwrap_or(0.0);
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
}

impl WakeupSources {
    pub fn new() -> Self {
        let mut w = WakeupSources { dirs: Vec::new(), prev: HashMap::new(), last_enum_ms: 0 };
        w.enumerate(0);
        w
    }

    fn enumerate(&mut self, now: u64) {
        self.last_enum_ms = now;
        self.dirs.clear();
        let Ok(rd) = std::fs::read_dir("/sys/class/wakeup") else { return };
        let mut seen: HashMap<String, u32> = HashMap::new();
        let mut v: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.path().to_string_lossy().into_owned()).collect();
        v.sort();
        for p in v {
            let mut name = util::read_str(format!("{p}/name")).unwrap_or_default();
            if name.is_empty() {
                name = "<без имени>".into();
            }
            let n = seen.entry(name.clone()).or_insert(0);
            *n += 1;
            let key = if *n > 1 { format!("{name} #{n}") } else { name };
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
