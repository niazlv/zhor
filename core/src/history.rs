//! История событий Android с точным временем: кто и когда запускал задания, держал wakelock,
//! синхронизировался, был на экране, кого будили пакеты из сети, когда работали GPS и радио.
//!
//! Источник — строки «9,h,…» в выводе `dumpsys batterystats -c`, который ядро и так снимает для
//! снимков (отдельного вызова dumpsys нет). Android держит там ~45 ч (буфер 4 МБ); ядро разбирает
//! весь буфер при каждом снимке, заменяет свои записи начиная с его начала и хранит события
//! в `hist/ГГГГ-ММ-ДД.jsonl.gz` дольше, чем Android (по `keep_days`).
//!
//! Формат строки: `9,h,<дельта мс>[,элементы…]` или `9,h,<дельта>:TIME:<мс эпохи>` (якорь
//! настенного времени), `:RESET:TIME:…`, `:START`, `:SHUTDOWN`. Элементы: флаги состояния
//! `+r`/`-r` (не спит), `+S` (экран), `+g` (GPS)…; события `+Ejb=u0a253:"тег"` (начало/конец),
//! `Esw=…` (мгновенное); тег — либо `uid:"строка"`, либо номер в пуле `9,hsp,<n>,<uid>,"строка"`.
//! `+w=…`/`-w=…` — не отдельные wakelock, а состояние «телефон держат wakelock'и»: тег — тот,
//! кто взял первым (строго чередуются, проверено на Android 17).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const NO_UID: u32 = u32::MAX;
/// Сколько держать в памяти (не меньше буфера Android); старше — с диска по запросу.
/// С подробной историей (full-history) событий ~10 тыс./ч: сутки в памяти ≈ 10 МБ, три — ≈ 30 МБ.
const MEM_MS: i64 = 26 * 3_600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    // ── приложения (есть uid) ──
    /// состояние «держат wakelock», приписано тому, кто взял первым
    Wakelock,
    /// отдельный wakelock (только при `dumpsys batterystats enable full-history`)
    WakelockIn,
    /// wakelock держится дольше минуты
    LongWake,
    Job,
    Sync,
    Top,
    Fg,
    /// временное разрешение работать в фоне (высокоприоритетный пуш, будильник)
    TmpWhitelist,
    /// входящий сетевой пакет разбудил процессор
    WakeupAp,
    /// только при full-history
    Alarm,
    /// только при full-history
    Proc,
    ScreenWake,
    // ── телефон целиком ──
    Running,
    Screen,
    Doze,
    Gps,
    Radio,
    WifiScan,
    BleScan,
    Audio,
    Video,
    Camera,
    WakeReason,
}

use Kind::*;

impl Kind {
    pub const ALL: [Kind; 23] = [
        Wakelock, WakelockIn, LongWake, Job, Sync, Top, Fg, TmpWhitelist, WakeupAp, Alarm, Proc, ScreenWake, Running, Screen, Doze, Gps,
        Radio, WifiScan, BleScan, Audio, Video, Camera, WakeReason,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Wakelock => "wakelock",
            WakelockIn => "wakelock_in",
            LongWake => "longwake",
            Job => "job",
            Sync => "sync",
            Top => "top",
            Fg => "fg",
            TmpWhitelist => "tmpwhitelist",
            WakeupAp => "wakeupap",
            Alarm => "alarm",
            Proc => "proc",
            ScreenWake => "screenwake",
            Running => "running",
            Screen => "screen",
            Doze => "doze",
            Gps => "gps",
            Radio => "mobile_radio",
            WifiScan => "wifi_scan",
            BleScan => "ble_scan",
            Audio => "audio",
            Video => "video",
            Camera => "camera",
            WakeReason => "wake_reason",
        }
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| k.name() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Wakelock => "Не давал уснуть (wakelock)",
            WakelockIn => "Wakelock",
            LongWake => "Длинный wakelock (> 1 мин)",
            Job => "Фоновые задания",
            Sync => "Синхронизация",
            Top => "На экране",
            Fg => "Передний план",
            TmpWhitelist => "Разрешён фон (пуш, будильник)",
            WakeupAp => "Сеть разбудила телефон",
            Alarm => "Будильники",
            Proc => "Процесс работал (запуск — остановка)",
            ScreenWake => "Включил экран",
            Running => "Телефон не спал",
            Screen => "Экран включён",
            Doze => "Doze (экономия в покое)",
            Gps => "GPS",
            Radio => "Мобильное радио",
            WifiScan => "Поиск Wi-Fi",
            BleScan => "Поиск Bluetooth LE",
            Audio => "Звук",
            Video => "Видео",
            Camera => "Камера",
            WakeReason => "Причины пробуждения ядра",
        }
    }

    /// Событие всего телефона (не приложения).
    pub fn global(self) -> bool {
        matches!(self, Running | Screen | Doze | Gps | Radio | WifiScan | BleScan | Audio | Video | Camera | WakeReason)
    }

    /// Одно состояние на весь телефон: начало/конец сопоставляются без тега.
    fn is_state(self) -> bool {
        matches!(self, Wakelock | Running | Screen | Doze | Gps | Radio | WifiScan | BleScan | Audio | Video | Camera)
    }

    fn from_event_code(code: &str) -> Option<Kind> {
        Some(match code {
            "Ewl" => WakelockIn,
            "Elw" => LongWake,
            "Ejb" => Job,
            "Esy" => Sync,
            "Etp" => Top,
            "Efg" => Fg,
            "Etw" => TmpWhitelist,
            "Ewa" => WakeupAp,
            "Eal" => Alarm,
            "Epr" => Proc,
            "Esw" => ScreenWake,
            _ => return None,
        })
    }

    fn from_flag(code: &str) -> Option<Kind> {
        Some(match code {
            "w" => Wakelock,
            "r" => Running,
            "S" => Screen,
            "g" => Gps,
            "Pr" => Radio,
            "Ws" => WifiScan,
            "bles" => BleScan,
            "a" => Audio,
            "v" => Video,
            "ca" => Camera,
            _ => return None,
        })
    }
}

