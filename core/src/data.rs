//! Настройки ядра, состояние хранилища, выгрузка и очистка данных.

use crate::report::{self, Query};
use crate::sampler::Daemon;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// шаг сбора при включённом экране, мс
    pub interval_on_ms: u64,
    /// шаг сбора при выключенном экране (по времени бодрствования), мс
    pub interval_off_ms: u64,
    /// сколько дней хранить записи
    pub keep_days: i64,
    /// как часто снимать batterystats, мин
    pub snap_every_min: u64,
    /// подробная история Android (`dumpsys batterystats enable full-history`): каждый wakelock,
    /// будильники, запуски процессов. Android сбрасывает её при перезагрузке — ядро включает
    /// заново перед каждым снимком, пока флаг стоит.
    pub full_history: bool,
    /// как часто читать полную историю Android (`-c --history`, ~0,4 с CPU system_server), мин;
    /// буфер Android держит ~20 ч с full_history и ~45 ч без — раз в 3 ч с запасом
    pub hist_every_min: u64,
    /// (0.6) сторож: проверять правила и слать тревоги приложению
    pub watch: bool,
    /// тревога, если в глубоком сне меньше этой доли покоя, % (0 — правило выключено)
    pub watch_sleep_pct: f64,
    /// разряд в покое больше, %/ч
    pub watch_drain_pct_h: f64,
    /// источник пробуждения будит чаще, раз в час
    pub watch_wakeups_h: f64,
    /// приложение тратит процессор в покое больше, мА в среднем
    pub watch_app_ma: f64,
    /// батарея горячее, °C, при выключенном экране
    pub watch_hot_c: f64,
    /// «не напоминать»: ключи тревог (правило или правило:виновник)
    pub watch_mute: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            interval_on_ms: 5_000,
            interval_off_ms: 20_000,
            keep_days: 10,
            snap_every_min: 60,
            full_history: false,
            hist_every_min: 180,
            watch: true,
            watch_sleep_pct: 50.0,
            watch_drain_pct_h: 3.0,
            watch_wakeups_h: 600.0,
            watch_app_ma: 10.0,
            watch_hot_c: 40.0,
            watch_mute: Vec::new(),
        }
    }
}

impl Config {
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Config>(&s).ok())
            .map(|c| c.clamped())
            .unwrap_or_default()
    }

    pub fn clamped(mut self) -> Self {
        self.interval_on_ms = self.interval_on_ms.clamp(1_000, 60_000);
        self.interval_off_ms = self.interval_off_ms.clamp(5_000, 600_000);
        self.keep_days = self.keep_days.clamp(1, 90);
        self.snap_every_min = self.snap_every_min.clamp(10, 1_440);
        self.hist_every_min = self.hist_every_min.clamp(30, 1_440);
        // 0 — правило выключено; иначе — в разумных пределах
        let opt = |v: f64, lo: f64, hi: f64| if v <= 0.0 || !v.is_finite() { 0.0 } else { v.clamp(lo, hi) };
        self.watch_sleep_pct = opt(self.watch_sleep_pct, 5.0, 95.0);
        self.watch_drain_pct_h = opt(self.watch_drain_pct_h, 0.5, 50.0);
        self.watch_wakeups_h = opt(self.watch_wakeups_h, 30.0, 100_000.0);
        self.watch_app_ma = opt(self.watch_app_ma, 1.0, 1_000.0);
        self.watch_hot_c = opt(self.watch_hot_c, 30.0, 60.0);
        self.watch_mute.retain(|k| !k.is_empty() && k.len() <= 200);
        self.watch_mute.sort();
        self.watch_mute.dedup();
        self.watch_mute.truncate(100);
        self
    }

    /// Применить частичное обновление из JSON.
    pub fn merged(&self, set: &Value) -> Self {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let (Some(dst), Some(src)) = (v.as_object_mut(), set.as_object()) {
            for (k, x) in src {
                if dst.contains_key(k) {
                    dst.insert(k.clone(), x.clone());
                }
            }
        }
        serde_json::from_value::<Config>(v).unwrap_or_else(|_| self.clone()).clamped()
    }

    pub fn save(&self, dir: &Path) {
        let _ = std::fs::write(dir.join("config.json"), serde_json::to_string_pretty(self).unwrap_or_default());
    }
}

