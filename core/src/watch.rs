//! Сторож: правила по свежим корзинам ядра, тревога в момент события, доставка в приложение.
//!
//! Правила смотрят на текущий период «экран выключен, без зарядки» — непрерывный хвост корзин —
//! и срабатывают не больше раза за период на правило и виновника. Тревога пишется в alerts.jsonl
//! и уходит приложению явной рассылкой (`cmd activity broadcast`): уведомление показывает оно.
//! Проверка идёт только на шаге сбора, то есть пока телефон и так не спит, — сам сторож его не будит.

use crate::data::Config;
use crate::explain;
use crate::report::label_for;
use crate::store::Bucket;
use crate::sys::SysSnap;
use crate::util::{self, fmt_dur};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// Как часто проверять правила (по настенным часам, только на шаге сбора).
pub const EVAL_EVERY_MS: i64 = 5 * 60_000;
/// Корзины дальше друг от друга — разрыв в записи (ядро не работало): период обрывается.
const GAP_MS: i64 = 5 * 60_000;
/// Тревоги в памяти и на диске.
const KEEP_MS: i64 = 14 * 86_400_000;
const KEEP_N: usize = 300;

/// (0.8.3) Блокировки сна приложений за период — разностью снимков batterystats (см. [app_holds]).
/// Снимок «на начале периода» снимается по смене экрана, бывает на пару минут позже первой корзины.
const HOLD_BASE_SLACK_MS: i64 = 3 * 60_000;
/// Снимок раньше начала периода больше чем на столько — в разность попал бы включённый экран: не считаем.
const HOLD_BASE_AGE_MS: i64 = 10 * 60_000;
/// Последний снимок не старше этого — блокировки за период известны (почти) целиком.
const HOLD_FRESH_MS: i64 = 8 * 60_000;
/// Сколько «Телефон не спит» ждёт свежий снимок, прежде чем уйти без блокировок приложений.
const HOLD_WAIT_MS: i64 = 12 * 60_000;
/// Блокировки короче — не виновник.
const HOLD_MIN_MS: u64 = 60_000;
/// (0.8.3) «Частые пробуждения» от сети называют приложение, если входящих пакетов за период хотя бы
/// столько и его доля не меньше [NET_SHARE_MIN] (второе — от [NET_SHARE_NEXT]).
const NET_MIN_PACKETS: u64 = 100;
const NET_SHARE_MIN: f64 = 0.25;
const NET_SHARE_NEXT: f64 = 0.2;

/// (0.8.3) Что сторожу нужно помимо корзин.
#[derive(Debug, Default, Clone, Copy)]
pub struct Extra<'a> {
    /// снимки batterystats по времени — блокировки сна приложений
    pub snaps: &'a [&'a SysSnap],
    /// пакеты VPN-служб: их туннель в «кому шли пакеты» не считается — это те же пакеты приложений
    pub vpn: Option<&'a BTreeSet<String>>,
}

pub const APP_PKG: &str = "dev.niazlv.zhor";
pub const ALERT_ACTION: &str = "dev.niazlv.zhor.ALERT";
pub const ALERT_RECEIVER: &str = "dev.niazlv.zhor/.AlertReceiver";

/// Расход приложения с выключенным экраном.
#[derive(Debug, Clone, Default)]
pub struct AppOff {
    /// энергия CPU, мА·с
    pub mas: f64,
    pub cpu_ms: f64,
    pub starts: f64,
    /// процесс → мА·с
    pub procs: HashMap<String, f64>,
}

/// Счётчики источника пробуждения за период.
#[derive(Debug, Clone, Default)]
pub struct WsOff {
    pub wakeups: f64,
    pub held_ms: f64,
    pub prevent_ms: f64,
}

/// Сводка по корзинам с выключенным экраном и без зарядки (покой).
#[derive(Debug, Clone, Default)]
pub struct OffStats {
    pub from: i64,
    pub to: i64,
    /// время покоя и из него — в глубоком сне, мс
    pub off_ms: f64,
    pub susp_ms: f64,
    /// по кулоновскому счётчику, мкА·ч, и сколько времени он был
    pub cc_uah: f64,
    pub cc_ms: f64,
    /// интеграл тока по замерам (только бодрствование), мА·с — если счётчика нет
    pub cur_mas: f64,
    pub lvl_drop: f64,
    pub suspends: f64,
    pub fails: f64,
    pub temp_max: f64,
    /// модель CPU, мА·с
    pub cpu_mas: f64,
    pub ws: HashMap<String, WsOff>,
    pub apps: HashMap<u32, AppOff>,
}

impl OffStats {
    /// Добавить корзину покоя с долей пересечения `ov` (0..1). Остальные корзины пропускаются.
    pub fn add(&mut self, b: &Bucket, ov: f64) {
        if b.scr || b.plug || ov <= 0.0 {
            return;
        }
        if self.off_ms == 0.0 || b.t0 < self.from {
            self.from = b.t0;
        }
        self.to = self.to.max(b.t1);
        let span = b.span as f64 * ov;
        self.off_ms += span;
        self.susp_ms += b.susp as f64 * ov;
        if b.cc_ok {
            self.cc_uah += b.cc_uah as f64 * ov;
            self.cc_ms += span;
        }
        self.cur_mas += b.cur_mas * ov;
        self.lvl_drop += (b.lvl0 as f64 - b.lvl1 as f64) * ov;
        self.suspends += b.sok as f64 * ov;
        self.fails += b.sfail as f64 * ov;
        self.temp_max = self.temp_max.max(b.temp);
        self.cpu_mas += b.cpu_mas * ov;
        for w in &b.ws {
            let x = self.ws.entry(w.n.clone()).or_default();
            x.wakeups += w.wk as f64 * ov;
            x.held_ms += w.ms as f64 * ov;
            x.prevent_ms += w.pv as f64 * ov;
        }
        for p in &b.procs {
            // изолированные процессы — хозяину (рендеры WebView), как в отчёте
            let owner = p.h.unwrap_or(p.uid);
            let a = self.apps.entry(owner).or_default();
            a.mas += p.e * ov;
            a.cpu_ms += p.t as f64 * ov;
            a.starts += p.st as f64 * ov;
            *a.procs.entry(p.n.clone()).or_default() += p.e * ov;
        }
    }

