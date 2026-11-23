//! Сканер процессов и потоков: дельты CPU-времени и энергия по частотам.
//!
//! Источник точности — `/proc/<pid>/task/<tid>/time_in_state` (CONFIG_CPU_FREQ_TIMES):
//! для каждого потока ядро считает время на каждой частоте каждого кластера.
//! Умножаем на мА ядра на этой частоте из профиля производителя → мА·с потока.
//! Чтобы не жечь батарею самим профилировщиком, потоки читаются только у процессов,
//! у которых CPU-время изменилось с прошлого шага.

use crate::profile::PowerProfile;
use crate::util;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;

/// Имена, которые процесс носит до специализации (зигота, usap): пере-читать позже.
fn transient_name(n: &str) -> bool {
    matches!(n, "zygote" | "zygote64" | "<pre-initialized>" | "usap32" | "usap64" | "app_process" | "app_process64" | "webview_zygote")
        || n.is_empty()
}

struct ThrState {
    start: u64,
    ticks: u64,
    tis: Vec<u64>,
    name: Arc<str>,
}

struct ProcState {
    start: u64,
    uid: u32,
    name: Arc<str>,
    kernel: bool,
    ticks: u64,
    threads: HashMap<u32, ThrState>,
    gen: u64,
    /// тики CPU, ещё не разложенные по потокам (мелкая активность копится)
    pending: u64,
    /// шаг, когда потоки читались в последний раз
    last_full: u64,
    /// сколько шагов подряд процесс не тратил CPU
    idle: u32,
    /// средняя цена тика процесса при последнем разборе, мА·с за тик
    avg_mas: f64,
}

#[derive(Debug, Clone)]
pub struct TickThread {
    pub name: Arc<str>,
    pub main: bool,
    pub ticks: u64,
    pub mas: f64,
}

#[derive(Debug, Clone)]
pub struct TickProc {
    pub pid: u32,
    pub uid: u32,
    pub name: Arc<str>,
    pub kernel: bool,
    pub started: bool,
    pub ticks: u64,
    pub mas: f64,
    pub threads: Vec<TickThread>,
}

#[derive(Debug, Default, Clone)]
pub struct TickCpu {
    pub procs: Vec<TickProc>,
    /// Сумма дельт time_in_state по всем потокам: [кластер][частота] в тиках.
    pub freq_ticks: Vec<Vec<u64>>,
    pub scanned_procs: u32,
    pub scanned_threads: u32,
    pub tis_reads: u32,
    pub skipped_idle: u32,
}

pub struct ProcScanner {
    procs: HashMap<u32, ProcState>,
    names: HashMap<String, Arc<str>>,
    buf: Vec<u8>,
    path: String,
    gen: u64,
    first: bool,
    last_boot_ms: u64,
    tick_ms: f64,
    /// cpu → кластер
    cpu_cluster: Vec<usize>,
    /// по кластерам: частота → индекс
    freq_idx: Vec<HashMap<u64, usize>>,
    offsets: Vec<usize>,
    flat_len: usize,
    ma_flat: Vec<f64>,
    flat_cluster: Vec<usize>,
    fallback_ma: Vec<f64>,
    pub tis_available: bool,
    /// потоки процесса читаются, когда у него накопилось столько тиков…
    pub min_ticks: u64,
    /// …или прошло столько шагов
    pub max_defer: u64,
    /// простаивающие процессы читаются раз в столько шагов (1 — каждый шаг)
    pub cold_every: u64,
}