/// Событие: интервал [t0, t1] (мгновенное — t0 == t1), мс эпохи.
#[derive(Debug, Clone)]
pub struct Ev {
    pub t0: i64,
    pub t1: i64,
    pub k: Kind,
    pub u: u32,
    pub g: Arc<str>,
    /// ещё не закончилось на момент снимка
    pub open: bool,
}

impl Ev {
    fn same_key(&self, o: &Ev) -> bool {
        self.k == o.k && (self.k.is_state() || (self.u == o.u && self.g == o.g))
    }
}

/// Разобранный буфер истории.
#[derive(Debug, Clone, Default)]
pub struct Dump {
    /// первое и последнее событие с известным настенным временем
    pub from: i64,
    pub to: i64,
    /// по t0
    pub ev: Vec<Ev>,
    /// концы без начала в буфере (начались раньше его начала)
    pub orphans: Vec<Ev>,
    pub records: usize,
}

/// uid в записи Android (`UserHandle.formatUid`): 1000, u0a253, u10a201, u0i12, u0ai3, u10s1000.
pub fn parse_uid(s: &str) -> u32 {
    if let Ok(n) = s.parse::<u32>() {
        return n;
    }
    let Some(rest) = s.strip_prefix('u') else { return NO_UID };
    let ud = rest.bytes().take_while(|c| c.is_ascii_digit()).count();
    let Ok(user) = rest[..ud].parse::<u32>() else { return NO_UID };
    let tail = &rest[ud..];
    let (base, num) = if let Some(n) = tail.strip_prefix("ai") {
        (90_000, n)
    } else if let Some(n) = tail.strip_prefix('a') {
        (10_000, n)
    } else if let Some(n) = tail.strip_prefix('i') {
        (99_000, n)
    } else if let Some(n) = tail.strip_prefix('s') {
        (0, n)
    } else {
        return NO_UID;
    };
    match num.parse::<u32>() {
        Ok(n) => user * 100_000 + base + n,
        Err(_) => NO_UID,
    }
}

