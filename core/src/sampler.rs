//! Демон: шаг сбора, корзины, события, системные снимки.

use crate::procfs::{ProcScanner, TickCpu};
use crate::profile::PowerProfile;
use crate::store::{Bucket, NetRec, OpenBucket, ProcRec, Store, ThrRec, WsRec};
use crate::sys::{self, SysSnap};
use crate::sysfs::{self, Battery, BatteryReader, Screen, ScreenReader, SuspendReader, WakeupSources};
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub t: i64,
    /// screen_on / screen_off / plug / unplug / start
    pub kind: String,
    pub level: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveTick {
    pub t: i64,
    pub dt_ms: u64,
    pub draw_ma: f64,
    pub voltage_mv: f64,
    pub level: u8,
    pub screen: bool,
    pub plugged: bool,
    pub model_ma: f64,
    pub cpu_ma: f64,
    pub top: Vec<(u32, String, f64)>,
}

#[derive(Clone, Copy, Default)]
struct Clocks {
    boot: u64,
    mono: u64,
    real: i64,
}

impl Clocks {
    fn now() -> Self {
        Clocks { boot: util::boot_ms(), mono: util::mono_ms(), real: util::real_ms() }
    }
}

pub struct Daemon {
    pub dir: PathBuf,
    pub pp: PowerProfile,
    pub started: i64,
    scanner: ProcScanner,
    batt: BatteryReader,
    screen_r: ScreenReader,
    ws: WakeupSources,
    susp: SuspendReader,
    open: Option<OpenBucket>,
    last: Option<(Clocks, Battery, Screen)>,
    gpu_last: Option<(u64, u64)>,
    tick_no: u64,
    pub store: Store,
    pub events: Vec<Event>,
    pub snaps: VecDeque<SysSnap>,
    pub live: VecDeque<LiveTick>,
    pub pkgs: BTreeMap<u32, Vec<String>>,
    pkgs_at: i64,
    last_snap_real: i64,
    pub want_snap: bool,
    /// когда последний раз читали полную историю Android
    pub last_hist_real: i64,
    /// подробная история Android включена в этой загрузке (Android сбрасывает её при перезагрузке)
    pub full_hist_ok: bool,
    pub last_scan_cost_us: u64,
    pub design_mah: Option<f64>,
    pub full_mah: Option<f64>,
    /// pid изолированного процесса → (его uid, uid хозяина)
    hosts: HashMap<u32, (u32, u32)>,
    hosts_at: u64,
    pub config: crate::data::Config,
    /// до какого момента (mono) кто-то смотрит «Сейчас» — шагаем чаще
    pub live_until: u64,
    /// цена последнего шага по частям
    pub cost: StepCost,
    /// сколько CPU потрачено на шаги с запуска, мкс
    pub cpu_total_us: u64,
    /// история событий Android (свой замок: разбор и запись идут в потоке снимка)
    pub hist: std::sync::Arc<std::sync::Mutex<crate::history::HistStore>>,
    /// (0.6) сторож: тревоги по свежим корзинам
    pub watch: crate::watch::Watch,
    /// (0.8) последний замер сети по uid и когда он был (настенное время)
    net_last: Option<BTreeMap<u32, sys::netstat::Cnt>>,
    net_at: i64,
    /// (0.8) пакеты с VPN-службой: их трафик в долях «кто будит по сети» не считается
    pub vpn_pkgs: BTreeSet<String>,
    /// (0.8) режимы и метки приложений, их состояние и защищённые пакеты
    pub modes: crate::modes::Modes,
    pub mode_rt: crate::modes::Runtime,
    pub protect: crate::modes::Protect,
    /// (0.8.3) мигание порта USB-C по журналу ядра
    pub usb: crate::usbport::UsbPort,
}

/// Как часто снимать сеть по приложениям (плюс каждая смена экрана и зарядки).
const NET_EVERY_MS: i64 = 5 * 60_000;