impl ProcScanner {
    pub fn new(pp: &PowerProfile) -> Self {
        let tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        let tick_ms = if tck > 0 { 1000.0 / tck as f64 } else { 10.0 };
        let maxcpu = pp.clusters.iter().flat_map(|c| c.cpus.iter().copied()).max().unwrap_or(0) as usize;
        let mut cpu_cluster = vec![0usize; maxcpu + 1];
        let mut freq_idx = Vec::new();
        let mut offsets = Vec::new();
        let mut ma_flat = Vec::new();
        let mut fallback_ma = Vec::new();
        let mut flat_cluster = Vec::new();
        for (ci, c) in pp.clusters.iter().enumerate() {
            for &cpu in &c.cpus {
                cpu_cluster[cpu as usize] = ci;
            }
            offsets.push(ma_flat.len());
            freq_idx.push(c.freqs_khz.iter().enumerate().map(|(i, &f)| (f, i)).collect());
            ma_flat.extend_from_slice(&c.ma);
            flat_cluster.extend(std::iter::repeat(ci).take(c.ma.len()));
            fallback_ma.push(pp.mid_ma(ci));
        }
        if fallback_ma.is_empty() {
            fallback_ma.push(150.0);
        }
        let tis_available = std::path::Path::new("/proc/self/time_in_state").exists();
        ProcScanner {
            procs: HashMap::new(),
            names: HashMap::new(),
            buf: Vec::with_capacity(8192),
            path: String::with_capacity(64),
            gen: 0,
            first: true,
            last_boot_ms: 0,
            tick_ms,
            cpu_cluster,
            freq_idx,
            offsets,
            flat_len: ma_flat.len(),
            ma_flat,
            flat_cluster,
            fallback_ma,
            tis_available,
            min_ticks: 3,
            max_defer: 12,
            cold_every: 1,
        }
    }

    pub fn tick_ms(&self) -> f64 {
        self.tick_ms
    }

    fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(a) = self.names.get(s) {
            return a.clone();
        }
        let a: Arc<str> = Arc::from(s);
        self.names.insert(s.to_string(), a.clone());
        a
    }

    /// Разбор /proc/.../stat: (comm, ppid, ticks=utime+stime, starttime, processor, num_threads).
    fn parse_stat(buf: &[u8]) -> Option<(&[u8], u32, u64, u64, u32, u32)> {
        let lp = buf.iter().position(|&c| c == b'(')?;
        let rp = buf.iter().rposition(|&c| c == b')')?;
        let comm = &buf[lp + 1..rp];
        let rest = buf.get(rp + 2..)?;
        // поля после ')': 0=state 1=ppid ... 11=utime 12=stime 17=num_threads 19=starttime 36=processor
        let mut f: [&[u8]; 37] = [&[]; 37];
        let mut n = 0;
        for part in rest.split(|&c| c == b' ') {
            if n >= 37 {
                break;
            }
            f[n] = part;
            n += 1;
        }
        if n < 20 {
            return None;
        }
        let ppid = util::parse_u64(f[1]) as u32;
        let ticks = util::parse_u64(f[11]) + util::parse_u64(f[12]);
        let nthr = util::parse_u64(f[17]) as u32;
        let start = util::parse_u64(f[19]);
        let cpu = if n > 36 { util::parse_u64(f[36]) as u32 } else { 0 };
        Some((comm, ppid, ticks, start, cpu, nthr))
    }

    /// Разбор time_in_state потока в плоский вектор [кластер][частота].
    fn parse_tis(&self, buf: &[u8], out: &mut Vec<u64>) -> bool {
        out.clear();
        out.resize(self.flat_len, 0);
        let mut cluster: Option<usize> = None;
        let mut any = false;
        for line in buf.split(|&c| c == b'\n') {
            if line.is_empty() {
                continue;
            }
            if line.starts_with(b"cpu") {
                let n = util::parse_u64(&line[3..]) as usize;
                cluster = self.cpu_cluster.get(n).copied();
                continue;
            }
            let Some(ci) = cluster else { continue };
            let sp = match line.iter().position(|&c| c == b' ') {
                Some(p) => p,
                None => continue,
            };
            let freq = util::parse_u64(&line[..sp]);
            let t = util::parse_u64(&line[sp + 1..]);
            if let Some(&fi) = self.freq_idx[ci].get(&freq) {
                out[self.offsets[ci] + fi] = t;
                any = true;
            }
        }
        any
    }

    fn read_cmdline_name(&mut self, pid: u32, comm: &[u8]) -> String {
        self.path.clear();
        let _ = write!(self.path, "/proc/{pid}/cmdline");
        let mut b = std::mem::take(&mut self.buf);
        let ok = util::read_into(&self.path, &mut b);
        let mut name = String::new();
        if ok && !b.is_empty() {
            let first = b.split(|&c| c == 0).next().unwrap_or(&[]);
            let s = String::from_utf8_lossy(first);
            let s = s.trim();
            // путь к бинарнику → базовое имя
            name = if s.starts_with('/') { s.rsplit('/').next().unwrap_or(s).to_string() } else { s.to_string() };
            if let Some(stripped) = name.split_whitespace().next() {
                name = stripped.to_string();
            }
        }
        self.buf = b;
        if name.is_empty() {
            name = String::from_utf8_lossy(comm).into_owned();
        }
        name
    }

    fn proc_uid(pid: u32) -> u32 {
        std::fs::metadata(format!("/proc/{pid}")).map(|m| m.uid()).unwrap_or(0)
    }

    /// Один шаг сканирования. `boot_ms` — CLOCK_BOOTTIME сейчас.
    pub fn scan(&mut self, boot_ms: u64) -> TickCpu {
        self.gen += 1;
        let gen = self.gen;
        let first = self.first;
        let since_ms = self.last_boot_ms;
        let mut out = TickCpu {
            freq_ticks: self
                .offsets
                .iter()
                .enumerate()
                .map(|(ci, _)| vec![0u64; self.freq_idx[ci].len()])
                .collect(),
            ..Default::default()
        };
        let Ok(rd) = std::fs::read_dir("/proc") else { return out };
        let pids: Vec<u32> = rd
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()))
            .collect();
        let mut tis_cur: Vec<u64> = Vec::with_capacity(self.flat_len);
        let mut buf = std::mem::take(&mut self.buf);
        for pid in pids {
            // Простаивающие процессы читаем раз в cold_every шагов: их CPU-время всё равно
            // доучтётся по разнице счётчика; так в каждом шаге читается лишь ~треть /proc.
            if self.cold_every > 1 {
                if let Some(p) = self.procs.get_mut(&pid) {
                    if p.idle >= 2 && (gen + pid as u64) % self.cold_every != 0 {
                        p.gen = gen;
                        out.skipped_idle += 1;
                        continue;
                    }
                }
            }
            self.path.clear();
            let _ = write!(self.path, "/proc/{pid}/stat");
            if !util::read_into(&self.path, &mut buf) {
                continue;
            }
            let Some((comm, ppid, ticks, start, _cpu, nthr)) = Self::parse_stat(&buf) else { continue };
            out.scanned_procs += 1;
            let comm = comm.to_vec();
            let known = self.procs.get(&pid).map(|p| p.start == start).unwrap_or(false);
            let start_ms = (start as f64 * self.tick_ms) as u64;
            // Новый процесс, родившийся после прошлого шага, — считаем всю его жизнь.
            let born_since = !first && start_ms + 50 >= since_ms;
            if !known {
                let kernel = ppid == 2 || pid == 2;
                let (uid, name) = if kernel {
                    (0, "kernel".to_string())
                } else {
                    (Self::proc_uid(pid), self.read_cmdline_name(pid, &comm))
                };
                let name = self.intern(&name);
                self.procs.insert(
                    pid,
                    ProcState { start, uid, name, kernel, ticks: if born_since { 0 } else { ticks }, threads: HashMap::new(), gen, pending: 0, last_full: gen, idle: 0, avg_mas: 0.0 },
                );
                if !born_since {
                    // база: потоки этого процесса запоминаем без учёта
                    self.baseline_threads(pid, &mut buf, &mut tis_cur);
                    continue;
                }
            }
            let (dticks, kernel, refresh) = {
                let p = self.procs.get_mut(&pid).unwrap();
                p.gen = gen;
                let d = ticks.saturating_sub(p.ticks);
                p.ticks = ticks;
                p.idle = if d == 0 { p.idle.saturating_add(1) } else { 0 };
                // Мелочь (1–2 тика) копим и раскладываем по потокам позже: чтение stat всех
                // потоков процесса — главная цена прохода. Потоки всё равно посчитают её по своей разнице.
                let total = d + p.pending;
                // срок разбора у каждого процесса свой (по pid), чтобы не разбирать все разом
                let due = self.max_defer <= 1 || gen - p.last_full >= self.max_defer || (gen + pid as u64) % self.max_defer == 0 && gen - p.last_full >= self.max_defer / 2;
                if known && total > 0 && total < self.min_ticks && !due {
                    p.pending = total;
                    (0, p.kernel, false)
                } else {
                    p.pending = 0;
                    if total > 0 || !known {
                        p.last_full = gen;
                    }
                    (total, p.kernel, transient_name(&p.name))
                }
            };
            if dticks == 0 && known {
                continue;
            }
            if refresh && !kernel {
                let uid = Self::proc_uid(pid);
                let name = self.read_cmdline_name(pid, &comm);
                let name = self.intern(&name);
                let p = self.procs.get_mut(&pid).unwrap();
                p.uid = uid;
                p.name = name;
            }
            let tp = self.scan_threads(pid, nthr, &comm, dticks, !known, &mut buf, &mut tis_cur, &mut out);
            out.procs.push(tp);
        }
        self.buf = buf;
        // Процессы, умершие с неразобранным временем: учитываем его по последней средней цене.
        let fallback = self.fallback_ma[0] * self.tick_ms / 1000.0;
        let dead_name = self.intern("<завершился до разбора>");
        for (&pid, p) in self.procs.iter() {
            if p.gen != gen && p.pending > 0 {
                let avg = if p.avg_mas > 0.0 { p.avg_mas } else { fallback };
                let mas = p.pending as f64 * avg;
                out.procs.push(TickProc {
                    pid,
                    uid: p.uid,
                    name: p.name.clone(),
                    kernel: p.kernel,
                    started: false,
                    ticks: p.pending,
                    mas,
                    threads: vec![TickThread { name: dead_name.clone(), main: false, ticks: p.pending, mas }],
                });
            }
        }
        self.procs.retain(|_, p| p.gen == gen);
        self.first = false;
        self.last_boot_ms = boot_ms;
        out
    }

    fn baseline_threads(&mut self, pid: u32, buf: &mut Vec<u8>, tis: &mut Vec<u64>) {
        let tids = list_tids(pid);
        let mut threads = HashMap::new();
        for tid in tids {
            self.path.clear();
            let _ = write!(self.path, "/proc/{pid}/task/{tid}/stat");
            if !util::read_into(&self.path, buf) {
                continue;
            }
            let Some((comm, _, ticks, start, _, _)) = Self::parse_stat(buf) else { continue };
            let kernel = self.procs.get(&pid).map(|p| p.kernel).unwrap_or(false);
            let name = thread_key(pid, tid, comm, kernel);
            let name = self.intern(&name);
            // time_in_state читаем лениво: при первой активности потока (дёшево на старте)
            let _ = &tis;
            threads.insert(tid, ThrState { start, ticks, tis: Vec::new(), name });
        }
        if let Some(p) = self.procs.get_mut(&pid) {
            p.threads = threads;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scan_threads(
        &mut self,
        pid: u32,
        _nthr: u32,
        _comm: &[u8],
        dticks: u64,
        new_proc: bool,
        buf: &mut Vec<u8>,
        tis: &mut Vec<u64>,
        out: &mut TickCpu,
    ) -> TickProc {
        let tids = list_tids(pid);
        let kernel = self.procs.get(&pid).map(|p| p.kernel).unwrap_or(false);
        let mut prev = std::mem::take(&mut self.procs.get_mut(&pid).unwrap().threads);
        let mut next: HashMap<u32, ThrState> = HashMap::with_capacity(tids.len());
        let mut res_threads: Vec<TickThread> = Vec::new();
        let mut sum_thr_ticks = 0u64;
        let mut sum_mas = 0.0f64;
        let mut tis_ticks_total = 0u64;
        for tid in tids {
            self.path.clear();
            let _ = write!(self.path, "/proc/{pid}/task/{tid}/stat");
            if !util::read_into(&self.path, buf) {
                continue;
            }
            out.scanned_threads += 1;
            let Some((comm, _, ticks, start, cpu, _)) = Self::parse_stat(buf) else { continue };
            let old = prev.remove(&tid).filter(|t| t.start == start);
            let is_new = old.is_none();
            let (old_ticks, old_tis, name) = match old {
                Some(t) => (t.ticks, t.tis, t.name),
                None => {
                    let k = thread_key(pid, tid, comm, kernel);
                    (0, Vec::new(), self.intern(&k))
                }
            };
            let dt = ticks.saturating_sub(old_ticks);
            // Имя потока может смениться (setName) — проверяем только у работавших потоков
            // и без аллокаций, если сырое имя не изменилось.
            let name = if !is_new && dt > 0 && (tid != pid || kernel) && !same_comm(&name, comm) {
                let k = thread_key(pid, tid, comm, kernel);
                if *name != *k { self.intern(&k) } else { name }
            } else {
                name
            };
            let mut cur_tis = old_tis;
            let mut mas = 0.0;
            if dt > 0 || is_new {
                let mut got = false;
                if self.tis_available {
                    self.path.clear();
                    let _ = write!(self.path, "/proc/{pid}/task/{tid}/time_in_state");
                    out.tis_reads += 1;
                    let ok = util::read_into(&self.path, buf) && self.parse_tis(buf, tis);
                    if ok && !is_new && cur_tis.is_empty() {
                        // первая активность потока из базы: запоминаем базу, этот шаг — оценкой
                        cur_tis.clone_from(tis);
                    } else if ok {
                        got = true;
                        let mut dsum = 0u64;
                        for i in 0..self.flat_len {
                            let o = cur_tis.get(i).copied().unwrap_or(0);
                            let d = tis[i].saturating_sub(o);
                            if d > 0 {
                                dsum += d;
                                mas += d as f64 * self.tick_ms / 1000.0 * self.ma_flat[i];
                                // глобальная раскладка по кластерам/частотам
                                let ci = self.flat_cluster[i];
                                out.freq_ticks[ci][i - self.offsets[ci]] += d;
                            }
                        }
                        tis_ticks_total += dsum;
                        // Время, не попавшее в time_in_state (округление) — по средней цене потока.
                        if dt > dsum {
                            let avg = if dsum > 0 { mas / (dsum as f64) } else { self.fallback_for_cpu(cpu) * self.tick_ms / 1000.0 };
                            mas += (dt - dsum) as f64 * avg;
                        }
                        cur_tis.clone_from(tis);
                    }
                }
                if !got {
                    mas = dt as f64 * self.tick_ms / 1000.0 * self.fallback_for_cpu(cpu);
                }
            }
            if dt > 0 {
                sum_thr_ticks += dt;
                sum_mas += mas;
                res_threads.push(TickThread { name: name.clone(), main: tid == pid && !kernel, ticks: dt, mas });
            }
            next.insert(tid, ThrState { start, ticks, tis: cur_tis, name });
        }
        let p = self.procs.get_mut(&pid).unwrap();
        p.threads = next;
        // Потоки, завершившиеся между шагами: их время есть в процессе, но не в живых потоках.
        let mut total_ticks = dticks;
        if new_proc {
            total_ticks = p.ticks.max(sum_thr_ticks);
        }
        if total_ticks > sum_thr_ticks {
            let resid = total_ticks - sum_thr_ticks;
            let avg = if sum_thr_ticks > 0 {
                sum_mas / sum_thr_ticks as f64
            } else {
                self.fallback_ma[0] * self.tick_ms / 1000.0
            };
            let name = self.intern("<завершённые потоки>");
            res_threads.push(TickThread { name, main: false, ticks: resid, mas: resid as f64 * avg });
            sum_mas += resid as f64 * avg;
        }
        let _ = tis_ticks_total;
        let all = total_ticks.max(sum_thr_ticks);
        let p = self.procs.get_mut(&pid).unwrap();
        if all > 0 {
            p.avg_mas = sum_mas / all as f64;
        }
        TickProc {
            pid,
            uid: p.uid,
            name: p.name.clone(),
            kernel: p.kernel,
            started: new_proc,
            ticks: total_ticks.max(sum_thr_ticks),
            mas: sum_mas,
            threads: res_threads,
        }
    }

    fn fallback_for_cpu(&self, cpu: u32) -> f64 {
        let ci = self.cpu_cluster.get(cpu as usize).copied().unwrap_or(0);
        self.fallback_ma.get(ci).copied().unwrap_or(150.0)
    }

    /// pid → имя процесса (для привязки нативных wakelock).
    pub fn pid_names(&self) -> HashMap<u32, (u32, Arc<str>)> {
        self.procs.iter().map(|(&pid, p)| (pid, (p.uid, p.name.clone()))).collect()
    }
}

fn list_tids(pid: u32) -> Vec<u32> {
    match std::fs::read_dir(format!("/proc/{pid}/task")) {
        Ok(rd) => rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_str().and_then(|s| s.parse().ok())).collect(),
        Err(_) => Vec::new(),
    }
}