/// Делит по запятым вне кавычек.
fn split_items(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut q = false;
    let mut start = 0;
    for (i, c) in s.bytes().enumerate() {
        match c {
            b'"' => q = !q,
            b',' if !q => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

struct Parser {
    pool: HashMap<u32, (u32, Arc<str>)>,
    strings: HashMap<String, Arc<str>>,
    open: HashMap<(Kind, u32, Arc<str>), (Ev, u32)>,
    out: Dump,
    empty: Arc<str>,
}

impl Parser {
    fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(a) = self.strings.get(s) {
            return a.clone();
        }
        let a: Arc<str> = Arc::from(s);
        self.strings.insert(s.to_string(), a.clone());
        a
    }

    /// `u0a253:"строка"` или номер в пуле.
    fn tag(&mut self, v: &str) -> (u32, Arc<str>) {
        if let Some(p) = v.find(":\"") {
            let uid = parse_uid(&v[..p]);
            let s = v[p + 2..].strip_suffix('"').unwrap_or(&v[p + 2..]);
            return (uid, self.intern(s));
        }
        if let Ok(i) = v.parse::<u32>() {
            if let Some((u, s)) = self.pool.get(&i) {
                return (*u, s.clone());
            }
            return (NO_UID, self.intern(&format!("#{i}")));
        }
        (NO_UID, self.intern(v))
    }

    fn key(&self, k: Kind, u: u32, g: &Arc<str>) -> (Kind, u32, Arc<str>) {
        if k.is_state() {
            (k, NO_UID, self.empty.clone())
        } else {
            (k, u, g.clone())
        }
    }

    fn start(&mut self, t: i64, k: Kind, u: u32, g: Arc<str>) {
        let key = self.key(k, u, &g);
        match self.open.get_mut(&key) {
            // вложенный захват того же тега (или повтор состояния после сброса)
            Some(e) if !k.is_state() => e.1 += 1,
            Some(_) => {}
            None => {
                self.open.insert(key, (Ev { t0: t, t1: t, k, u, g, open: true }, 1));
            }
        }
    }

    fn finish(&mut self, t: i64, k: Kind, u: u32, g: Arc<str>) {
        let key = self.key(k, u, &g);
        match self.open.get_mut(&key) {
            Some(e) if e.1 > 1 => e.1 -= 1,
            Some(_) => {
                let (mut e, _) = self.open.remove(&key).unwrap();
                e.t1 = t.max(e.t0);
                e.open = false;
                self.out.ev.push(e);
            }
            None => self.out.orphans.push(Ev { t0: t, t1: t, k, u, g, open: false }),
        }
    }

    fn instant(&mut self, t: i64, k: Kind, u: u32, g: Arc<str>) {
        self.out.ev.push(Ev { t0: t, t1: t, k, u, g, open: false });
    }

    /// Закрыть всё открытое (перезагрузка, сброс статистики).
    fn close_all(&mut self, t: i64) {
        for (_, (mut e, _)) in self.open.drain() {
            e.t1 = t.max(e.t0);
            e.open = false;
            no_end(&mut e);
            self.out.ev.push(e);
        }
    }

    fn item(&mut self, t: i64, it: &str) {
        let (sign, body) = match it.as_bytes().first() {
            Some(b'+') => (1, &it[1..]),
            Some(b'-') => (-1, &it[1..]),
            _ => (0, it),
        };
        let (name, val) = match body.find('=') {
            Some(p) => (&body[..p], Some(&body[p + 1..])),
            None => (body, None),
        };
        if name == "wr" {
            if let Some(v) = val {
                let (_, g) = self.tag(v);
                self.instant(t, WakeReason, NO_UID, g);
            }
            return;
        }
        if name == "di" {
            let mode = val.unwrap_or("off");
            let cur = self.open.get(&(Doze, NO_UID, self.empty.clone())).map(|e| e.0.g.clone());
            if cur.as_deref() == Some(mode) {
                return;
            }
            if cur.is_some() {
                self.finish(t, Doze, NO_UID, self.empty.clone());
            }
            if mode != "off" {
                let g = self.intern(mode);
                self.start(t, Doze, NO_UID, g);
            }
            return;
        }
        let kind = if name.starts_with('E') { Kind::from_event_code(name) } else { Kind::from_flag(name) };
        let Some(k) = kind else { return };
        let (u, g) = match val {
            Some(v) => self.tag(v),
            None => (NO_UID, self.empty.clone()),
        };
        match sign {
            1 => self.start(t, k, u, g),
            -1 => self.finish(t, k, u, g),
            _ if k.is_state() => {}
            _ => self.instant(t, k, u, g),
        }
    }
}

/// Разбор истории из вывода `dumpsys batterystats -c` (остальные строки пропускаются).
pub fn parse_checkin(text: &str) -> Dump {
    let empty: Arc<str> = Arc::from("");
    let mut p = Parser { pool: HashMap::new(), strings: HashMap::new(), open: HashMap::new(), out: Dump::default(), empty };
    // время истории — сумма дельт; настенное = время истории + base (якорь по TIME)
    let mut ht: i64 = 0;
    let mut base: Option<i64> = None;
    let mut first: Option<i64> = None;
    let mut last: i64 = 0;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("9,hsp,") {
            // 9,hsp,<n>,<uid>,"строка"
            let mut it = rest.splitn(3, ',');
            let (Some(n), Some(u), Some(s)) = (it.next(), it.next(), it.next()) else { continue };
            let (Ok(n), Ok(u)) = (n.parse::<u32>(), u.parse::<i64>()) else { continue };
            let s = s.trim().trim_matches('"');
            let uid = if u < 0 { NO_UID } else { u as u32 };
            let a = p.intern(s);
            p.pool.insert(n, (uid, a));
            continue;
        }
        let Some(rest) = line.strip_prefix("9,h,") else { continue };
        p.out.records += 1;
        let nd = rest.bytes().take_while(|c| c.is_ascii_digit()).count();
        let delta: i64 = rest[..nd].parse().unwrap_or(0);
        ht += delta;
        let tail = &rest[nd..];
        if let Some(sp) = tail.strip_prefix(':') {
            let (reset, sp) = match sp.strip_prefix("RESET:") {
                Some(x) => (true, x),
                None => (false, sp),
            };
            if let Some(e) = sp.strip_prefix("TIME:").and_then(|x| x.trim().parse::<i64>().ok()) {
                if reset {
                    if let Some(b) = base {
                        p.close_all(ht + b);
                    }
                }
                base = Some(e - ht);
            } else if sp.starts_with("START") || sp.starts_with("SHUTDOWN") {
                if let Some(b) = base {
                    p.close_all(ht + b);
                }
            }
            continue;
        }
        let Some(b) = base else { continue };
        let t = ht + b;
        if first.is_none() {
            first = Some(t);
        }
        last = last.max(t);
        let Some(items) = tail.strip_prefix(',') else { continue };
        for it in split_items(items) {
            p.item(t, it);
        }
    }
    // незакрытые — до конца буфера
    for (_, (mut e, _)) in p.open.drain() {
        e.t1 = last.max(e.t0);
        e.open = true;
        no_end(&mut e);
        p.out.ev.push(e);
    }
    p.out.from = first.unwrap_or(0);
    p.out.to = last;
    p.out.ev.sort_by_key(|e| (e.t0, e.t1));
    p.out
}

/// Конца в истории не было. «Разрешён фон» у пользователей кроме 0 (второе пространство,
/// двойные приложения) Android закрывать не записывает (на Android 17: 522 начала, 0 концов),
/// и такое событие растянулось бы до сброса статистики. Длиннее 10 мин без конца — мгновенное.
fn no_end(e: &mut Ev) {
    if e.k == TmpWhitelist && e.t1 - e.t0 > 600_000 {
        e.t1 = e.t0;
        e.open = false;
    }
}

/// Включить/выключить подробную историю Android. Опция живёт до перезагрузки
/// («Option state is not saved across boots»), поэтому ядро повторяет включение перед снимками.
pub fn set_android_full_history(on: bool) -> bool {
    let arg = if on { "enable" } else { "disable" };
    crate::util::run_cmd("dumpsys", &["batterystats", arg, "full-history"], 10_000)
        .map(|o| o.contains("Enabled") || o.contains("Disabled"))
        .unwrap_or(false)
}

// ───────────────────────── хранение ─────────────────────────

#[derive(Serialize, Deserialize)]
struct DiskEv {
    a: i64,
    b: i64,
    k: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    u: Option<u32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    g: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    o: bool,
}

pub struct HistStore {
    dir: PathBuf,
    /// по t0; события с t0 ≥ mem_from
    pub ev: Vec<Ev>,
    mem_from: i64,
    /// самое раннее время, с которого есть история (память + диск)
    pub covered_from: Option<i64>,
    pub dump_from: i64,
    pub dump_to: i64,
    pub last_cost_ms: u64,
    pub keep_days: i64,
    /// отпечатки записанных дней — не переписывать неизменённые
    day_fp: HashMap<String, u64>,
    strings: HashMap<String, Arc<str>>,
    /// после очистки пользователем: раньше этого из буфера Android не брать (hist/.floor)
    floor: i64,
}

fn fingerprint(evs: &[&Ev]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for e in evs {
        (e.t0, e.t1, e.k, e.u, &*e.g, e.open).hash(&mut h);
    }
    evs.len().hash(&mut h);
    h.finish()
}

fn day_start(day: &str) -> i64 {
    // ГГГГ-ММ-ДД (местное) → мс эпохи начала дня, через смещение пояса в полдень этого дня
    let p: Vec<i64> = day.split('-').filter_map(|x| x.parse().ok()).collect();
    if p.len() != 3 {
        return 0;
    }
    let days = days_from_civil(p[0], p[1] as u32, p[2] as u32);
    let utc = days * 86_400_000;
    utc - crate::store::local_offset_ms(utc + 12 * 3_600_000)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl HistStore {
    pub fn open(dir: PathBuf, keep_days: i64) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        let now = crate::util::real_ms();
        let mut s = HistStore {
            dir,
            ev: Vec::new(),
            mem_from: now - MEM_MS,
            covered_from: None,
            dump_from: 0,
            dump_to: 0,
            last_cost_ms: 0,
            keep_days,
            day_fp: HashMap::new(),
            strings: HashMap::new(),
            floor: 0,
        };
        s.floor = std::fs::read_to_string(s.dir.join(".floor")).ok().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
        let from_day = crate::store::day_of(s.mem_from);
        let files = s.files();
        if let Some(f) = files.first() {
            s.covered_from = s.read_file(f).first().map(|e| e.t0);
        }
        let mf = s.mem_from;
        for f in &files {
            if stem(f) >= from_day {
                let evs = s.read_file(f);
                s.ev.extend(evs.into_iter().filter(|e| e.t0 >= mf));
            }
        }
        s.ev.sort_by_key(|e| (e.t0, e.t1));
        s
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".jsonl.gz")).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(a) = self.strings.get(s) {
            return a.clone();
        }
        let a: Arc<str> = Arc::from(s);
        self.strings.insert(s.to_string(), a.clone());
        a
    }

    fn read_file(&mut self, p: &Path) -> Vec<Ev> {
        let Ok(f) = File::open(p) else { return Vec::new() };
        let r = BufReader::new(flate2::read::GzDecoder::new(f));
        let mut out = Vec::new();
        for line in r.lines().map_while(Result::ok) {
            let Ok(d) = serde_json::from_str::<DiskEv>(&line) else { continue };
            let Some(k) = Kind::from_name(&d.k) else { continue };
            let g = self.intern(&d.g);
            out.push(Ev { t0: d.a, t1: d.b, k, u: d.u.unwrap_or(NO_UID), g, open: d.o });
        }
        out
    }

    fn write_day(&mut self, day: &str) -> std::io::Result<()> {
        let ds = day_start(day);
        let next = crate::store::day_of(ds + 36 * 3_600_000);
        let de = day_start(&next);
        // часть дня, которой уже нет в памяти, — с диска
        let path = self.dir.join(format!("{day}.jsonl.gz"));
        let mf = self.mem_from;
        let old: Vec<Ev> = if ds < mf { self.read_file(&path).into_iter().filter(|e| e.t0 < mf).collect() } else { Vec::new() };
        let lo = self.ev.partition_point(|e| e.t0 < ds);
        let hi = self.ev.partition_point(|e| e.t0 < de);
        let mut all: Vec<&Ev> = old.iter().collect();
        all.extend(self.ev[lo..hi].iter());
        let fp = fingerprint(&all);
        if self.day_fp.get(day) == Some(&fp) && path.exists() {
            return Ok(());
        }
        let tmp = self.dir.join(format!(".{day}.tmp"));
        {
            let f = File::create(&tmp)?;
            let mut gz = flate2::write::GzEncoder::new(std::io::BufWriter::new(f), flate2::Compression::fast());
            for e in &all {
                let d = DiskEv {
                    a: e.t0,
                    b: e.t1,
                    k: e.k.name().to_string(),
                    u: if e.u == NO_UID { None } else { Some(e.u) },
                    g: e.g.to_string(),
                    o: e.open,
                };
                writeln!(gz, "{}", serde_json::to_string(&d).unwrap_or_default())?;
            }
            gz.finish()?.flush()?;
        }
        std::fs::rename(&tmp, &path)?;
        self.day_fp.insert(day.to_string(), fp);
        Ok(())
    }

    /// Новый разбор буфера Android: всё начиная с его начала заменяется, раньше — остаётся.
    pub fn ingest(&mut self, mut dump: Dump) {
        if dump.records == 0 || dump.from == 0 {
            return;
        }
        if self.floor > 0 {
            let f = self.floor;
            dump.ev.retain(|e| e.t0 >= f);
            dump.from = dump.from.max(f);
        }
        let cut = dump.from;
        let mut keep: Vec<Ev> = self.ev.drain(..).filter(|e| e.t0 < cut).collect();
        let mut changed_days: BTreeSet<String> = BTreeSet::new();
        for e in keep.iter_mut().filter(|e| e.open) {
            // продолжение: в начале буфера Android перечисляет текущее состояние заново
            if let Some(pos) = dump.ev.iter().position(|d| d.same_key(e) && d.t0 <= cut + 2_000) {
                let d = dump.ev.remove(pos);
                e.t1 = d.t1.max(e.t1);
                e.open = d.open;
            } else if let Some(o) = dump.orphans.iter().find(|o| o.same_key(e)) {
                e.t1 = o.t1.max(e.t0);
                e.open = false;
            } else {
                e.open = false;
            }
            changed_days.insert(crate::store::day_of(e.t0));
        }
        let now = crate::util::real_ms();
        keep.extend(dump.ev);
        keep.sort_by_key(|e| (e.t0, e.t1));
        // теперь память верна начиная с min(старой границы, начала буфера); раньше — диск
        let authoritative = self.mem_from.min(cut);
        self.mem_from = authoritative;
        // держать в памяти от min(начало буфера, сейчас − 3 дня); старшее уйдёт на диск
        let mem_from = cut.min(now - MEM_MS).max(authoritative);
        let first_mem = keep.partition_point(|e| e.t0 < mem_from);
        // дни, которые уйдут из памяти, должны быть записаны до этого
        for e in &keep[..first_mem] {
            changed_days.insert(crate::store::day_of(e.t0));
        }
        self.ev = keep;
        let mut d = crate::store::day_of(cut);
        let today = crate::store::day_of(now);
        while d <= today {
            changed_days.insert(d.clone());
            let ds = day_start(&d);
            d = crate::store::day_of(ds + 36 * 3_600_000);
        }
        for day in &changed_days {
            if let Err(e) = self.write_day(day) {
                eprintln!("zhord: история {day} не записалась: {e}");
            }
        }
        self.ev.drain(..first_mem);
        self.ev.shrink_to_fit();
        self.mem_from = mem_from;
        self.covered_from = Some(self.covered_from.map(|c| c.min(cut)).unwrap_or(cut));
        self.dump_from = cut;
        self.dump_to = dump.to;
        self.prune_old();
    }

    /// Файлы старше keep_days (та же настройка, что у корзин).
    fn prune_old(&mut self) {
        let cut = crate::store::day_of(crate::util::real_ms() - self.keep_days * 86_400_000);
        for f in self.files() {
            if stem(&f) < cut {
                let _ = std::fs::remove_file(&f);
            }
        }
        if let Some(f) = self.files().first() {
            let first = self.read_file(f).first().map(|e| e.t0);
            if first.is_some() {
                self.covered_from = first;
            }
        }
    }

    /// Удаление по команде пользователя (`prune`): всё или раньше `before`. → (файлов, байт)
    pub fn prune(&mut self, before: Option<i64>) -> (u64, u64) {
        let (mut n, mut freed) = (0u64, 0u64);
        match before {
            None => {
                for f in self.files() {
                    freed += std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                    if std::fs::remove_file(&f).is_ok() {
                        n += 1;
                    }
                }
                self.ev.clear();
                self.covered_from = None;
            }
            Some(b) => {
                let bday = crate::store::day_of(b);
                for f in self.files() {
                    let day = stem(&f);
                    if day > bday {
                        continue;
                    }
                    let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                    if day < bday {
                        if std::fs::remove_file(&f).is_ok() {
                            n += 1;
                            freed += size;
                        }
                    } else {
                        // день на границе: оставить события с t0 ≥ before
                        let kept: Vec<Ev> = self.read_file(&f).into_iter().filter(|e| e.t0 >= b).collect();
                        self.ev.retain(|e| e.t0 >= b);
                        let save_mem = std::mem::replace(&mut self.ev, kept);
                        let save_from = self.mem_from;
                        self.mem_from = i64::MIN;
                        self.day_fp.remove(&day);
                        let _ = self.write_day(&day);
                        self.ev = save_mem;
                        self.mem_from = save_from;
                        let after = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                        freed += size.saturating_sub(after);
                    }
                }
                self.ev.retain(|e| e.t0 >= b);
                self.covered_from = self.covered_from.map(|c| c.max(b));
            }
        }
        self.day_fp.clear();
        self.floor = before.unwrap_or_else(crate::util::real_ms);
        let _ = std::fs::write(self.dir.join(".floor"), self.floor.to_string());
        (n, freed)
    }

    /// События, пересекающие [from, to]. Старше памяти — с диска.
    pub fn query(&mut self, from: i64, to: i64) -> Vec<Ev> {
        let mut out = Vec::new();
        if from < self.mem_from {
            let fd = crate::store::day_of(from - 86_400_000);
            let td = crate::store::day_of(self.mem_from);
            for f in self.files() {
                let day = stem(&f);
                if day < fd || day > td {
                    continue;
                }
                let mf = self.mem_from;
                out.extend(self.read_file(&f).into_iter().filter(|e| e.t0 < mf && e.t0 <= to && e.t1 >= from));
            }
        }
        let hi = self.ev.partition_point(|e| e.t0 <= to);
        out.extend(self.ev[..hi].iter().filter(|e| e.t1 >= from).cloned());
        out
    }

    /// Примерно сколько памяти занимают события (без строк тегов), байт.
    pub fn mem_bytes(&self) -> usize {
        self.ev.capacity() * std::mem::size_of::<Ev>() + self.strings.len() * 64
    }

    /// Для экрана «Данные»: (файлов, байт, событий в памяти, с какого момента).
    pub fn storage(&self) -> (u64, u64, usize, Option<i64>) {
        let files = self.files();
        let bytes = files.iter().map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).sum();
        (files.len() as u64, bytes, self.ev.len(), self.covered_from)
    }

    /// Для выгрузки raw: события интервала в виде JSON.
    pub fn export_rows(&mut self, from: i64, to: i64) -> Vec<serde_json::Value> {
        self.query(from, to)
            .into_iter()
            .map(|e| {
                serde_json::json!({"t0": e.t0, "t1": e.t1, "kind": e.k.name(),
                    "uid": if e.u == NO_UID { None } else { Some(e.u) }, "name": &*e.g, "open": e.open})
            })
            .collect()
    }
}