    /// Сводка по отрезку [from, to] (корзины, пересекающие его, — по доле пересечения).
    pub fn over<'a>(buckets: impl IntoIterator<Item = &'a Bucket>, from: i64, to: i64) -> OffStats {
        let mut s = OffStats::default();
        for b in buckets {
            let len = (b.t1 - b.t0).max(1) as f64;
            let ov = ((b.t1.min(to) - b.t0.max(from)) as f64 / len).clamp(0.0, 1.0);
            s.add(b, ov);
        }
        s
    }

    pub fn hours(&self) -> f64 {
        self.off_ms / 3_600_000.0
    }

    pub fn deep_pct(&self) -> f64 {
        if self.off_ms > 0.0 { 100.0 * self.susp_ms / self.off_ms } else { 0.0 }
    }

    /// Израсходовано, мА·ч: счётчик, если он был почти всё время, иначе интеграл тока.
    pub fn mah(&self) -> f64 {
        if self.cc_ms > 0.0 && self.cc_ms >= 0.9 * self.off_ms {
            self.cc_uah / 1000.0
        } else {
            self.cur_mas / 3600.0
        }
    }

    /// Средний ток покоя, мА.
    pub fn avg_ma(&self) -> f64 {
        if self.off_ms > 0.0 { self.mah() / self.hours() } else { 0.0 }
    }

    /// Разряд, %/ч: по току и ёмкости, если она известна (уровень меняется шагом 1 %), иначе по уровню.
    pub fn pct_h(&self, full_mah: Option<f64>) -> f64 {
        let h = self.hours();
        if h <= 0.0 {
            return 0.0;
        }
        match full_mah.filter(|&c| c > 500.0) {
            Some(c) => 100.0 * self.mah() / c / h,
            None => self.lvl_drop / h,
        }
    }

    pub fn wakeups(&self) -> f64 {
        self.ws.values().map(|w| w.wakeups).sum()
    }

    /// Средний ток CPU приложения в покое, мА.
    pub fn app_ma(&self, uid: u32) -> f64 {
        let s = self.off_ms / 1000.0;
        if s <= 0.0 {
            return 0.0;
        }
        self.apps.get(&uid).map(|a| a.mas / s).unwrap_or(0.0)
    }

    /// Приложения (не система, не ядро, не бесхозные песочницы) по убыванию CPU в покое: (uid, мА).
    pub fn top_apps(&self, n: usize) -> Vec<(u32, f64)> {
        let s = (self.off_ms / 1000.0).max(0.001);
        let mut v: Vec<(u32, f64)> = self
            .apps
            .iter()
            .filter(|(u, _)| explain::uid_kind(**u) == "app")
            .map(|(u, a)| (*u, a.mas / s))
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(n);
        v
    }

    /// Все владельцы (вместе с системой и ядром) по убыванию CPU: (uid, мА).
    pub fn top_any(&self, n: usize) -> Vec<(u32, f64)> {
        let s = (self.off_ms / 1000.0).max(0.001);
        let mut v: Vec<(u32, f64)> = self.apps.iter().map(|(u, a)| (*u, a.mas / s)).collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(n);
        v
    }

    fn top_proc(&self, uid: u32) -> Option<(String, f64)> {
        let s = (self.off_ms / 1000.0).max(0.001);
        self.apps.get(&uid)?.procs.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(n, e)| (n.clone(), e / s))
    }

    /// Источник, дольше всех державший телефон без сна: (имя, мс).
    fn top_holder(&self) -> Option<(String, f64)> {
        self.ws
            .iter()
            .map(|(n, w)| (n, w.prevent_ms.max(w.held_ms)))
            .filter(|(_, ms)| *ms >= 60_000.0)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(n, ms)| (n.clone(), ms))
    }

    fn top_waker(&self) -> Option<(String, f64)> {
        // таймер AlarmManager «смена часов» (CLOCK_REALTIME + CANCEL_ON_SET) ядро отмечает на каждом выходе из сна
        // (timerfd_resume): его счётчик — эхо чужих пробуждений, не причина. Остальные [timerfd] будят по-настоящему.
        self.ws
            .iter()
            .filter(|(n, _)| !(n.starts_with("[timerfd]") && n.contains("смена часов")))
            .map(|(n, w)| (n, w.wakeups))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(n, c)| (n.clone(), c))
    }
}

/// Имя источника пробуждения без номера экземпляра: «[timerfd] #6» → «[timerfd]».
pub fn base_name(n: &str) -> &str {
    match n.rsplit_once(" #") {
        Some((b, num)) if !b.is_empty() && !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) => b,
        _ => n,
    }
}

