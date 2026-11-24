//! Корзины по времени (≈1 мин, однородные по экрану/зарядке), запись на диск, выборка.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThrRec {
    pub n: String,
    /// CPU-время, мс
    pub t: u64,
    /// энергия, мА·с
    pub e: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcRec {
    pub uid: u32,
    pub n: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub k: bool,
    pub t: u64,
    pub e: f64,
    #[serde(default, skip_serializing_if = "is_zero32")]
    pub st: u32,
    /// изолированный процесс (рендер WebView и т.п.): uid приложения-хозяина
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<u32>,
    pub th: Vec<ThrRec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WsRec {
    pub n: String,
    pub ac: u64,
    pub wk: u64,
    pub ms: u64,
    pub pv: u64,
}

/// (0.8) Сеть приложения за отрезок между замерами `dumpsys netstats`: принято/отправлено пакетов
/// и байт. Замер — раз в 5 мин и на смене экрана/зарядки, разность лежит в корзине, где замер снят
/// (а относится к её состоянию: экран, зарядка).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetRec {
    pub u: u32,
    pub rp: u64,
    pub rb: u64,
    #[serde(default)]
    pub tp: u64,
    #[serde(default)]
    pub tb: u64,
}

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero32(v: &u32) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bucket {
    /// начало/конец, настенное время мс
    pub t0: i64,
    pub t1: i64,
    /// всего / бодрствовал / спал, мс
    pub span: u64,
    pub awake: u64,
    pub susp: u64,
    pub scr: bool,
    pub plug: bool,
    /// средняя яркость при включённом экране 0..1
    pub bright: f64,
    /// по кулоновскому счётчику: израсходовано мкА·ч (>0 — разряд)
    pub cc_uah: i64,
    pub cc_ok: bool,
    /// интеграл тока по замерам, мА·с
    pub cur_mas: f64,
    pub lvl0: u8,
    pub lvl1: u8,
    pub mv: f64,
    pub temp: f64,
    /// модель: потоки CPU / CPU в простое (бодрствование) / экран, мА·с
    pub cpu_mas: f64,
    pub base_mas: f64,
    pub scr_mas: f64,
    pub procs: Vec<ProcRec>,
    pub ws: Vec<WsRec>,
    pub sok: u64,
    pub sfail: u64,
    pub rr: Vec<(String, u64)>,
    /// [кластер][частота] тики CPU (все потоки)
    pub freq: Vec<Vec<u64>>,
    /// GPU занят, доля 0..1 (по kgsl), среднее
    pub gpu: f64,
    pub ticks: u32,
    /// (0.8) сеть по приложениям (см. NetRec); пусто — замера в этой корзине не было
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub net: Vec<NetRec>,
}

pub const MAX_THREADS_PER_PROC: usize = 20;

impl Bucket {
    /// Сжатие перед записью: потоки — топ-N + «прочие».
    pub fn compact(&mut self) {
        for p in &mut self.procs {
            if p.th.len() > MAX_THREADS_PER_PROC {
                p.th.sort_by(|a, b| b.e.partial_cmp(&a.e).unwrap_or(std::cmp::Ordering::Equal));
                let rest: Vec<ThrRec> = p.th.split_off(MAX_THREADS_PER_PROC - 1);
                let mut o = ThrRec { n: "<прочие потоки>".into(), t: 0, e: 0.0 };
                for r in rest {
                    o.t += r.t;
                    o.e += r.e;
                }
                p.th.push(o);
            }
            for t in &mut p.th {
                t.e = (t.e * 1000.0).round() / 1000.0;
            }
            p.e = (p.e * 1000.0).round() / 1000.0;
        }
        self.procs.sort_by(|a, b| b.e.partial_cmp(&a.e).unwrap_or(std::cmp::Ordering::Equal));
    }
}

/// Аккумулятор открытой корзины: процессы по ключу (uid, имя).
#[derive(Default)]
pub struct OpenBucket {
    pub b: Bucket,
    pub procs: HashMap<(u32, String, Option<u32>), (ProcRec, HashMap<String, ThrRec>)>,
    pub ws: HashMap<String, WsRec>,
    pub rr: HashMap<String, u64>,
    pub bright_ms: f64,
    pub on_ms: u64,
    pub gpu_sum: f64,
    pub gpu_n: u32,
    pub mv_sum: f64,
}

impl OpenBucket {
    pub fn finish(mut self) -> Bucket {
        let mut b = std::mem::take(&mut self.b);
        b.procs = self
            .procs
            .into_values()
            .map(|(mut p, th)| {
                p.th = th.into_values().collect();
                p
            })
            .collect();
        b.ws = self.ws.into_values().collect();
        b.ws.sort_by(|a, b| b.ms.cmp(&a.ms));
        b.rr = self.rr.into_iter().collect();
        b.bright = if self.on_ms > 0 { self.bright_ms / self.on_ms as f64 } else { 0.0 };
        b.gpu = if self.gpu_n > 0 { self.gpu_sum / self.gpu_n as f64 } else { 0.0 };
        b.mv = if b.ticks > 0 { self.mv_sum / b.ticks as f64 } else { 0.0 };
        b.compact();
        b
    }
}