/// Совпадает ли сохранённый ключ потока с сырым comm (без аллокаций): ключ — это comm с
/// цифрами, свёрнутыми в «#».
fn same_comm(key: &str, comm: &[u8]) -> bool {
    let k = key.as_bytes();
    let (mut i, mut j) = (0usize, 0usize);
    while j < comm.len() {
        if comm[j].is_ascii_digit() {
            if i >= k.len() || k[i] != b'#' {
                return false;
            }
            i += 1;
            while j < comm.len() && comm[j].is_ascii_digit() {
                j += 1;
            }
        } else {
            if i >= k.len() || k[i] != comm[j] {
                return false;
            }
            i += 1;
            j += 1;
        }
    }
    i == k.len()
}

/// Ключ потока: главный поток — «<main>», остальные — имя с цифрами, заменёнными на «#».
fn thread_key(pid: u32, tid: u32, comm: &[u8], kernel: bool) -> String {
    if pid == tid && !kernel {
        return "<main>".to_string();
    }
    util::normalize_name(&String::from_utf8_lossy(comm))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comm_match() {
        assert!(same_comm("binder:#_#", b"binder:1234_5"));
        assert!(same_comm("RenderThread", b"RenderThread"));
        assert!(!same_comm("RenderThread", b"RenderThreaX"));
        assert!(!same_comm("binder:#_#", b"binder:12"));
    }

    #[test]
    fn stat_parse() {
        let s = b"1234 (binder:12 34_5) S 1 1234 0 0 -1 4194624 100 0 0 0 250 130 0 0 20 0 42 0 9876 0 0 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 6 0 0 0 0 0";
        let (comm, ppid, ticks, start, cpu, nthr) = ProcScanner::parse_stat(s).unwrap();
        assert_eq!(comm, b"binder:12 34_5");
        assert_eq!(ppid, 1);
        assert_eq!(ticks, 380);
        assert_eq!(nthr, 42);
        assert_eq!(start, 9876);
        assert_eq!(cpu, 6);
    }
}