/// Текущий период покоя: непрерывный хвост корзин с выключенным экраном и без зарядки.
/// → (начало периода, индекс его первой корзины). None — сейчас экран включён, телефон на зарядке
/// или хвост устарел.
pub fn current_period(buckets: &[&Bucket], now: i64) -> Option<(i64, usize)> {
    let last = buckets.last()?;
    if last.scr || last.plug || now - last.t1 > GAP_MS {
        return None;
    }
    let mut i = buckets.len() - 1;
    while i > 0 {
        let (p, c) = (buckets[i - 1], buckets[i]);
        if p.scr || p.plug || c.t0 - p.t1 > GAP_MS {
            break;
        }
        i -= 1;
    }
    Some((buckets[i].t0, i))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Alert {
    /// правило:виновник@начало периода — одна тревога на период
    pub id: String,
    pub t: i64,
    /// no_sleep | idle_drain | wakeups | app_bg | hot | usb_flap | test
    pub rule: String,
    /// ключ для «не напоминать»: правило или правило:виновник
    pub key: String,
    /// warn | bad | info
    pub level: String,
    pub title: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// пакет приложения из текста — приложение подставит вместо него человеческое имя
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pkg: Option<String>,
    /// период, о котором тревога (для «подробнее» — отчёт за этот отрезок)
    pub from: i64,
    pub to: i64,
    /// рассылка приложению прошла
    #[serde(default)]
    pub sent: bool,
}

pub struct Watch {
    dir: PathBuf,
    pub alerts: VecDeque<Alert>,
    fired: HashSet<String>,
    pub last_eval: i64,
    pub last_cost_us: u64,
    /// множитель минимальных сроков правил (1 — как есть; меньше — только для `watch-check --fast`)
    pub min_scale: f64,
    /// (0.8.3) «Телефон не спит» ждёт свежий снимок ради блокировок приложений (у прогона по истории — нет:
    /// новых снимков там не будет)
    defer: bool,
    /// (начало периода, когда попросили снимок) — ждём не дольше [HOLD_WAIT_MS]
    hold_wait: Option<(i64, i64)>,
    /// попросить у демона внеочередной снимок
    want_snap: bool,
}

fn ma_text(v: f64) -> String {
    if v >= 10.0 { format!("{v:.0}") } else { format!("{v:.1}") }
}

impl Watch {
    pub fn open(dir: PathBuf) -> Self {
        let mut w = Watch { dir, alerts: VecDeque::new(), fired: HashSet::new(), last_eval: 0, last_cost_us: 0, min_scale: 1.0, defer: true, hold_wait: None, want_snap: false };
        let cut = util::real_ms() - KEEP_MS;
        let mut dropped = false;
        if let Ok(f) = std::fs::File::open(w.path()) {
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                match serde_json::from_str::<Alert>(&line) {
                    // проверочные (их писали сборки до выпуска) и устаревшие — выбросить
                    Ok(a) if a.t >= cut && a.rule != "test" => {
                        w.fired.insert(a.id.clone());
                        w.alerts.push_back(a);
                    }
                    _ => dropped = true,
                }
            }
        }
        while w.alerts.len() > KEEP_N {
            w.alerts.pop_front();
            dropped = true;
        }
        if dropped {
            w.rewrite();
        }
        w
    }

    /// Сторож без журнала на диске — для прогона правил по записанной истории (`zhord watch-check`).
    pub fn scratch() -> Self {
        Watch { dir: PathBuf::from("/nonexistent"), alerts: VecDeque::new(), fired: HashSet::new(), last_eval: 0, last_cost_us: 0, min_scale: 1.0, defer: false, hold_wait: None, want_snap: false }
    }

    /// (0.8.3) Сторож просит внеочередной снимок batterystats (для блокировок сна в «Телефон не спит»).
    pub fn take_want_snap(&mut self) -> bool {
        std::mem::take(&mut self.want_snap)
    }

    fn path(&self) -> PathBuf {
        self.dir.join("alerts.jsonl")
    }

    pub fn due(&self, now: i64) -> bool {
        now - self.last_eval >= EVAL_EVERY_MS
    }

    /// Проверить правила по хвосту корзин. Возвращает новые тревоги (ещё не записанные).
    #[cfg(test)]
    pub fn eval(&mut self, buckets: &[&Bucket], cfg: &Config, full_mah: Option<f64>, pkgs: &BTreeMap<u32, Vec<String>>, now: i64) -> Vec<Alert> {
        self.eval_with(buckets, Extra::default(), cfg, full_mah, pkgs, now)
    }

    /// То же, со снимками batterystats (из них — блокировки сна приложений за период) и VPN-пакетами.
    pub fn eval_with(&mut self, buckets: &[&Bucket], x: Extra, cfg: &Config, full_mah: Option<f64>, pkgs: &BTreeMap<u32, Vec<String>>, now: i64) -> Vec<Alert> {
        self.last_eval = now;
        if !cfg.watch {
            return Vec::new();
        }
        let t0 = std::time::Instant::now();
        let out = match current_period(buckets, now) {
            Some((start, i)) => {
                let holds = app_holds(x.snaps, start, now);
                let mut out = rules(&buckets[i..], start, cfg, full_mah, pkgs, now, self.min_scale, &holds, x.vpn);
                // Снимок старый — блокировок приложений за период не видно. «Не спит» придержать до свежего
                // (его снимут в этом же шаге), но не дольше HOLD_WAIT_MS: снимок мог не получиться.
                if holds.wait && self.defer {
                    if let Some(pos) = out.iter().position(|a| a.rule == "no_sleep" && !self.fired.contains(&a.id)) {
                        let asked = match self.hold_wait {
                            Some((s, t)) if s == start => t,
                            _ => {
                                self.hold_wait = Some((start, now));
                                now
                            }
                        };
                        if now - asked < HOLD_WAIT_MS {
                            out.remove(pos);
                            self.want_snap = true;
                        }
                    }
                }
                out
            }
            None => Vec::new(),
        };
        let mute: HashSet<&str> = cfg.watch_mute.iter().map(|s| s.as_str()).collect();
        let fresh: Vec<Alert> = out
            .into_iter()
            .filter(|a| !mute.contains(a.key.as_str()) && !mute.contains(a.rule.as_str()))
            .filter(|a| self.fired.insert(a.id.clone()))
            .collect();
        self.last_cost_us = t0.elapsed().as_micros() as u64;
        fresh
    }

    /// (0.8.3) Тревога мимо правил покоя (мигание USB): сторож выключен, «не напоминать» или уже
    /// была — None.
    pub fn admit(&mut self, cfg: &Config, a: Alert) -> Option<Alert> {
        if !cfg.watch || cfg.watch_mute.iter().any(|m| *m == a.key || *m == a.rule) {
            return None;
        }
        self.fired.insert(a.id.clone()).then_some(a)
    }

    /// Записать тревогу (после попытки доставки).
    pub fn record(&mut self, a: Alert) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.path()) {
            let _ = writeln!(f, "{}", serde_json::to_string(&a).unwrap_or_default());
        }
        self.fired.insert(a.id.clone());
        self.alerts.push_back(a);
        let cut = util::real_ms() - KEEP_MS;
        let mut trimmed = false;
        while self.alerts.len() > KEEP_N || self.alerts.front().map(|a| a.t < cut).unwrap_or(false) {
            self.alerts.pop_front();
            trimmed = true;
        }
        if trimmed {
            self.rewrite();
        }
    }

    fn rewrite(&self) {
        let mut text = String::new();
        for a in &self.alerts {
            text += &serde_json::to_string(a).unwrap_or_default();
            text.push('\n');
        }
        let _ = std::fs::write(self.path(), text);
    }

    /// Очистка: всё или раньше `before`. → (файлов, байт)
    pub fn prune(&mut self, before: Option<i64>) -> (u64, u64) {
        let size = std::fs::metadata(self.path()).map(|m| m.len()).unwrap_or(0);
        match before {
            None => {
                self.alerts.clear();
                if std::fs::remove_file(self.path()).is_ok() {
                    return (1, size);
                }
                (0, 0)
            }
            Some(b) => {
                self.alerts.retain(|a| a.t >= b);
                self.rewrite();
                let after = std::fs::metadata(self.path()).map(|m| m.len()).unwrap_or(0);
                (0, size.saturating_sub(after))
            }
        }
    }
}

/// (0.8.3) Блокировки сна приложений за период покоя. В корзинах их нет: приложение держит процессор через
/// suspend-service («PowerManagerService.WakeLocks»), а не своим источником пробуждения ядра, — и сторож
/// винил Wi-Fi, пока почта держала телефон 9 минут из 35. Берём разность частичных wakelock по uid между
/// снимком у начала периода и последним.
#[derive(Debug, Clone, Default)]
pub struct AppHolds {
    /// последний снимок старый — свежий покажет блокировки за период целиком
    pub wait: bool,
    /// (uid, тег, мс) по убыванию. На uid — самый долгий тег, не сумма: теги одного приложения обычно
    /// вложены (задача JobScheduler и wakelock внутри неё), сумма их удвоила бы.
    pub top: Vec<(u32, String, u64)>,
}