fn file_stats(p: &Path) -> (u64, u64, Option<i64>, Option<i64>) {
    // байты, строки, первое и последнее время (t0 / t)
    let bytes = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let Ok(f) = std::fs::File::open(p) else { return (bytes, 0, None, None) };
    let mut n = 0u64;
    let mut first = None;
    let mut last_line = String::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        if n == 0 {
            first = time_of(&line);
        }
        n += 1;
        last_line = line;
    }
    let last = time_of(&last_line);
    (bytes, n, first, last)
}

fn time_of(line: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(line).ok()?;
    v.get("t1").or_else(|| v.get("t0")).or_else(|| v.get("t")).and_then(|x| x.as_i64())
}

/// Состояние хранилища: размеры, число записей, по дням. Только по запросу.
pub fn storage(d: &Daemon) -> Value {
    let dir = &d.dir;
    let mut total = 0u64;
    let mut days = Vec::new();
    let (mut b_files, mut b_bytes, mut b_rec, mut b_from, mut b_to) = (0u64, 0u64, 0u64, None::<i64>, None::<i64>);
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("buckets"))
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.sort();
    for f in &files {
        let (bytes, n, first, last) = file_stats(f);
        b_files += 1;
        b_bytes += bytes;
        b_rec += n;
        if b_from.is_none() {
            b_from = first;
        }
        if last.is_some() {
            b_to = last;
        }
        days.push(json!({
            "date": f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            "bytes": bytes, "records": n,
        }));
    }
    total += b_bytes;
    let mut groups = vec![json!({"kind": "buckets", "files": b_files, "bytes": b_bytes, "records": b_rec, "from": b_from, "to": b_to})];
    for (kind, name) in [("snaps", "snaps.jsonl"), ("events", "events.jsonl"), ("alerts", "alerts.jsonl"), ("actions", "actions.jsonl")] {
        let p = dir.join(name);
        let (bytes, n, first, last) = file_stats(&p);
        total += bytes;
        groups.push(json!({"kind": kind, "files": if bytes > 0 {1} else {0}, "bytes": bytes, "records": n, "from": first, "to": last}));
    }
    let mut log_bytes = 0;
    for name in ["zhord.log", "zhord.log.1"] {
        log_bytes += std::fs::metadata(dir.join(name)).map(|m| m.len()).unwrap_or(0);
    }
    total += log_bytes;
    groups.push(json!({"kind": "log", "files": 1, "bytes": log_bytes, "records": null, "from": null, "to": null}));
    if let Ok(h) = d.hist.lock() {
        let (files, bytes, mem, from) = h.storage();
        total += bytes;
        groups.push(json!({"kind": "history", "files": files, "bytes": bytes, "records": mem, "from": from, "to": if h.dump_to > 0 { Some(h.dump_to) } else { None }}));
    }
    json!({
        "dir": dir, "total_bytes": total, "groups": groups, "days": days,
        "mem_buckets": d.store.mem.len(), "mem_snaps": d.snaps.len(), "config": d.config,
    })
}

/// Разрешённые места для выгрузки: папки приложения и общие каталоги.
fn export_path_ok(p: &str) -> bool {
    if p.contains("..") || !p.starts_with('/') {
        return false;
    }
    let app = p.contains("/dev.niazlv.zhor/") && (p.starts_with("/data/user/") || p.starts_with("/data/data/") || p.starts_with("/data/user_de/"));
    app || p.starts_with("/data/local/tmp/") || p.starts_with("/sdcard/") || p.starts_with("/storage/emulated/")
}