fn merge_net(dst: &mut Vec<NetRec>, src: Vec<NetRec>) {
    for r in src {
        match dst.iter_mut().find(|x| x.u == r.u) {
            Some(x) => {
                x.rp += r.rp;
                x.rb += r.rb;
                x.tp += r.tp;
                x.tb += r.tb;
            }
            None => dst.push(r),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct StepCost {
    /// процессорное время шага (настоящая цена), мкс
    pub cpu_us: u64,
    /// CPU: батарея+экран / сканер процессов / прочее, мкс
    pub cpu_sensors_us: u64,
    pub cpu_scan_us: u64,
    /// настенное время шага (с вытеснением — демон работает с низким приоритетом), мкс
    pub total_us: u64,
    pub procs_us: u64,
    pub wakeup_sources_us: u64,
    pub hosts_us: u64,
    /// последний замер сети по приложениям (раз в 5 мин), мкс
    pub net_us: u64,
    pub procs: u32,
    pub threads: u32,
    pub tis_reads: u32,
    pub skipped_idle: u32,
}

impl Daemon {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        let pp = crate::profile::load_device();
        let scanner = ProcScanner::new(&pp);
        let batt = BatteryReader::new();
        let design_mah = batt.design_capacity_mah();
        let full_mah = batt.full_capacity_mah();
        let config = crate::data::Config::load(&dir);
        let mut store = Store::open(dir.clone());
        store.keep_disk_days = config.keep_days;
        store.prune_disk();
        let hist = std::sync::Arc::new(std::sync::Mutex::new(crate::history::HistStore::open(dir.join("hist"), config.keep_days)));
        let watch = crate::watch::Watch::open(dir.clone());
        let mut d = Daemon {
            dir: dir.clone(),
            pp,
            started: util::real_ms(),
            scanner,
            batt,
            screen_r: ScreenReader::new(),
            ws: WakeupSources::new(),
            susp: SuspendReader::new(),
            open: None,
            last: None,
            gpu_last: None,
            tick_no: 0,
            store,
            events: Vec::new(),
            snaps: VecDeque::new(),
            live: VecDeque::new(),
            pkgs: BTreeMap::new(),
            pkgs_at: 0,
            last_snap_real: 0,
            want_snap: true,
            last_hist_real: 0,
            full_hist_ok: false,
            last_scan_cost_us: 0,
            design_mah,
            full_mah,
            hosts: HashMap::new(),
            hosts_at: 0,
            config,
            live_until: 0,
            cost: StepCost::default(),
            cpu_total_us: 0,
            hist,
            watch,
            net_last: None,
            net_at: 0,
            vpn_pkgs: BTreeSet::new(),
            modes: crate::modes::Modes::load(&dir),
            mode_rt: crate::modes::Runtime::default(),
            protect: crate::modes::Protect::default(),
            usb: crate::usbport::UsbPort::open(),
        };
        d.load_events();
        d.load_snaps();
        // перезапуск ядра в той же загрузке: снимок с диска моложе 10 мин годится, новый не снимаем.
        // После перезагрузки телефона — снимаем всегда (счётчики «с загрузки» начались заново).
        d.last_snap_real = d.snaps.back().map(|s| s.t).unwrap_or(0);
        let same_boot = d.snaps.back().map(|s| {
            let (now_b, now_r) = (util::boot_ms() as i64, util::real_ms());
            now_b > s.boot_ms as i64 && ((now_b - s.boot_ms as i64) - (now_r - s.t)).abs() < 60_000
        }).unwrap_or(false);
        d.want_snap = !same_boot || util::real_ms() - d.last_snap_real > 600_000;
        d.refresh_pkgs();
        d
    }

    fn load_events(&mut self) {
        let p = self.dir.join("events.jsonl");
        let cut = util::real_ms() - 14 * 86_400_000;
        if let Ok(f) = std::fs::File::open(&p) {
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                if let Ok(e) = serde_json::from_str::<Event>(&line) {
                    if e.t >= cut {
                        self.events.push(e);
                    }
                }
            }
        }
    }

    fn add_event(&mut self, t: i64, kind: &str, level: u8) {
        let e = Event { t, kind: kind.to_string(), level };
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(self.dir.join("events.jsonl")) {
            let _ = writeln!(f, "{}", serde_json::to_string(&e).unwrap_or_default());
        }
        self.events.push(e);
    }

    fn load_snaps(&mut self) {
        let p = self.dir.join("snaps.jsonl");
        let cut = util::real_ms() - 3 * 86_400_000;
        let mut kept = Vec::new();
        if let Ok(f) = std::fs::File::open(&p) {
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                if let Ok(s) = serde_json::from_str::<SysSnap>(&line) {
                    if s.t >= cut {
                        kept.push((line, s));
                    }
                }
            }
        }
        // перезаписываем файл без устаревших
        if let Ok(mut f) = std::fs::File::create(&p) {
            for (line, _) in &kept {
                let _ = writeln!(f, "{line}");
            }
        }
        self.snaps = kept.into_iter().map(|(_, s)| s).collect();
    }

    pub fn add_snap(&mut self, s: SysSnap) {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(self.dir.join("snaps.jsonl")) {
            let _ = writeln!(f, "{}", serde_json::to_string(&s).unwrap_or_default());
        }
        self.last_snap_real = s.t;
        if !s.checkin.uid_pkgs.is_empty() {
            for (u, p) in &s.checkin.uid_pkgs {
                self.pkgs.entry(*u).or_insert_with(|| p.clone());
            }
        }
        self.snaps.push_back(s);
        let cut = util::real_ms() - 3 * 86_400_000;
        while self.snaps.front().map(|s| s.t < cut).unwrap_or(false) {
            self.snaps.pop_front();
        }
    }

    pub fn refresh_pkgs(&mut self) {
        let p = sys::packages();
        if !p.is_empty() {
            self.pkgs = p;
        }
        let users: BTreeSet<u32> = self.pkgs.keys().map(|u| u / 100_000).collect();
        let vpn = sys::netstat::vpn_packages(&users);
        if !vpn.is_empty() {
            self.vpn_pkgs = vpn;
        }
        self.protect = crate::modes::Protect::load(&users, &self.vpn_pkgs);
        self.pkgs_at = util::real_ms();
    }

    /// pid → (uid, имя) для привязки нативных wakelock в снимке.
    pub fn pid_names(&self) -> BTreeMap<u32, (u32, String)> {
        self.scanner.pid_names().into_iter().map(|(k, (u, n))| (k, (u, n.to_string()))).collect()
    }

    /// Нужен ли сейчас системный снимок (решение принимает цикл, снимает — отдельный поток).
    pub fn snap_due(&self, now: i64) -> bool {
        self.want_snap || now - self.last_snap_real > self.config.snap_every_min as i64 * 60_000
    }

    /// Интервал до следующего шага (по монотонным часам: во сне не тикают).
    pub fn interval_ms(&self) -> u64 {
        if util::mono_ms() < self.live_until {
            return 2_000;
        }
        match &self.last {
            Some((_, _, s)) if s.on => self.config.interval_on_ms,
            Some(_) => self.config.interval_off_ms,
            None => 1_000,
        }
    }

    pub fn tick(&mut self) {
        let t_start = std::time::Instant::now();
        let cpu_start = util::thread_cpu_us();
        self.tick_no += 1;
        if self.config.full_history && !self.full_hist_ok && self.tick_no % 6 == 1 {
            self.full_hist_ok = crate::history::set_android_full_history(true);
        }
        let now = Clocks::now();
        let c0 = util::thread_cpu_us();
        let batt = self.batt.read();
        let screen = self.screen_r.read();
        self.cost.cpu_sensors_us = util::thread_cpu_us().saturating_sub(c0);
        let t_procs = std::time::Instant::now();
        // Разложение по потокам — самое дорогое. В обычном режиме процесс раскладывается
        // раз в 6 шагов (~30 с); суммы точные, детализация с задержкой до 30 с.
        // В живом режиме («Сейчас» открыт) — каждый шаг.
        let live = now.mono < self.live_until;
        // разбор не по объёму, а строго раз в max_defer шагов (новые процессы — сразу)
        self.scanner.min_ticks = if live { 1 } else { u64::MAX };
        self.scanner.max_defer = if live { 1 } else { 6 };
        self.scanner.cold_every = if live { 1 } else { 6 };
        let c1 = util::thread_cpu_us();
        let cpu = self.scanner.scan(now.boot);
        self.cost.cpu_scan_us = util::thread_cpu_us().saturating_sub(c1);
        self.cost.procs_us = t_procs.elapsed().as_micros() as u64;
        self.cost.procs = cpu.scanned_procs;
        self.cost.threads = cpu.scanned_threads;
        self.cost.tis_reads = cpu.tis_reads;
        self.cost.skipped_idle = cpu.skipped_idle;
        let sd = self.susp.sample();
        let do_ws = !screen.on || self.tick_no % 6 == 1;
        let t_ws = std::time::Instant::now();
        let wsd = if do_ws { self.ws.sample(now.mono) } else { Vec::new() };
        self.cost.wakeup_sources_us = t_ws.elapsed().as_micros() as u64;
        let gpu = sysfs::gpu_busy();
        let gpu_frac = match (gpu, self.gpu_last) {
            (Some((a, b)), Some((pa, pb))) if b > pb => (a.saturating_sub(pa)) as f64 / (b - pb) as f64,
            (Some((a, b)), _) if b > 0 => a as f64 / b as f64,
            _ => 0.0,
        };
        self.gpu_last = gpu;

        if now.real - self.pkgs_at > 6 * 3_600_000 {
            self.refresh_pkgs();
        }

        let Some((last, last_batt, last_screen)) = self.last.clone() else {
            self.add_event(now.real, "start", batt.level);
            self.last = Some((now, batt, screen));
            self.last_scan_cost_us = t_start.elapsed().as_micros() as u64;
            return;
        };

        // события
        if screen.on != last_screen.on {
            self.add_event(now.real, if screen.on { "screen_on" } else { "screen_off" }, batt.level);
            // снимок на смене экрана (граница периода «экран выкл» в данных Android) — не чаще раза в 5 мин
            if now.real - self.last_snap_real > 300_000 {
                self.want_snap = true;
            }
        }
        if batt.plugged != last_batt.plugged {
            self.add_event(now.real, if batt.plugged { "plug" } else { "unplug" }, batt.level);
            self.want_snap = true;
        }

        // (0.8) сеть по приложениям: раз в 5 мин и на смене экрана/зарядки — тогда разность целиком
        // относится к состоянию до смены и ложится в корзину этого состояния (её закроют ниже)
        let changed = screen.on != last_screen.on || batt.plugged != last_batt.plugged;
        let mut net_pending: Vec<NetRec> = Vec::new();
        if changed || now.real - self.net_at >= NET_EVERY_MS {
            let t_net = std::time::Instant::now();
            self.net_at = now.real;
            if let Some(cur) = sys::netstat::take() {
                if let Some(prev) = &self.net_last {
                    net_pending = sys::netstat::delta(prev, &cur)
                        .into_iter()
                        .map(|(u, c)| NetRec { u, rp: c.rp, rb: c.rb, tp: c.tp, tb: c.tb })
                        .collect();
                }
                self.net_last = Some(cur);
            }
            self.cost.net_us = t_net.elapsed().as_micros() as u64;
        }
        if let Some(ob) = self.open.as_mut() {
            merge_net(&mut ob.b.net, std::mem::take(&mut net_pending));
        }

        let d_boot = now.boot.saturating_sub(last.boot);
        let d_mono = now.mono.saturating_sub(last.mono).min(d_boot);
        let d_susp = d_boot - d_mono;

        // закрыть корзину?
        let close = match &self.open {
            Some(ob) => {
                ob.b.scr != screen.on
                    || ob.b.plug != batt.plugged
                    || now.real - ob.b.t0 >= 60_000
                    || d_boot > 180_000
            }
            None => false,
        };
        if close {
            let ob = self.open.take().unwrap();
            self.store.push(ob.finish());
        }
        let ob = self.open.get_or_insert_with(|| {
            let mut o = OpenBucket::default();
            o.b.t0 = last.real;
            o.b.scr = screen.on;
            o.b.plug = batt.plugged;
            o.b.lvl0 = last_batt.level;
            o.b.freq = cpu.freq_ticks.iter().map(|v| vec![0; v.len()]).collect();
            o
        });
        merge_net(&mut ob.b.net, net_pending);
        let b = &mut ob.b;
        b.t1 = now.real;
        b.span += d_boot;
        b.awake += d_mono;
        b.susp += d_susp;
        b.ticks += 1;
        b.lvl1 = batt.level;
        b.temp = batt.temp_c;
        ob.mv_sum += batt.voltage_mv;
        if let (Some(c0), Some(c1)) = (last_batt.charge_uah, batt.charge_uah) {
            b.cc_uah += c0 - c1;
            b.cc_ok = true;
        }
        let avg_draw = (last_batt.draw_ma + batt.draw_ma) / 2.0;
        b.cur_mas += avg_draw * d_mono as f64 / 1000.0;
        let cpu_mas: f64 = cpu.procs.iter().map(|p| p.mas).sum();
        b.cpu_mas += cpu_mas;
        b.base_mas += self.pp.cpu_idle_ma * d_mono as f64 / 1000.0;
        let scr_ma = if screen.on { self.pp.screen_on_ma + screen.bright * self.pp.screen_full_ma } else { 0.0 };
        b.scr_mas += scr_ma * d_boot as f64 / 1000.0;
        if screen.on {
            ob.bright_ms += screen.bright * d_boot as f64;
            ob.on_ms += d_boot;
        }
        ob.gpu_sum += gpu_frac;
        ob.gpu_n += 1;
        b.sok += sd.ok;
        b.sfail += sd.fail;
        if let Some(r) = sd.reason {
            *ob.rr.entry(r).or_default() += 1;
        }
        for (ci, v) in cpu.freq_ticks.iter().enumerate() {
            if let Some(dst) = b.freq.get_mut(ci) {
                for (i, x) in v.iter().enumerate() {
                    if let Some(d) = dst.get_mut(i) {
                        *d += x;
                    }
                }
            }
        }
        for (name, c) in wsd {
            let w = ob.ws.entry(name.clone()).or_insert_with(|| WsRec { n: name, ..Default::default() });
            w.ac += c.active_count;
            w.wk += c.wakeup_count;
            w.ms += c.total_ms;
            w.pv += c.prevent_ms;
        }
        let tick_ms = self.scanner.tick_ms();
        // хозяева изолированных процессов: спрашиваем ActivityManager, только если появился незнакомый
        let need = cpu.procs.iter().any(|p| is_isolated(p.uid) && self.hosts.get(&p.pid).map(|h| h.0 != p.uid).unwrap_or(true));
        let t_hosts = std::time::Instant::now();
        if need && now.mono.saturating_sub(self.hosts_at) > 20_000 {
            self.hosts_at = now.mono;
            let found = sys::isolated_hosts();
            let uids = self.scanner.pid_names();
            self.hosts.retain(|pid, _| uids.contains_key(pid));
            for (pid, host) in found {
                if let Some((uid, _)) = uids.get(&pid) {
                    self.hosts.insert(pid, (*uid, host));
                }
            }
            // не найденных запоминаем как «хозяин неизвестен», чтобы не дёргать дамп снова
            for p in cpu.procs.iter().filter(|p| is_isolated(p.uid)) {
                self.hosts.entry(p.pid).or_insert((p.uid, NO_HOST));
            }
        }
        self.cost.hosts_us = t_hosts.elapsed().as_micros() as u64;
        let ob = self.open.as_mut().unwrap();
        merge_procs(ob, &cpu, tick_ms, &self.hosts);

        // «живой» ряд
        let dt_s = (d_boot as f64 / 1000.0).max(0.001);
        let host_of = |p: &crate::procfs::TickProc| self.hosts.get(&p.pid).filter(|h| h.0 == p.uid && h.1 != NO_HOST).map(|h| h.1);
        let mut top: Vec<(u32, String, f64)> = cpu.procs.iter().map(|p| (host_of(p).unwrap_or(p.uid), p.name.to_string(), p.mas / dt_s)).collect();
        top.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        top.truncate(10);
        let cpu_ma = cpu_mas / dt_s;
        let model_ma = cpu_ma + self.pp.cpu_idle_ma * d_mono as f64 / d_boot.max(1) as f64 + scr_ma;
        self.live.push_back(LiveTick {
            t: now.real,
            dt_ms: d_boot,
            draw_ma: batt.draw_ma,
            voltage_mv: batt.voltage_mv,
            level: batt.level,
            screen: screen.on,
            plugged: batt.plugged,
            model_ma,
            cpu_ma,
            top,
        });
        while self.live.len() > 180 {
            self.live.pop_front();
        }
        self.last = Some((now, batt, screen));
        self.last_scan_cost_us = t_start.elapsed().as_micros() as u64;
        self.cost.total_us = self.last_scan_cost_us;
        self.cost.cpu_us = util::thread_cpu_us().saturating_sub(cpu_start);
        self.cpu_total_us += self.cost.cpu_us;
    }

    /// Закрыть текущую корзину (при остановке), чтобы ничего не потерять.
    pub fn flush(&mut self) {
        if let Some(ob) = self.open.take() {
            if ob.b.ticks > 0 {
                self.store.push(ob.finish());
            }
        }
    }

    /// Текущая открытая корзина (для отчётов «до этой секунды»).
    pub fn open_snapshot(&self) -> Option<Bucket> {
        self.open.as_ref().map(|ob| ob.clone().finish())
    }

    pub fn set_config(&mut self, c: crate::data::Config) {
        if c.full_history != self.config.full_history {
            self.full_hist_ok = crate::history::set_android_full_history(c.full_history) && c.full_history;
        }
        c.save(&self.dir);
        self.store.keep_disk_days = c.keep_days;
        if let Ok(mut h) = self.hist.lock() {
            h.keep_days = c.keep_days;
        }
        self.config = c;
        self.store.prune_disk();
    }

    /// Очистка записей (все / раньше before). → (файлов, байт)
    pub fn prune(&mut self, before: Option<i64>) -> (u64, u64) {
        let (mut n, mut freed) = self.store.prune(before);
        // actions.jsonl не чистим: по нему Жор возвращает ограничения фона, которые поставил
        for name in ["snaps.jsonl", "events.jsonl"] {
            let p = self.dir.join(name);
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            if size == 0 {
                continue;
            }
            match before {
                None => {
                    if std::fs::remove_file(&p).is_ok() {
                        n += 1;
                        freed += size;
                    }
                }
                Some(b) => {
                    let kept: Vec<String> = std::fs::File::open(&p)
                        .map(|f| {
                            BufReader::new(f)
                                .lines()
                                .map_while(Result::ok)
                                .filter(|l| {
                                    serde_json::from_str::<serde_json::Value>(l)
                                        .ok()
                                        .and_then(|v| v["t"].as_i64())
                                        .map(|t| t >= b)
                                        .unwrap_or(false)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut text = kept.join("\n");
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    let _ = std::fs::write(&p, &text);
                    freed += size.saturating_sub(text.len() as u64);
                }
            }
        }
        match before {
            None => {
                self.snaps.clear();
                self.events.clear();
            }
            Some(b) => {
                self.snaps.retain(|s| s.t >= b);
                self.events.retain(|e| e.t >= b);
            }
        }
        let (wn, wf) = self.watch.prune(before);
        n += wn;
        freed += wf;
        if let Ok(mut h) = self.hist.lock() {
            let (hn, hf) = h.prune(before);
            n += hn;
            freed += hf;
        }
        self.want_snap = true;
        (n, freed)
    }

    /// (0.8) Режимы: решить, что сделать. Сами команды (`cmd appops`, `force-stop`) — вне замка,
    /// в [crate::modes::execute]. `env` — экономия энергии и страна сети (их читает цикл, тоже вне замка).
    pub fn modes_plan(&mut self, env: &crate::modes::Env) -> Vec<crate::modes::Op> {
        let now = util::real_ms();
        let Some((_, batt, screen)) = self.last.clone() else { return Vec::new() };
        if !crate::modes::due(&self.mode_rt, now, screen.on) {
            return Vec::new();
        }
        let claims: Vec<crate::compare::Restr> = crate::compare::claims(&self.dir).into_iter().filter(|r| r.by == "mode").collect();
        if !self.mode_rt.trips_restored() {
            self.mode_rt.restore_trips(&crate::compare::watch_trips(&self.dir), &claims);
        }
        let dis_claims: Vec<crate::compare::Restr> = crate::compare::disable_claims(&self.dir).into_iter().filter(|r| r.by == "mode").collect();
        let net_claims: Vec<crate::compare::Restr> = crate::compare::nonet_claims(&self.dir).into_iter().filter(|r| r.by == "mode").collect();
        let comp_claims: Vec<crate::compare::Restr> = crate::compare::comp_claims(&self.dir).into_iter().filter(|r| r.by == "mode").collect();
        let running: BTreeSet<u32> = self.scanner.pid_names().values().map(|(u, _)| *u).collect();
        let open = self.open_snapshot();
        let cut = now - 35 * 60_000;
        let tail: Vec<&Bucket> = self.store.mem.iter().filter(|b| b.t1 > cut).chain(open.iter()).collect();
        // когда приложение было на экране — по истории Android (в памяти ядра ~26 ч)
        let top: Option<(HashMap<u32, (i64, bool)>, i64)> = self.hist.lock().ok().and_then(|h| {
            let since = h.ev.first().map(|e| e.t0)?;
            let mut m: HashMap<u32, (i64, bool)> = HashMap::new();
            for e in h.ev.iter().filter(|e| e.k == crate::history::Kind::Top) {
                let x = m.entry(e.u).or_insert((0, false));
                x.0 = x.0.max(e.t1);
                x.1 |= e.open;
            }
            Some((m, since))
        });
        let inputs = crate::modes::Inputs {
            now,
            local_min: crate::modes::local_min(now),
            screen_on: screen.on,
            plugged: batt.plugged,
            level: batt.level,
            env,
            pkgs: &self.pkgs,
            protect: &self.protect,
            running: &running,
            claims: &claims,
            dis_claims: &dis_claims,
            net_claims: &net_claims,
            comp_claims: &comp_claims,
            tail: &tail,
            last_top: top.as_ref().map(|(m, s)| (m, *s)),
            hist_at: self.last_hist_real,
        };
        crate::modes::plan(&self.modes, &mut self.mode_rt, &inputs)
    }

    /// Сторож: проверить правила по хвосту корзин (не чаще раза в 5 мин, только на шаге сбора).
    /// Возвращает новые тревоги — доставляет и записывает их вызывающий (вне замка).
    pub fn watch_eval(&mut self) -> Vec<crate::watch::Alert> {
        let now = util::real_ms();
        if !self.watch.due(now) {
            return Vec::new();
        }
        let open = self.open_snapshot();
        let mut tail: Vec<&crate::store::Bucket> = Vec::new();
        if let Some(ob) = &open {
            tail.push(ob);
        }
        for b in self.store.mem.iter().rev() {
            tail.push(b);
            if b.scr || b.plug {
                break;
            }
        }
        tail.reverse();
        let cap = self.full_mah.or(self.design_mah);
        let snaps: Vec<&sys::SysSnap> = self.snaps.iter().collect();
        let x = crate::watch::Extra { snaps: &snaps, vpn: Some(&self.vpn_pkgs) };
        let alerts = self.watch.eval_with(&tail, x, &self.config, cap, &self.pkgs, now);
        // (0.8.3) «Телефон не спит» ждёт свежий снимок — снимет поток снимков в этом же шаге
        if self.watch.take_want_snap() {
            self.want_snap = true;
        }
        alerts
    }

    /// (0.8.3) Мигание порта USB-C — на каждом шаге, при любом экране и зарядке: правила сторожа
    /// смотрят только покой без зарядки и раз в 5 минут, а паника бывает через несколько минут после
    /// начала мигания. → новые тревоги и строка в журнал (не чаще раза в минуту, пока мигает).
    pub fn usb_eval(&mut self) -> (Vec<crate::watch::Alert>, Option<String>) {
        self.usb.poll();
        let now_us = util::mono_ms() * 1000;
        let mut line = None;
        if self.usb.pending > 0 && now_us.saturating_sub(self.usb.logged_us) >= 60_000_000 {
            let since = if self.usb.logged_us == 0 { "с загрузки".to_string() } else { format!("за {}", util::fmt_dur((now_us - self.usb.logged_us) / 1000)) };
            line = Some(format!("USB-C: порт включал режим хоста {} раз {since}", self.usb.pending));
            self.usb.pending = 0;
            self.usb.logged_us = now_us;
        }
        let alerts = match self.usb.det.check(now_us) {
            Some(f) => {
                let a = crate::watch::usb_flap_alert(&f, util::real_ms(), now_us, &self.usb.boot);
                self.watch.admit(&self.config, a).into_iter().collect()
            }
            None => Vec::new(),
        };
        (alerts, line)
    }

    pub fn current(&self) -> Option<(Battery, Screen)> {
        self.last.as_ref().map(|(_, b, s)| (b.clone(), *s))
    }
}

const NO_HOST: u32 = u32::MAX;

fn is_isolated(uid: u32) -> bool {
    let a = uid % 100_000;
    (90_000..100_000).contains(&a)
}

fn merge_procs(ob: &mut OpenBucket, cpu: &TickCpu, tick_ms: f64, hosts: &HashMap<u32, (u32, u32)>) {
    for p in &cpu.procs {
        let host = hosts.get(&p.pid).filter(|h| h.0 == p.uid && h.1 != NO_HOST).map(|h| h.1);
        let key = (p.uid, p.name.to_string(), host);
        let e = ob.procs.entry(key).or_insert_with(|| {
            (ProcRec { uid: p.uid, n: p.name.to_string(), k: p.kernel, h: host, ..Default::default() }, HashMap::new())
        });
        e.0.t += (p.ticks as f64 * tick_ms) as u64;
        e.0.e += p.mas;
        if p.started {
            e.0.st += 1;
        }
        for t in &p.threads {
            let name = if t.main { "<main>".to_string() } else { t.name.to_string() };
            let r = e.1.entry(name.clone()).or_insert_with(|| ThrRec { n: name, t: 0, e: 0.0 });
            r.t += (t.ticks as f64 * tick_ms) as u64;
            r.e += t.mas;
        }
    }
}

// OpenBucket нужен Clone для снимка открытой корзины.
impl Clone for OpenBucket {
    fn clone(&self) -> Self {
        OpenBucket {
            b: self.b.clone(),
            procs: self.procs.clone(),
            ws: self.ws.clone(),
            rr: self.rr.clone(),
            bright_ms: self.bright_ms,
            on_ms: self.on_ms,
            gpu_sum: self.gpu_sum,
            gpu_n: self.gpu_n,
            mv_sum: self.mv_sum,
        }
    }
}