pub fn app_holds(snaps: &[&SysSnap], start: i64, now: i64) -> AppHolds {
    let Some(bi) = snaps.iter().rposition(|s| s.t <= start + HOLD_BASE_SLACK_MS) else {
        return AppHolds::default();
    };
    let (base, last) = (snaps[bi], snaps[snaps.len() - 1]);
    // снимка у начала периода нет — разность была бы не про покой, и свежий снимок этого не исправит
    if base.t < start - HOLD_BASE_AGE_MS {
        return AppHolds::default();
    }
    let wait = now - last.t > HOLD_FRESH_MS;
    if last.t <= base.t {
        return AppHolds { wait, top: Vec::new() };
    }
    // Android обнулил статистику (перезагрузка, отключение после полной зарядки) — разность бессмысленна
    if last.checkin.start_clock_ms != base.checkin.start_clock_ms {
        return AppHolds::default();
    }
    let mut top: Vec<(u32, String, u64)> = last
        .checkin
        .uids
        .iter()
        .filter_map(|(uid, u)| {
            let old = base.checkin.uids.get(uid);
            let d: Vec<(&String, u64)> = u.wakelocks.iter().map(|(n, w)| (n, w.ms.saturating_sub(old.and_then(|o| o.wakelocks.get(n)).map(|o| o.ms).unwrap_or(0)))).collect();
            let (n, ms) = d.iter().max_by_key(|x| x.1).copied().filter(|x| x.1 >= HOLD_MIN_MS)?;
            // задача или синхронизация почти той же длины говорит больше, чем «App:WakeLock» внутри неё
            let named = d.iter().filter(|x| (x.0.starts_with("*job*") || x.0.starts_with("*sync*")) && x.1 * 10 >= ms * 9).max_by_key(|x| x.1).map(|x| x.0).unwrap_or(n);
            Some((*uid, named.clone(), ms))
        })
        .collect();
    top.sort_by_key(|x| std::cmp::Reverse(x.2));
    top.truncate(3);
    AppHolds { wait, top }
}

/// Тег блокировки сна покороче: «*job*r/pkg/.mail.ews.push.EwsPushJobService» → «задача EwsPushJobService»,
/// «*job*r/#UpdateTask#@androidx.work…» → «задача UpdateTask», «*sync*/…» → «синхронизация».
fn wl_tag(tag: &str) -> String {
    if let Some(rest) = tag.strip_prefix("*job*") {
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_lowercase()).trim_start_matches('/');
        let name = match rest.strip_prefix('#') {
            Some(r) => r.split('#').next().unwrap_or(r),
            None => rest.rsplit(['/', '.']).find(|p| !p.is_empty()).unwrap_or(rest),
        };
        return format!("задача {name}");
    }
    if tag.starts_with("*sync*") {
        return "синхронизация".into();
    }
    if tag.starts_with("*alarm*") {
        return "будильник".into();
    }
    tag.chars().take(48).collect()
}

/// (0.8.3) Кому шли входящие пакеты за период: (uid, пакетов, байт) по убыванию и всего пакетов.
/// VPN-службы не в счёт: их туннель — те же пакеты приложений (как в «Кто будит по сети» отчёта).
fn net_top(period: &[&Bucket], pkgs: &BTreeMap<u32, Vec<String>>, vpn: Option<&BTreeSet<String>>) -> (Vec<(u32, u64, u64)>, u64) {
    let is_vpn = |uid: u32| vpn.zip(pkgs.get(&uid)).map(|(v, ps)| ps.iter().any(|p| v.contains(p))).unwrap_or(false);
    let mut m: HashMap<u32, (u64, u64)> = HashMap::new();
    for b in period.iter().filter(|b| !b.scr && !b.plug) {
        for r in &b.net {
            let e = m.entry(r.u).or_default();
            e.0 += r.rp;
            e.1 += r.rb;
        }
    }
    let mut v: Vec<(u32, u64, u64)> = m.into_iter().filter(|(u, _)| !is_vpn(*u)).map(|(u, (p, b))| (u, p, b)).collect();
    let total = v.iter().map(|x| x.1).sum();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    (v, total)
}