/// Отдать файл владельцу и SELinux-метке родительской папки, чтобы приложение его прочитало.
fn adopt_parent(path: &Path) {
    use std::os::unix::fs::MetadataExt;
    let Some(parent) = path.parent() else { return };
    let Ok(m) = std::fs::metadata(parent) else { return };
    let Ok(cpath) = std::ffi::CString::new(path.as_os_str().to_string_lossy().as_bytes()) else { return };
    let Ok(cparent) = std::ffi::CString::new(parent.as_os_str().to_string_lossy().as_bytes()) else { return };
    unsafe {
        libc::chown(cpath.as_ptr(), m.uid(), m.gid());
        libc::chmod(cpath.as_ptr(), 0o644);
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let name = c"security.selinux";
            let mut buf = [0u8; 256];
            let n = libc::getxattr(cparent.as_ptr(), name.as_ptr(), buf.as_mut_ptr() as *mut libc::c_void, buf.len());
            if n > 0 {
                libc::setxattr(cpath.as_ptr(), name.as_ptr(), buf.as_ptr() as *const libc::c_void, n as usize, 0);
            }
        }
        let _ = &cparent;
    }
}

/// Выгрузка: report_json | report_text | raw (.jsonl.gz со всеми записями интервала).
pub fn export(d: &Daemon, req: &Value, q: &Query) -> Value {
    let kind = req["kind"].as_str().unwrap_or("report_json");
    let Some(path) = req["path"].as_str() else { return json!({"error": "нужен path"}) };
    if !export_path_ok(path) {
        return json!({"error": format!("путь не разрешён: {path}")});
    }
    let path = PathBuf::from(path);
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let rep = report::build(d, q, None);
    let (from, to) = (rep.from, rep.to);
    let mut records = json!({});
    let res: std::io::Result<()> = (|| {
        match kind {
            "report_text" => std::fs::write(&path, report::render_text(&rep))?,
            "raw" => {
                let f = std::fs::File::create(&path)?;
                let mut gz = flate2::write::GzEncoder::new(std::io::BufWriter::new(f), flate2::Compression::default());
                let buckets = d.store.range(from, to);
                let snaps: Vec<_> = d.snaps.iter().filter(|s| s.t >= from && s.t <= to).collect();
                let events: Vec<_> = d.events.iter().filter(|e| e.t >= from && e.t <= to).collect();
                writeln!(gz, "{}", json!({"type": "meta", "from": from, "to": to, "version": crate::server::VERSION, "profile": d.pp}))?;
                for b in &buckets {
                    writeln!(gz, "{}", json!({"type": "bucket", "data": b}))?;
                }
                for s in &snaps {
                    writeln!(gz, "{}", json!({"type": "snap", "data": s}))?;
                }
                for e in &events {
                    writeln!(gz, "{}", json!({"type": "event", "data": e}))?;
                }
                let hist = d.hist.lock().map(|mut h| h.export_rows(from, to)).unwrap_or_default();
                for e in &hist {
                    writeln!(gz, "{}", json!({"type": "hist", "data": e}))?;
                }
                gz.finish()?.flush()?;
                records = json!({"buckets": buckets.len(), "snaps": snaps.len(), "events": events.len(), "hist": hist.len()});
            }
            _ => std::fs::write(&path, serde_json::to_string_pretty(&rep).unwrap_or_default())?,
        }
        Ok(())
    })();
    if let Err(e) = res {
        return json!({"error": format!("не записалось: {e}")});
    }
    if records.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        records = json!({"apps": rep.apps.len(), "timeline": rep.timeline.len(), "events": rep.events.len()});
    }
    adopt_parent(&path);
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    json!({"ok": true, "path": path, "kind": kind, "bytes": bytes, "records": records, "from": from, "to": to})
}

/// Очистка: всё или записи до `before`. Вызывать только после подтверждения пользователя.
pub fn prune(d: &mut Daemon, req: &Value) -> Value {
    let all = req["all"].as_bool().unwrap_or(false);
    let before = req["before"].as_i64();
    if !all && before.is_none() {
        return json!({"error": "нужно all:true или before:<мс>"});
    }
    let (n, freed) = d.prune(if all { None } else { before });
    json!({"ok": true, "removed_files": n, "freed_bytes": freed})
}
