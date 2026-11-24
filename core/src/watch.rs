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
use crate::util::{self, fmt_dur};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// Как часто проверять правила (по настенным часам, только на шаге сбора).
pub const EVAL_EVERY_MS: i64 = 5 * 60_000;
/// Корзины дальше друг от друга — разрыв в записи (ядро не работало): период обрывается.
const GAP_MS: i64 = 5 * 60_000;
/// Тревоги в памяти и на диске.
const KEEP_MS: i64 = 14 * 86_400_000;
const KEEP_N: usize = 300;

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
    /// no_sleep | idle_drain | wakeups | app_bg | hot | test
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
}

fn ma_text(v: f64) -> String {
    if v >= 10.0 { format!("{v:.0}") } else { format!("{v:.1}") }
}

impl Watch {
    pub fn open(dir: PathBuf) -> Self {
        let mut w = Watch { dir, alerts: VecDeque::new(), fired: HashSet::new(), last_eval: 0, last_cost_us: 0, min_scale: 1.0 };
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
        Watch { dir: PathBuf::from("/nonexistent"), alerts: VecDeque::new(), fired: HashSet::new(), last_eval: 0, last_cost_us: 0, min_scale: 1.0 }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("alerts.jsonl")
    }

    pub fn due(&self, now: i64) -> bool {
        now - self.last_eval >= EVAL_EVERY_MS
    }

    /// Проверить правила по хвосту корзин. Возвращает новые тревоги (ещё не записанные).
    pub fn eval(&mut self, buckets: &[&Bucket], cfg: &Config, full_mah: Option<f64>, pkgs: &BTreeMap<u32, Vec<String>>, now: i64) -> Vec<Alert> {
        self.last_eval = now;
        if !cfg.watch {
            return Vec::new();
        }
        let t0 = std::time::Instant::now();
        let out = match current_period(buckets, now) {
            Some((start, i)) => rules(&buckets[i..], start, cfg, full_mah, pkgs, now, self.min_scale),
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

/// Правила по текущему периоду покоя.
fn rules(period: &[&Bucket], start: i64, cfg: &Config, full_mah: Option<f64>, pkgs: &BTreeMap<u32, Vec<String>>, now: i64, scale: f64) -> Vec<Alert> {
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
        if let Some((n, ms)) = st.top_holder() {
            text += &format!(" Дольше всех не даёт спать «{n}» ({}).", fmt_dur(ms as u64));
        }
        let top = top_cpu();
        if let Some((_, t)) = &top {
            text += t;
        }
        let lvl = if deep < cfg.watch_sleep_pct / 2.0 { "bad" } else { "warn" };
        out.push(mk("no_sleep", None, lvl, "Телефон не спит".into(), text, top.map(|(u, _)| u).filter(|u| explain::uid_kind(*u) == "app")));
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
                let text = format!("«{n}» будит телефон ~{per_h:.0} раз в час ({:.0} за {}).{why}", c, fmt_dur(off));
                let lvl = if per_h > 4.0 * cfg.watch_wakeups_h { "bad" } else { "warn" };
                // «[timerfd] #6» → ключ «[timerfd]»: номер экземпляра меняется после перезагрузки
                out.push(mk("wakeups", Some(base_name(&n)), lvl, format!("Частые пробуждения: {n}"), text, None));
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
        let mut w = Watch { dir: std::env::temp_dir(), alerts: VecDeque::new(), fired: HashSet::new(), last_eval: 0, last_cost_us: 0, min_scale: 1.0 };
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
}