/// Правила по текущему периоду покоя.
#[allow(clippy::too_many_arguments)]
fn rules(period: &[&Bucket], start: i64, cfg: &Config, full_mah: Option<f64>, pkgs: &BTreeMap<u32, Vec<String>>, now: i64, scale: f64, holds: &AppHolds, vpn: Option<&BTreeSet<String>>) -> Vec<Alert> {
    let st = OffStats::over(period.iter().copied(), i64::MIN, i64::MAX);
    let mut out = Vec::new();
    let off = st.off_ms as u64;
    // минимальная длительность покоя для правила, мс
    let min = |minutes: u64| (minutes as f64 * 60_000.0 * scale) as u64;
    let label = |uid: u32| label_for(uid, pkgs).0;
    let pkg_of = |uid: u32| pkgs.get(&uid).and_then(|p| p.first().cloned());
    let mk = |rule: &str, culprit: Option<&str>, level: &str, title: String, text: String, uid: Option<u32>| {
        let key = match culprit {
            Some(c) => format!("{rule}:{c}"),
            None => rule.to_string(),
        };
        Alert {
            id: format!("{key}@{start}"),
            t: now,
            rule: rule.into(),
            key,
            level: level.into(),
            title,
            text,
            uid,
            pkg: uid.and_then(pkg_of),
            from: start,
            to: now,
            sent: false,
        }
    };
    // кто больше всех тратил процессор в покое — подсказка к правилам о сне и расходе
    let top_cpu = || {
        st.top_any(1).into_iter().find(|(_, ma)| *ma >= 1.0).map(|(u, ma)| (u, format!(" Больше всех процессора — {}: ~{} мА.", label(u), ma_text(ma))))
    };

    // 1. телефон не спит
    if cfg.watch_sleep_pct > 0.0 && off >= min(30) && st.deep_pct() < cfg.watch_sleep_pct {
        let deep = st.deep_pct();
        let mut text = format!("Экран выключен {}, а в глубоком сне телефон провёл {:.0} % этого времени.", fmt_dur(off), deep);
        // (0.8.3) источник ядра или блокировка сна приложения — кто дольше
        let mut culprit = None;
        match (holds.top.first(), st.top_holder()) {
            (Some((u, tag, ms)), k) if k.as_ref().map(|(_, kms)| *ms as f64 >= *kms).unwrap_or(true) => {
                text += &format!(" Дольше всех не даёт спать {}: блокировка сна «{}» ({}).", label(*u), wl_tag(tag), fmt_dur(*ms));
                culprit = Some(*u);
            }
            (app, Some((n, kms))) => {
                text += &format!(" Дольше всех не даёт спать «{n}» ({}).", fmt_dur(kms as u64));
                if let Some((u, tag, ms)) = app {
                    text += &format!(" Из приложений дольше всех держит блокировку сна {}: «{}» ({}).", label(*u), wl_tag(tag), fmt_dur(*ms));
                    culprit = Some(*u);
                }
            }
            _ => {}
        }
        let top = top_cpu();
        if let Some((_, t)) = &top {
            text += t;
        }
        let lvl = if deep < cfg.watch_sleep_pct / 2.0 { "bad" } else { "warn" };
        let is_app = |u: &u32| explain::uid_kind(*u) == "app";
        let uid = culprit.filter(is_app).or(top.map(|(u, _)| u).filter(is_app));
        out.push(mk("no_sleep", None, lvl, "Телефон не спит".into(), text, uid));
    }

    // 2. батарея тает в покое
    let pct_h = st.pct_h(full_mah);
    if cfg.watch_drain_pct_h > 0.0 && off >= min(60) && pct_h > cfg.watch_drain_pct_h {
        let mut text = format!(
            "С выключенным экраном уходит {:.1} %/ч (~{} мА) уже {}: {:.0} мА·ч. Норма — меньше 1 %/ч.",
            pct_h,
            ma_text(st.avg_ma()),
            fmt_dur(off),
            st.mah()
        );
        let top = top_cpu();
        if let Some((_, t)) = &top {
            text += t;
        }
        let lvl = if pct_h > 2.0 * cfg.watch_drain_pct_h { "bad" } else { "warn" };
        out.push(mk("idle_drain", None, lvl, "Батарея тает в покое".into(), text, top.map(|(u, _)| u).filter(|u| explain::uid_kind(*u) == "app")));
    }

    // 3. частые пробуждения
    if cfg.watch_wakeups_h > 0.0 && off >= min(30) {
        if let Some((n, c)) = st.top_waker() {
            let per_h = c / st.hours().max(0.01);
            if c >= 50.0 && per_h > cfg.watch_wakeups_h {
                let why = explain::wakeup_source(&n).map(|e| format!(" {e}")).unwrap_or_default();
                let mut text = format!("«{n}» будит телефон ~{per_h:.0} раз в час ({:.0} за {}).{why}", c, fmt_dur(off));
                // (0.8.3) будит сеть — назвать, кому шли пакеты, а не отсылать к экрану «Сон»
                let mut uid = None;
                if crate::report::net_ws_kind(&n).is_some() {
                    let (top, total) = net_top(period, pkgs, vpn);
                    let share = |p: u64| p as f64 / total.max(1) as f64;
                    if let Some(&(u, p, bytes)) = top.first().filter(|x| total >= NET_MIN_PACKETS && share(x.1) >= NET_SHARE_MIN) {
                        text += &format!(
                            " Больше всех входящих пакетов получило {}: {:.0} % (~{:.0} в час, {}).",
                            label(u),
                            100.0 * share(p),
                            p as f64 / st.hours().max(0.01),
                            util::fmt_bytes(bytes)
                        );
                        if let Some(&(u2, p2, _)) = top.get(1).filter(|x| share(x.1) >= NET_SHARE_NEXT) {
                            text += &format!(" Следом — {}: {:.0} %.", label(u2), 100.0 * share(p2));
                        }
                        uid = Some(u).filter(|u| explain::uid_kind(*u) == "app");
                    }
                }
                let lvl = if per_h > 4.0 * cfg.watch_wakeups_h { "bad" } else { "warn" };
                // «[timerfd] #6» → ключ «[timerfd]»: номер экземпляра меняется после перезагрузки
                out.push(mk("wakeups", Some(base_name(&n)), lvl, format!("Частые пробуждения: {n}"), text, uid));
            }
        }
    }

    // 4. приложение работает в фоне
    if cfg.watch_app_ma > 0.0 && off >= min(60) {
        if let Some((uid, ma)) = st.top_apps(1).into_iter().next() {
            if ma > cfg.watch_app_ma {
                let l = label(uid);
                let mut text = format!(
                    "С выключенным экраном ({}) {} тратит процессор ~{} мА в среднем: {:.1} мА·ч.",
                    fmt_dur(off),
                    l,
                    ma_text(ma),
                    ma * st.hours()
                );
                if let Some((p, pma)) = st.top_proc(uid) {
                    if pma >= 0.5 {
                        text += &format!(" Больше всех — процесс {p} (~{} мА).", ma_text(pma));
                    }
                }
                let lvl = if ma > 3.0 * cfg.watch_app_ma { "bad" } else { "warn" };
                out.push(mk("app_bg", Some(&uid.to_string()), lvl, format!("Работает в фоне: {l}"), text, Some(uid)));
            }
        }
    }

    // 5. греется в покое (батарея горячая всё последние 10 минут)
    if cfg.watch_hot_c > 0.0 && off >= min(10) {
        let recent: Vec<&Bucket> = period.iter().copied().filter(|b| b.t1 >= now - 10 * 60_000).collect();
        let covered: u64 = recent.iter().map(|b| b.span).sum();
        let tmin = recent.iter().map(|b| b.temp).fold(f64::INFINITY, f64::min);
        if covered >= 5 * 60_000 && tmin.is_finite() && tmin >= cfg.watch_hot_c {
            let rs = OffStats::over(recent.iter().copied(), i64::MIN, i64::MAX);
            let tmax = recent.iter().map(|b| b.temp).fold(f64::MIN, f64::max);
            let mut text = format!("Батарея {tmax:.1} °C, хотя экран выключен {} — телефон работает в кармане.", fmt_dur(off));
            let top = rs.top_any(1).into_iter().find(|(_, ma)| *ma >= 5.0);
            if let Some((u, ma)) = top {
                text += &format!(" Последние 10 минут больше всех процессора — {}: ~{} мА.", label(u), ma_text(ma));
            }
            out.push(mk("hot", None, "warn", "Греется с выключенным экраном".into(), text, top.map(|(u, _)| u).filter(|u| explain::uid_kind(*u) == "app")));
        }
    }
    out
}

/// (0.8.3) «USB-порт мигает». `now_us` и `f.first_us` — время с загрузки по часам ядра, мкс.
pub fn usb_flap_alert(f: &crate::usbport::Flap, now: i64, now_us: u64, boot: &str) -> Alert {
    let from = now - (now_us.saturating_sub(f.first_us) / 1000) as i64;
    // ядро пишет время с загрузки: мигание в первые минуты — сразу после перезагрузки
    let after_boot = if f.first_us < 5 * 60 * 1_000_000 { " сразу после загрузки телефона" } else { "" };
    let text = format!(
        "Разъём USB-C {} раз за {} включил режим USB-хоста и тут же выключил{after_boot}. Так не ведёт себя ни зарядка, \
         ни подключённое устройство: обычно это влага или грязь в разъёме, повреждённый кабель или переходник. Пока порт \
         мигает, телефон не засыпает, а на некоторых моделях через несколько минут случается паника ядра и перезагрузка. \
         Отключите кабель и переходники, просушите и прочистите разъём — не металлом.",
        f.n,
        fmt_dur((f.span_us / 1000).max(1000))
    );
    Alert {
        id: format!("usb_flap@{boot}:{}", f.first_seq),
        t: now,
        rule: "usb_flap".into(),
        key: "usb_flap".into(),
        level: "bad".into(),
        title: "USB-порт мигает".into(),
        text,
        uid: None,
        pkg: None,
        from,
        to: now,
        sent: false,
    }
}