pub struct Store {
    pub dir: PathBuf,
    pub mem: VecDeque<Bucket>,
    pub keep_mem_ms: i64,
    pub keep_disk_days: i64,
    last_day: String,
}

pub(crate) fn day_of(t: i64) -> String {
    // локальная дата телефона (смещение часового пояса из libc)
    let off = local_offset_ms(t);
    let days = (t + off).div_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

pub(crate) fn local_offset_ms(t: i64) -> i64 {
    let secs = (t / 1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&secs, &mut tm);
    }
    tm.tm_gmtoff as i64 * 1000
}

/// Алгоритм Говарда Хиннанта: дни с эпохи → (год, месяц, день).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl Store {
    pub fn open(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(dir.join("buckets"));
        let mut s = Store { dir, mem: VecDeque::new(), keep_mem_ms: 3 * 86_400_000, keep_disk_days: 10, last_day: String::new() };
        s.load_recent();
        s.prune_disk();
        s
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.dir.join("buckets"))
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "jsonl").unwrap_or(false)).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    fn load_recent(&mut self) {
        let now = crate::util::real_ms();
        let from = now - self.keep_mem_ms;
        let from_day = day_of(from);
        for f in self.files() {
            let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if name < from_day {
                continue;
            }
            if let Ok(fh) = File::open(&f) {
                for line in BufReader::new(fh).lines().map_while(Result::ok) {
                    if let Ok(b) = serde_json::from_str::<Bucket>(&line) {
                        if b.t1 >= from {
                            self.mem.push_back(b);
                        }
                    }
                }
            }
        }
        self.mem.make_contiguous().sort_by_key(|b| b.t0);
    }

    pub fn prune_disk(&self) {
        let cut = day_of(crate::util::real_ms() - self.keep_disk_days * 86_400_000);
        for f in self.files() {
            let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if name < cut {
                let _ = std::fs::remove_file(f);
            }
        }
    }

    pub fn push(&mut self, b: Bucket) {
        let day = day_of(b.t0);
        if day != self.last_day {
            self.last_day = day.clone();
            self.prune_disk();
        }
        let path = self.dir.join("buckets").join(format!("{day}.jsonl"));
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
            if let Ok(s) = serde_json::to_string(&b) {
                let _ = writeln!(f, "{s}");
            }
        }
        self.mem.push_back(b);
        let cut = crate::util::real_ms() - self.keep_mem_ms;
        while self.mem.front().map(|b| b.t1 < cut).unwrap_or(false) {
            self.mem.pop_front();
        }
    }

    /// Корзины, пересекающие [from, to]. Старые — с диска.
    pub fn range(&self, from: i64, to: i64) -> Vec<Bucket> {
        let mem_from = self.mem.front().map(|b| b.t0).unwrap_or(i64::MAX);
        let mut out = Vec::new();
        if from < mem_from {
            let fd = day_of(from);
            let md = day_of(mem_from.min(to));
            for f in self.files() {
                let name = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if name < fd || name > md {
                    continue;
                }
                if let Ok(fh) = File::open(&f) {
                    for line in BufReader::new(fh).lines().map_while(Result::ok) {
                        if let Ok(b) = serde_json::from_str::<Bucket>(&line) {
                            if b.t1 > from && b.t0 < to && b.t0 < mem_from {
                                out.push(b);
                            }
                        }
                    }
                }
            }
        }
        out.extend(self.mem.iter().filter(|b| b.t1 > from && b.t0 < to).cloned());
        out.sort_by_key(|b| b.t0);
        out
    }

    /// Удалить записи: все (None) или раньше `before`. → (удалено файлов, освобождено байт)
    pub fn prune(&mut self, before: Option<i64>) -> (u64, u64) {
        let (mut n, mut freed) = (0u64, 0u64);
        for f in self.files() {
            let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
            let day = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match before {
                None => {
                    if std::fs::remove_file(&f).is_ok() {
                        n += 1;
                        freed += size;
                    }
                }
                Some(b) => {
                    let bd = day_of(b);
                    if day < bd {
                        if std::fs::remove_file(&f).is_ok() {
                            n += 1;
                            freed += size;
                        }
                    } else if day == bd {
                        let kept: Vec<String> = File::open(&f)
                            .map(|fh| {
                                BufReader::new(fh)
                                    .lines()
                                    .map_while(Result::ok)
                                    .filter(|l| serde_json::from_str::<Bucket>(l).map(|x| x.t1 >= b).unwrap_or(false))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let mut text = kept.join("\n");
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        let _ = std::fs::write(&f, &text);
                        freed += size.saturating_sub(text.len() as u64);
                    }
                }
            }
        }
        match before {
            None => self.mem.clear(),
            Some(b) => self.mem.retain(|x| x.t1 >= b),
        }
        (n, freed)
    }

    pub fn oldest(&self) -> Option<i64> {
        if let Some(f) = self.files().first() {
            if let Ok(fh) = File::open(f) {
                if let Some(Ok(line)) = BufReader::new(fh).lines().next() {
                    if let Ok(b) = serde_json::from_str::<Bucket>(&line) {
                        return Some(b.t0);
                    }
                }
            }
        }
        self.mem.front().map(|b| b.t0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(1_795_384_601_413 / 86_400_000), (2026, 11, 22));
    }
}