fn stem(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().trim_end_matches(".jsonl.gz").to_string()).unwrap_or_default()
}

// ───────────────────────── отчёт ─────────────────────────

#[derive(Debug, Clone, Serialize, Default)]
pub struct Lane {
    pub kind: &'static str,
    pub label: &'static str,
    /// событие всего телефона, а не приложения
    pub global: bool,
    /// число событий в окне
    pub count: u64,
    /// время в окне (пересечения склеены), мс
    pub ms: u64,
    /// [t0, t1, событий] — соседние события ближе ~1/400 окна склеены
    pub spans: Vec<(i64, i64, u32)>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HItem {
    pub kind: &'static str,
    pub label: &'static str,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub count: u64,
    /// сумма длительностей в окне, мс
    pub ms: u64,
    pub max_ms: u64,
    /// начало последнего
    pub last: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HEvRep {
    pub t0: i64,
    pub t1: i64,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub open: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HistRep {
    /// с какого момента внутри окна есть история Android
    pub from: i64,
    pub to: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    pub lanes: Vec<Lane>,
    pub items: Vec<HItem>,
    /// отдельные события: для приложения — всегда, для всего телефона — если окно ≤ 3 ч
    pub events: Vec<HEvRep>,
    pub events_total: usize,
    pub notes: Vec<String>,
}

const APP_LANES: [Kind; 11] = [Top, Fg, Job, Sync, Wakelock, WakelockIn, LongWake, TmpWhitelist, WakeupAp, Alarm, Proc];
const CONTEXT_LANES: [Kind; 5] = [Running, Screen, Gps, Radio, WifiScan];
const PHONE_LANES: [Kind; 11] = [Running, Screen, Doze, Gps, Radio, WifiScan, BleScan, Audio, Video, Camera, WakeReason];

fn clip(e: &Ev, from: i64, to: i64) -> i64 {
    (e.t1.min(to) - e.t0.max(from)).max(0)
}

fn lane(k: Kind, evs: &[&Ev], from: i64, to: i64) -> Lane {
    let mut l = Lane { kind: k.name(), label: k.label(), global: k.global(), ..Default::default() };
    let mut v: Vec<(i64, i64)> = evs.iter().map(|e| (e.t0.max(from), e.t1.min(to).max(e.t0.max(from)))).collect();
    v.sort();
    l.count = v.len() as u64;
    // время без двойного счёта пересечений
    let mut cur: Option<(i64, i64)> = None;
    for &(a, b) in &v {
        match cur {
            Some((ca, cb)) if a <= cb => cur = Some((ca, cb.max(b))),
            Some((ca, cb)) => {
                l.ms += (cb - ca) as u64;
                cur = Some((a, b));
            }
            None => cur = Some((a, b)),
        }
    }
    if let Some((a, b)) = cur {
        l.ms += (b - a) as u64;
    }
    let gap = ((to - from) / 400).max(1);
    let mut sp: Option<(i64, i64, u32)> = None;
    for &(a, b) in &v {
        match sp {
            Some((sa, sb, n)) if a - sb <= gap => sp = Some((sa, sb.max(b), n + 1)),
            Some(s) => {
                l.spans.push(s);
                sp = Some((a, b, 1));
            }
            None => sp = Some((a, b, 1)),
        }
    }
    if let Some(s) = sp {
        l.spans.push(s);
    }
    l
}

/// История за окно: дорожки, сводка по тегам и (если немного) отдельные события.
/// `uid` — одно приложение; без него — весь телефон.
pub fn report(st: &mut HistStore, from: i64, to: i64, uid: Option<u32>, label: &dyn Fn(u32) -> String) -> Option<HistRep> {
    let covered = st.covered_from?;
    if covered > to {
        return None;
    }
    let start = from.max(covered);
    let all = st.query(start, to);
    let mut rep = HistRep { from: start, to, uid, ..Default::default() };
    if covered > from + 5 * 60_000 {
        rep.notes.push(format!("История событий Android есть с {}.", crate::report::fmt_time(covered)));
    }
    let mine = |e: &&Ev| match uid {
        Some(u) => e.u == u && !e.k.global(),
        None => true,
    };
    // дорожки
    let kinds: Vec<Kind> = match uid {
        Some(_) => APP_LANES.iter().chain(CONTEXT_LANES.iter()).copied().collect(),
        None => PHONE_LANES.to_vec(),
    };
    for k in kinds {
        let evs: Vec<&Ev> = all.iter().filter(|e| e.k == k && (k.global() || mine(e))).collect();
        if !evs.is_empty() {
            rep.lanes.push(lane(k, &evs, start, to));
        }
    }
    // сводка по (вид, приложение, тег)
    let mut agg: HashMap<(Kind, u32, Arc<str>), HItem> = HashMap::new();
    for e in all.iter().filter(|e| (!e.k.global() || e.k == WakeReason) && (uid.is_none() || mine(e))) {
        let it = agg.entry((e.k, e.u, e.g.clone())).or_insert_with(|| HItem {
            kind: e.k.name(),
            label: e.k.label(),
            name: if e.g.is_empty() { "—".into() } else { e.g.to_string() },
            uid: (e.u != NO_UID).then_some(e.u),
            app: (e.u != NO_UID && uid.is_none()).then(|| label(e.u)),
            ..Default::default()
        });
        let ms = clip(e, start, to) as u64;
        it.count += 1;
        it.ms += ms;
        it.max_ms = it.max_ms.max(ms);
        it.last = it.last.max(e.t0);
    }
    let mut items: Vec<HItem> = agg.into_values().collect();
    items.sort_by(|a, b| b.ms.cmp(&a.ms).then(b.count.cmp(&a.count)));
    items.truncate(if uid.is_some() { 80 } else { 60 });
    rep.items = items;
    // отдельные события
    if uid.is_some() || to - start <= 3 * 3_600_000 {
        let mut evs: Vec<&Ev> = all.iter().filter(|e| (!e.k.global() || e.k == WakeReason) && mine(e)).collect();
        rep.events_total = evs.len();
        let cap = if uid.is_some() { 400 } else { 300 };
        if evs.len() > cap {
            // самые длинные, мгновенные — по порядку
            evs.sort_by_key(|e| std::cmp::Reverse(clip(e, start, to)));
            evs.truncate(cap);
        }
        evs.sort_by_key(|e| (e.t0, e.t1));
        rep.events = evs
            .into_iter()
            .map(|e| HEvRep {
                t0: e.t0,
                t1: e.t1,
                kind: e.k.name(),
                uid: (e.u != NO_UID).then_some(e.u),
                app: (e.u != NO_UID && uid.is_none()).then(|| label(e.u)),
                name: e.g.to_string(),
                open: e.open,
            })
            .collect();
    }
    if rep.lanes.is_empty() && rep.items.is_empty() {
        if let Some(u) = uid {
            rep.notes.push(format!("В истории Android за этот отрезок у uid {u} нет событий."));
        }
    }
    Some(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"9,0,i,vers,36,216,CP2A,CP2A
9,hsp,0,1000,"sensor:0x33"
9,hsp,20,10171,"com.miui.home"
9,h,0:TIME:1000000
9,h,0,Bl=95,+r,+w,+S,di=off
9,h,1,+Ejb=u0a253:"com.x/.Job,with,commas"
9,h,2,-w=u0a253:"*job*r/com.x"
9,h,3,+w=u0a358:"NotificationManagerService:post:com.example.vpn"
9,h,100,-w=u0a358:"NotificationManagerService:post:com.example.vpn",+Etp=20
9,h,10,-Ejb=u0a253:"com.x/.Job,with,commas",Ewa=u0a358:""
9,h,5,-r,wr=0:"Abort: Pending Wakeup Sources: [timerfd]",di=light
9,h,1000,+r,-Etp=20,+g,di=off
9,h,7,-Esy=u0a330:"com.example.mail"
9,h,0,+Etw=u10a439:"738004 c2dm/u10"
9,h,0:RESET:TIME:2000000
9,h,0,+r,+Elw=u10a201:"AudioMix"
9,h,50,-g
"#;

    #[test]
    fn uids() {
        assert_eq!(parse_uid("1000"), 1000);
        assert_eq!(parse_uid("u0a253"), 10_253);
        assert_eq!(parse_uid("u10a201"), 1_010_201);
        assert_eq!(parse_uid("u0i12"), 99_012);
        assert_eq!(parse_uid("u0ai3"), 90_003);
        assert_eq!(parse_uid("u10s1000"), 1_001_000);
        assert_eq!(parse_uid("-1"), NO_UID);
    }

    #[test]
    fn parse_sample() {
        let d = parse_checkin(SAMPLE);
        let find = |k: Kind| d.ev.iter().filter(|e| e.k == k).cloned().collect::<Vec<_>>();
        // задание с запятыми в теге: 1001 → 1116
        let jobs = find(Job);
        assert_eq!(jobs.len(), 1);
        assert_eq!(&*jobs[0].g, "com.x/.Job,with,commas");
        assert_eq!((jobs[0].t0, jobs[0].t1), (1_000_001, 1_000_116));
        assert_eq!(jobs[0].u, 10_253);
        // состояние wakelock: сначала без тега (с начала буфера), потом — приписано com.example.vpn
        let wl = find(Wakelock);
        assert_eq!(wl.len(), 2);
        assert_eq!((wl[1].t0, wl[1].t1, wl[1].u), (1_000_006, 1_000_106, 10_358));
        // на экране из пула
        let top = find(Top);
        assert_eq!((top[0].u, &*top[0].g, top[0].t0, top[0].t1), (10_171, "com.miui.home", 1_000_106, 1_001_121));
        // пробуждение сетью — мгновенное
        let wa = find(WakeupAp);
        assert_eq!((wa[0].t0, wa[0].t1, wa[0].u), (1_000_116, 1_000_116, 10_358));
        // причина пробуждения ядра
        assert!(find(WakeReason)[0].g.starts_with("Abort"));
        // doze light 1121..2121
        let dz = find(Doze);
        assert_eq!((dz[0].t0, dz[0].t1, &*dz[0].g), (1_000_121, 1_001_121, "light"));
        // конец синхронизации без начала — в orphans
        assert_eq!(d.orphans.iter().filter(|e| e.k == Sync).count(), 1);
        // сброс закрыл GPS и running; после сброса новые: длинный wakelock (u10) и GPS до конца
        let g = find(Gps);
        assert_eq!(g.len(), 1);
        assert_eq!((g[0].t0, g[0].t1), (1_001_121, 1_001_128));
        let lw = find(LongWake);
        assert_eq!((lw[0].u, lw[0].t0, lw[0].open), (1_010_201, 2_000_000, true));
        assert_eq!((d.from, d.to), (1_000_000, 2_000_050));
        // «разрешён фон» без конца: закрыт сбросом через ~0 мс — остаётся как есть (≤ 10 мин)
        let tw = find(TmpWhitelist);
        assert_eq!((tw[0].u, tw[0].t0, tw[0].t1), (1_010_439, 1_001_128, 1_001_128));
        let mut e = Ev { t0: 0, t1: 3_600_000, k: TmpWhitelist, u: 1, g: Arc::from(""), open: true };
        no_end(&mut e);
        assert_eq!((e.t1, e.open), (0, false));
    }

    #[test]
    fn lanes_merge() {
        let g: Arc<str> = Arc::from("x");
        let mk = |a, b| Ev { t0: a, t1: b, k: Job, u: 1, g: g.clone(), open: false };
        let v = [mk(0, 10), mk(5, 20), mk(30, 40), mk(10_000, 10_010)];
        let r: Vec<&Ev> = v.iter().collect();
        let l = lane(Job, &r, 0, 40_000);
        assert_eq!(l.count, 4);
        assert_eq!(l.ms, 20 + 10 + 10);
        // окно 40 с → склейка при разрыве ≤ 100 мс
        assert_eq!(l.spans, vec![(0, 40, 3), (10_000, 10_010, 1)]);
    }

    #[test]
    fn store_ingest_and_reload() {
        let dir = std::env::temp_dir().join(format!("zhor-hist-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let now = crate::util::real_ms();
        let mk = |t0: i64, t1: i64, k: Kind, open: bool| Ev { t0, t1, k, u: 10_001, g: Arc::from("tag"), open };
        let mut st = HistStore::open(dir.clone(), 10);
        // первый буфер: задание закончилось, GPS ещё работает
        let d1 = Dump {
            from: now - 3_600_000,
            to: now - 1_000_000,
            ev: vec![mk(now - 3_000_000, now - 2_900_000, Job, false), mk(now - 2_000_000, now - 1_000_000, Gps, true)],
            orphans: vec![],
            records: 10,
        };
        st.ingest(d1);
        assert_eq!(st.ev.len(), 2);
        // второй буфер начался позже начала GPS: GPS закончился (конец без начала)
        let d2 = Dump {
            from: now - 1_500_000,
            to: now,
            ev: vec![mk(now - 500_000, now - 400_000, Job, false)],
            orphans: vec![mk(now - 800_000, now - 800_000, Gps, false)],
            records: 10,
        };
        st.ingest(d2);
        let gps: Vec<&Ev> = st.ev.iter().filter(|e| e.k == Gps).collect();
        assert_eq!(gps.len(), 1);
        assert_eq!((gps[0].t1, gps[0].open), (now - 800_000, false));
        assert_eq!(st.ev.iter().filter(|e| e.k == Job).count(), 2);
        // перечитать с диска
        let mut st2 = HistStore::open(dir.clone(), 10);
        assert_eq!(st2.ev.len(), 3);
        assert_eq!(st2.covered_from, Some(now - 3_000_000));
        let q = st2.query(now - 600_000, now);
        assert_eq!(q.len(), 1);
        let rep = report(&mut st2, now - 3_600_000, now, Some(10_001), &|u| format!("app{u}")).unwrap();
        let job = rep.lanes.iter().find(|l| l.kind == "job").unwrap();
        assert_eq!((job.count, job.ms), (2, 200_000));
        assert_eq!(rep.events.len(), 2);
        assert!(rep.lanes.iter().any(|l| l.kind == "gps" && l.global));
        // очистка по команде
        let (n, _) = st2.prune(None);
        assert!(n >= 1);
        assert!(st2.ev.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