/// Отправить тревогу приложению. → (доставлено, вывод cmd)
pub fn deliver(a: &Alert) -> (bool, String) {
    let (from, to, t) = (a.from.to_string(), a.to.to_string(), a.t.to_string());
    let uid = a.uid.map(|u| u.to_string());
    let mut args: Vec<&str> = vec![
        "activity", "broadcast", "--user", "0", "--include-stopped-packages",
        "-a", ALERT_ACTION, "-n", ALERT_RECEIVER,
        "--es", "id", &a.id, "--es", "rule", &a.rule, "--es", "key", &a.key, "--es", "level", &a.level,
        "--es", "title", &a.title, "--es", "text", &a.text,
        "--el", "from", &from, "--el", "to", &to, "--el", "t", &t,
    ];
    if let Some(u) = &uid {
        args.extend(["--ei", "uid", u.as_str()]);
    }
    if let Some(p) = &a.pkg {
        args.extend(["--es", "pkg", p.as_str()]);
    }
    let out = util::run_cmd("cmd", &args, 15_000).unwrap_or_default();
    let ok = out.contains("Broadcast completed") || out.contains("Broadcast sent");
    (ok, out.trim().to_string())
}

/// Проверочная тревога: как настоящая, но без правила.
pub fn test_alert(now: i64) -> Alert {
    Alert {
        id: format!("test@{now}"),
        t: now,
        rule: "test".into(),
        key: "test".into(),
        level: "info".into(),
        title: "Сторож на связи".into(),
        text: "Проверочное уведомление: ядро достучалось до приложения. Настоящие тревоги придут так же.".into(),
        from: now - 3_600_000,
        to: now,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ProcRec, WsRec};

    fn b(t0: i64, scr: bool, susp_frac: f64, cc_uah: i64) -> Bucket {
        let span = 60_000u64;
        let susp = (span as f64 * susp_frac) as u64;
        Bucket { t0, t1: t0 + 60_000, span, awake: span - susp, susp, scr, cc_uah, cc_ok: true, temp: 30.0, ..Default::default() }
    }

    fn cfg() -> Config {
        Config::default()
    }

    fn refs(v: &[Bucket]) -> Vec<&Bucket> {
        v.iter().collect()
    }

    #[test]
    fn period_is_tail_of_quiet_buckets() {
        let mut v = vec![b(0, true, 0.0, 0)];
        for i in 1..=40 {
            v.push(b(i * 60_000, false, 0.9, 100));
        }
        let now = 41 * 60_000;
        let (start, i) = current_period(&refs(&v), now).unwrap();
        assert_eq!(start, 60_000);
        assert_eq!(v.len() - i, 40);
        // экран включён сейчас — периода нет
        v.push(b(41 * 60_000, true, 0.0, 0));
        assert!(current_period(&refs(&v), 42 * 60_000).is_none());
        // разрыв записи обрывает период
        let v2 = vec![b(0, false, 0.9, 0), b(20 * 60_000, false, 0.9, 0)];
        assert_eq!(current_period(&refs(&v2), 21 * 60_000).unwrap().0, 20 * 60_000);
    }

    #[test]
    fn no_sleep_fires_once_per_period_and_respects_mute() {
        let mut v = Vec::new();
        for i in 0..40 {
            let mut x = b(i * 60_000, false, 0.2, 200);
            x.ws.push(WsRec { n: "qcom_rx_wakelock".into(), ac: 100, wk: 100, ms: 30_000, pv: 30_000 });
            v.push(x);
        }
        let now = 40 * 60_000;
        let mut w = Watch { dir: std::env::temp_dir(), ..Watch::scratch() };
        let pk = BTreeMap::new();
        let a = w.eval(&refs(&v), &cfg(), Some(5000.0), &pk, now);
        let rules: Vec<&str> = a.iter().map(|a| a.rule.as_str()).collect();
        assert!(rules.contains(&"no_sleep"), "{rules:?}");
        // 100 пробуждений в минуту = 6000/ч > 600
        assert!(rules.contains(&"wakeups"), "{rules:?}");
        let ns = a.iter().find(|a| a.rule == "no_sleep").unwrap();
        assert!(ns.text.contains("qcom_rx_wakelock"), "{}", ns.text);
        assert_eq!(base_name("[timerfd] #6"), "[timerfd]");
        assert_eq!(base_name("qcom_rx_wakelock"), "qcom_rx_wakelock");
        assert_eq!(base_name("x #a1"), "x #a1");
        assert_eq!(ns.from, 0);
        // второй раз в том же периоде — тишина
        assert!(w.eval(&refs(&v), &cfg(), Some(5000.0), &pk, now + 60_000).is_empty());
        // приглушённый ключ не срабатывает и в новом периоде
        let mut c = cfg();
        c.watch_mute = vec!["wakeups:qcom_rx_wakelock".into()];
        let shifted: Vec<Bucket> = v.iter().map(|x| Bucket { t0: x.t0 + 10_000_000, t1: x.t1 + 10_000_000, ..x.clone() }).collect();
        let a2 = w.eval(&refs(&shifted), &c, Some(5000.0), &pk, now + 10_000_000);
        assert!(a2.iter().all(|a| a.rule != "wakeups"));
        assert!(a2.iter().any(|a| a.rule == "no_sleep"));
    }

    /// Снимок с частичными wakelock: [(uid, тег, мс)].
    fn snap(t: i64, start_clock: i64, wl: &[(u32, &str, u64)]) -> SysSnap {
        let mut s = SysSnap { t, ..Default::default() };
        s.checkin.start_clock_ms = start_clock;
        for (uid, tag, ms) in wl {
            s.checkin.uids.entry(*uid).or_default().wakelocks.insert(tag.to_string(), crate::sys::checkin::Wl { ms: *ms, ..Default::default() });
        }
        s
    }

    const JOB: &str = "*job*r/com.example.mail/.mail.ews.push.EwsPushJobService";

    #[test]
    fn app_holds_takes_longest_tag_per_app() {
        let base = snap(60_000, 1, &[(10_330, JOB, 1_000), (10_330, "Mail:WakeLock", 1_000), (10_500, "*sync*/x", 0)]);
        let last = snap(40 * 60_000, 1, &[(10_330, JOB, 541_000), (10_330, "Mail:WakeLock", 539_000), (10_500, "*sync*/x", 30_000)]);
        let h = app_holds(&[&base, &last], 0, 41 * 60_000);
        assert!(!h.wait);
        // вложенные теги не складываются; меньше минуты — не виновник
        assert_eq!(h.top, vec![(10_330, JOB.to_string(), 540_000)]);
        // последний снимок старый — ждать свежий
        assert!(app_holds(&[&base, &last], 0, 60 * 60_000).wait);
        // только снимок у начала — ждать; разность ещё пустая
        let h1 = app_holds(&[&base], 0, 30 * 60_000);
        assert!(h1.wait && h1.top.is_empty());
        // свой wakelock чуть длиннее задачи — время его, а имя задачи (понятнее)
        let last_w = snap(40 * 60_000, 1, &[(10_330, JOB, 538_000), (10_330, "Mail:WakeLock", 541_000)]);
        assert_eq!(app_holds(&[&base, &last_w], 0, 41 * 60_000).top, vec![(10_330, JOB.to_string(), 540_000)]);
        // Android обнулил статистику — ни ждать, ни считать
        let reset = snap(40 * 60_000, 2, &[(10_330, JOB, 5_000)]);
        let h2 = app_holds(&[&base, &reset], 0, 41 * 60_000);
        assert!(!h2.wait && h2.top.is_empty());
        // снимка у начала периода нет (последний перед ним — на 20 мин раньше)
        let h3 = app_holds(&[&base, &last], 21 * 60_000, 41 * 60_000);
        assert!(!h3.wait && h3.top.is_empty());
        assert_eq!(wl_tag(JOB), "задача EwsPushJobService");
        assert_eq!(wl_tag("*job*r/#SubscriptionUpdater$UpdateTask#@androidx.work.systemjobscheduler/com.example/x"), "задача SubscriptionUpdater$UpdateTask");
        assert_eq!(wl_tag("*sync*/gmail-ls/com.google"), "синхронизация");
        assert_eq!(wl_tag("Mail:WakeLock"), "Mail:WakeLock");
    }

    #[test]
    fn no_sleep_names_app_holding_wakelock() {
        let mut pk = BTreeMap::new();
        pk.insert(10_330u32, vec!["com.example.mail".to_string()]);
        // сон 20 %, Wi-Fi держит 30 с из каждой минуты = 20 мин за 40; почта — 30 мин
        let mut v = Vec::new();
        for i in 0..40 {
            let mut x = b(i * 60_000, false, 0.2, 200);
            x.ws.push(WsRec { n: "qcom_rx_wakelock".into(), ac: 100, wk: 100, ms: 30_000, pv: 30_000 });
            v.push(x);
        }
        let now = 40 * 60_000;
        let base = snap(30_000, 1, &[(10_330, JOB, 0)]);
        let last = snap(now - 60_000, 1, &[(10_330, JOB, 30 * 60_000)]);
        let mut w = Watch { dir: std::env::temp_dir(), defer: true, ..Watch::scratch() };
        let a = w.eval_with(&refs(&v), Extra { snaps: &[&base, &last], vpn: None }, &cfg(), Some(5000.0), &pk, now);
        let ns = a.iter().find(|a| a.rule == "no_sleep").expect("no_sleep");
        assert!(ns.text.contains("не даёт спать com.example.mail: блокировка сна «задача EwsPushJobService» (30 мин 00 с)"), "{}", ns.text);
        assert!(!ns.text.contains("qcom_rx_wakelock"), "{}", ns.text);
        assert_eq!(ns.uid, Some(10_330));
        assert_eq!(ns.pkg.as_deref(), Some("com.example.mail"));
        assert!(!w.take_want_snap());

        // почта держала меньше Wi-Fi — назвать обоих
        let last2 = snap(now - 60_000, 1, &[(10_330, JOB, 5 * 60_000)]);
        let mut w2 = Watch::scratch();
        let a2 = w2.eval_with(&refs(&v), Extra { snaps: &[&base, &last2], vpn: None }, &cfg(), Some(5000.0), &pk, now);
        let ns2 = a2.iter().find(|a| a.rule == "no_sleep").expect("no_sleep");
        assert!(ns2.text.contains("«qcom_rx_wakelock» (20 мин 00 с)"), "{}", ns2.text);
        assert!(ns2.text.contains("Из приложений дольше всех держит блокировку сна com.example.mail"), "{}", ns2.text);
        assert_eq!(ns2.uid, Some(10_330));
    }

    #[test]
    fn no_sleep_waits_for_fresh_snapshot() {
        let mut pk = BTreeMap::new();
        pk.insert(10_330u32, vec!["com.example.mail".to_string()]);
        let mk_v = |n: i64| -> Vec<Bucket> { (0..n).map(|i| b(i * 60_000, false, 0.2, 200)).collect() };
        let base = snap(30_000, 1, &[(10_330, JOB, 0)]);
        let mut w = Watch { dir: std::env::temp_dir(), defer: true, ..Watch::scratch() };
        // снимок только у начала — придержать и попросить свежий
        let v = mk_v(40);
        let a = w.eval_with(&refs(&v), Extra { snaps: &[&base], vpn: None }, &cfg(), Some(5000.0), &pk, 40 * 60_000);
        assert!(a.iter().all(|a| a.rule != "no_sleep"));
        assert!(w.take_want_snap());
        // свежий снят — уходит с почтой
        let v = mk_v(45);
        let fresh = snap(41 * 60_000, 1, &[(10_330, JOB, 25 * 60_000)]);
        let a = w.eval_with(&refs(&v), Extra { snaps: &[&base, &fresh], vpn: None }, &cfg(), Some(5000.0), &pk, 45 * 60_000);
        let ns = a.iter().find(|a| a.rule == "no_sleep").expect("no_sleep");
        assert!(ns.text.contains("com.example.mail"), "{}", ns.text);

        // снимок так и не появился — через HOLD_WAIT_MS уходит без него
        let mut w = Watch { dir: std::env::temp_dir(), defer: true, ..Watch::scratch() };
        let v = mk_v(40);
        assert!(w.eval_with(&refs(&v), Extra { snaps: &[&base], vpn: None }, &cfg(), Some(5000.0), &pk, 40 * 60_000).iter().all(|a| a.rule != "no_sleep"));
        let v = mk_v(45);
        assert!(w.eval_with(&refs(&v), Extra { snaps: &[&base], vpn: None }, &cfg(), Some(5000.0), &pk, 45 * 60_000).iter().all(|a| a.rule != "no_sleep"));
        let n = 40 + HOLD_WAIT_MS / 60_000;
        let v = mk_v(n);
        let a = w.eval_with(&refs(&v), Extra { snaps: &[&base], vpn: None }, &cfg(), Some(5000.0), &pk, n * 60_000);
        assert!(a.iter().any(|a| a.rule == "no_sleep"));
    }

    #[test]
    fn wakeups_from_network_name_receiver() {
        use crate::store::NetRec;
        let mut pk = BTreeMap::new();
        pk.insert(10_439u32, vec!["com.example.chat".to_string()]);
        pk.insert(10_399u32, vec!["com.example.bank".to_string()]);
        pk.insert(10_358u32, vec!["com.example.vpn".to_string()]);
        let vpn: BTreeSet<String> = ["com.example.vpn".to_string()].into();
        let nr = |u: u32, rp: u64| NetRec { u, rp, rb: rp * 600, ..Default::default() };
        let mk_v = |net: Vec<Vec<NetRec>>| -> Vec<Bucket> {
            (0..40)
                .map(|i| {
                    let mut x = b(i * 60_000, false, 0.9, 50);
                    x.ws.push(WsRec { n: "qcom_rx_wakelock".into(), ac: 30, wk: 30, ms: 1_000, pv: 1_000 });
                    if i % 10 == 9 {
                        x.net = net[(i / 10) as usize].clone();
                    }
                    x
                })
                .collect()
        };
        // чат 700 пакетов из 1000 (туннель VPN — 900 — не в счёт), банк 250
        let q = vec![nr(10_439, 175), nr(10_399, 62), nr(10_358, 225), nr(1051, 13)];
        let v = mk_v(vec![q.clone(), q.clone(), q.clone(), q]);
        let mut w = Watch::scratch();
        let a = w.eval_with(&refs(&v), Extra { snaps: &[], vpn: Some(&vpn) }, &cfg(), Some(5000.0), &pk, 40 * 60_000);
        let wk = a.iter().find(|a| a.rule == "wakeups").expect("wakeups");
        assert!(wk.text.contains("Больше всех входящих пакетов получило com.example.chat: 70 % (~1050 в час"), "{}", wk.text);
        assert!(wk.text.contains("Следом — com.example.bank: 25 %."), "{}", wk.text);
        assert!(!wk.text.contains("com.example.vpn"), "{}", wk.text);
        assert_eq!(wk.uid, Some(10_439));
        assert_eq!(wk.pkg.as_deref(), Some("com.example.chat"));
        assert_eq!(wk.key, "wakeups:qcom_rx_wakelock");

        // пакеты размазаны — никого не называть
        let flat: Vec<NetRec> = (0..6).map(|k| nr(10_400 + k, 50)).collect();
        let v = mk_v(vec![flat.clone(), flat.clone(), flat.clone(), flat]);
        let mut w = Watch::scratch();
        let a = w.eval_with(&refs(&v), Extra { snaps: &[], vpn: Some(&vpn) }, &cfg(), Some(5000.0), &pk, 40 * 60_000);
        let wk = a.iter().find(|a| a.rule == "wakeups").expect("wakeups");
        assert!(!wk.text.contains("Больше всех входящих пакетов"), "{}", wk.text);
        assert_eq!(wk.uid, None);
    }

    #[test]
    fn drain_and_background_app() {
        let mut pk = BTreeMap::new();
        pk.insert(10_123u32, vec!["com.example.chatty".to_string()]);
        let mut v = Vec::new();
        for i in 0..70 {
            // 6 мА·ч за минуту = 360 мА (7,2 %/ч при 5000 мА·ч); приложение — 20 мА CPU (1200 мА·с за минуту)
            let mut x = b(i * 60_000, false, 0.95, 6_000);
            x.procs.push(ProcRec { uid: 10_123, n: "com.example.chatty:sync".into(), t: 1000, e: 1200.0, ..Default::default() });
            v.push(x);
        }
        let now = 70 * 60_000;
        let mut w = Watch::scratch();
        let a = w.eval(&refs(&v), &cfg(), Some(5000.0), &pk, now);
        let drain = a.iter().find(|a| a.rule == "idle_drain").expect("idle_drain");
        assert_eq!(drain.level, "bad");
        let bg = a.iter().find(|a| a.rule == "app_bg").expect("app_bg");
        assert_eq!(bg.uid, Some(10_123));
        assert_eq!(bg.pkg.as_deref(), Some("com.example.chatty"));
        assert!(bg.text.contains("com.example.chatty:sync"), "{}", bg.text);
        // хорошо спит — о сне молчит
        assert!(a.iter().all(|a| a.rule != "no_sleep"));
    }

    #[test]
    fn hot_needs_sustained_heat() {
        let mut v = Vec::new();
        for i in 0..20 {
            let mut x = b(i * 60_000, false, 0.9, 50);
            x.temp = if i >= 8 { 43.0 } else { 33.0 };
            v.push(x);
        }
        let mut w = Watch::scratch();
        let a = w.eval(&refs(&v), &cfg(), Some(5000.0), &BTreeMap::new(), 20 * 60_000);
        assert!(a.iter().any(|a| a.rule == "hot"));
        v[17].temp = 38.0;
        let mut w2 = Watch::scratch();
        assert!(w2.eval(&refs(&v), &cfg(), Some(5000.0), &BTreeMap::new(), 20 * 60_000).iter().all(|a| a.rule != "hot"));
    }

    #[test]
    fn usb_flap_alert_once_and_mutable() {
        let s = 1_000_000u64;
        // мигание началось на 4-й секунде после загрузки, сейчас 100-я
        let f = crate::usbport::Flap { n: 252, span_us: 96 * s, first_seq: 48_123, first_us: 4 * s };
        let now = 1_795_722_976_000i64;
        let a = usb_flap_alert(&f, now, 100 * s, "1a2b3c4d");
        assert_eq!(a.id, "usb_flap@1a2b3c4d:48123");
        assert_eq!((a.rule.as_str(), a.level.as_str()), ("usb_flap", "bad"));
        assert_eq!(a.from, now - 96_000);
        assert!(a.text.contains("252 раз за 1 мин 36 с"), "{}", a.text);
        assert!(a.text.contains("сразу после загрузки"));
        let mut w = Watch::scratch();
        let c = cfg();
        assert!(w.admit(&c, a.clone()).is_some());
        // та же тревога (перезапуск демона в той же загрузке) — второй раз не показываем
        assert!(w.admit(&c, a.clone()).is_none());
        let mut w = Watch::scratch();
        let mut muted = cfg();
        muted.watch_mute = vec!["usb_flap".into()];
        assert!(w.admit(&muted, a.clone()).is_none());
        let mut off = cfg();
        off.watch = false;
        assert!(Watch::scratch().admit(&off, a).is_none());
    }
}
