//! Действия над приложением (ограничить работу в фоне, остановить) и сравнение «до/после».
//!
//! Сравнение идёт по покою — времени с выключенным экраном без зарядки: именно его меняют
//! ограничения фона, и его можно честно привести к «в час», даже если отрезки разной длины.

use crate::explain;
use crate::report::{self, label_for, Query};
use crate::sampler::Daemon;
use crate::store::Bucket;
use crate::util;
use crate::watch::OffStats;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Покоя в каждом отрезке должно быть хотя бы столько, чтобы сравнение что-то значило.
pub const MIN_OFF_MS: i64 = 90 * 60_000;
/// Период покоя для пресета «два последних»: не короче.
const REST_MIN_MS: i64 = 2 * 3_600_000;
/// Включения экрана короче этого внутри покоя склеиваются (глянуть время ночью).
const BLIP_MS: i64 = 5 * 60_000;
/// «До» действия — не дальше назад.
const BEFORE_MAX_MS: i64 = 48 * 3_600_000;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Action {
    pub t: i64,
    pub pkg: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// restrict | unrestrict | stop
    pub action: String,
    pub ok: bool,
    /// режим работы в фоне до действия (allow / ignore / …)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub out: String,
    /// (0.8) кто сделал: "" — вручную из карточки, mode — экономный режим, clean — авто-уборка
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
}

fn valid_pkg(p: &str) -> bool {
    p.len() < 256 && p.contains('.') && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

fn user_of(uid: Option<u32>) -> String {
    (uid.unwrap_or(0) / 100_000).to_string()
}

/// Вывод `cmd appops get … RUN_ANY_IN_BACKGROUND`: «RUN_ANY_IN_BACKGROUND: ignore; time=…»;
/// «No operations.» — режим не менялся (по умолчанию разрешено).
pub fn parse_appop(t: &str) -> Option<String> {
    for line in t.lines() {
        if let Some(rest) = line.trim().strip_prefix("RUN_ANY_IN_BACKGROUND:") {
            return Some(rest.split(';').next().unwrap_or("").trim().to_string());
        }
    }
    if t.contains("No operations") {
        return Some("allow".into());
    }
    None
}

pub fn bucket_name(b: i64) -> &'static str {
    match b {
        5 => "exempted",
        10 => "active",
        20 => "working_set",
        30 => "frequent",
        40 => "rare",
        45 => "restricted",
        50 => "never",
        _ => "unknown",
    }
}

fn bg_mode(pkg: &str, user: &str) -> Option<String> {
    util::run_cmd("cmd", &["appops", "get", "--user", user, pkg, "RUN_ANY_IN_BACKGROUND"], 10_000).and_then(|t| parse_appop(&t))
}

/// То же с повторами: под нагрузкой `cmd appops` иногда не отвечает — «не прочитался» не значит «не ограничено».
fn bg_mode_sure(pkg: &str, user: &str) -> Option<String> {
    for i in 0..3 {
        if let Some(m) = bg_mode(pkg, user) {
            return Some(m);
        }
        if i < 2 {
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
    }
    None
}

/// Можно ли работать в фоне (appop RUN_ANY_IN_BACKGROUND) и корзина ожидания приложения.
pub fn app_state(pkg: &str, uid: Option<u32>) -> Value {
    if !valid_pkg(pkg) {
        return json!({"error": "плохое имя пакета"});
    }
    let user = user_of(uid);
    let bg = bg_mode(pkg, &user);
    let bucket = util::run_cmd("cmd", &["activity", "get-standby-bucket", "--user", &user, pkg], 10_000).and_then(|t| t.trim().parse::<i64>().ok());
    json!({"pkg": pkg, "uid": uid, "bg": bg, "restricted": bg.as_deref() == Some("ignore"), "bucket": bucket, "bucket_name": bucket.map(bucket_name)})
}

/// Выполнить действие и записать его в actions.jsonl.
pub fn act(dir: &Path, pkg: &str, uid: Option<u32>, action: &str) -> Value {
    if !valid_pkg(pkg) {
        return json!({"error": "плохое имя пакета"});
    }
    if pkg == crate::watch::APP_PKG || pkg == "android" {
        return json!({"error": "это действие для этого пакета запрещено"});
    }
    if let Some(u) = uid {
        if explain::uid_kind(u) != "app" {
            return json!({"error": "действия — только для приложений, не для системы"});
        }
    }
    let user = user_of(uid);
    let prev = bg_mode(pkg, &user);
    let (ok, out) = match action {
        "restrict" | "unrestrict" => {
            let mode = if action == "restrict" { "ignore" } else { "allow" };
            let out = util::run_cmd("cmd", &["appops", "set", "--user", &user, pkg, "RUN_ANY_IN_BACKGROUND", mode], 10_000).unwrap_or_default();
            (bg_mode(pkg, &user).as_deref() == Some(mode), out)
        }
        "stop" => match util::run_cmd("cmd", &["activity", "force-stop", "--user", &user, pkg], 15_000) {
            Some(out) => (true, out),
            None => (false, "cmd activity force-stop не запустился".into()),
        },
        other => return json!({"error": format!("неизвестное действие {other}")}),
    };
    let a = Action { t: util::real_ms(), pkg: pkg.into(), uid, action: action.into(), ok, prev, out: out.trim().chars().take(300).collect(), by: String::new() };
    journal(dir, &a);
    json!({"ok": ok, "action": a, "state": app_state(pkg, uid)})
}

fn journal(dir: &Path, a: &Action) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("actions.jsonl")) {
        let _ = writeln!(f, "{}", serde_json::to_string(a).unwrap_or_default());
    }
}

/// (0.8) Запретить фон от имени режима или авто-уборки. None — фон и так запрещён (не наше, не пишем).
pub fn restrict_by(dir: &Path, pkg: &str, uid: Option<u32>, by: &str) -> Option<Action> {
    if !valid_pkg(pkg) || pkg == crate::watch::APP_PKG {
        return None;
    }
    let user = user_of(uid);
    let prev = bg_mode_sure(pkg, &user);
    if matches!(prev.as_deref(), Some("ignore") | Some("deny")) {
        return None;
    }
    let out = util::run_cmd("cmd", &["appops", "set", "--user", &user, pkg, "RUN_ANY_IN_BACKGROUND", "ignore"], 10_000).unwrap_or_default();
    let ok = bg_mode_sure(pkg, &user).as_deref() == Some("ignore");
    let a = Action { t: util::real_ms(), pkg: pkg.into(), uid, action: "restrict".into(), ok, prev, out: out.trim().chars().take(300).collect(), by: by.into() };
    journal(dir, &a);
    Some(a)
}

/// (0.8) Остановить приложение от имени режима или авто-уборки; `why` — в журнал.
pub fn stop_by(dir: &Path, pkg: &str, uid: Option<u32>, by: &str, why: &str) -> Action {
    let user = user_of(uid);
    let (ok, out) = match util::run_cmd("cmd", &["activity", "force-stop", "--user", &user, pkg], 15_000) {
        Some(o) => (true, format!("{why} {}", o.trim())),
        None => (false, format!("{why} — cmd activity force-stop не запустился")),
    };
    let a = Action { t: util::real_ms(), pkg: pkg.into(), uid, action: "stop".into(), ok, prev: None, out: out.trim().chars().take(300).collect(), by: by.into() };
    journal(dir, &a);
    a
}

/// Весь журнал действий по порядку (без отсечки по сроку — по нему возвращаются ограничения).
fn all_actions(dir: &Path) -> Vec<Action> {
    let mut v: Vec<Action> = std::fs::File::open(dir.join("actions.jsonl"))
        .map(|f| BufReader::new(f).lines().map_while(Result::ok).filter_map(|l| serde_json::from_str::<Action>(&l).ok()).collect())
        .unwrap_or_default();
    v.sort_by_key(|a| a.t);
    v
}

/// (0.8.1) Кого останавливало правило «только если много ест»: uid → время последней такой остановки.
/// Нужно ядру после перезапуска, чтобы не снять запрет фона раньше конца профиля.
pub fn watch_trips(dir: &Path) -> Vec<(u32, i64)> {
    let mut m: std::collections::BTreeMap<u32, i64> = std::collections::BTreeMap::new();
    for a in all_actions(dir).iter().filter(|a| a.ok && a.by == "watch" && a.action == "stop") {
        if let Some(u) = a.uid {
            m.insert(u, a.t);
        }
    }
    m.into_iter().collect()
}

/// Ограничение, которое поставил Жор и которое до сих пор действует.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Restr {
    pub pkg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// когда ограничил
    pub since: i64,
    /// режим работы в фоне до Жора — в него и вернём
    pub prev: String,
    /// (0.8) кто ограничил: "" — вручную, mode — профили
    #[serde(skip_serializing_if = "String::is_empty")]
    pub by: String,
    /// (0.8) что именно: bg — запрет фона, disable — пакет отключён, nonet — без сети
    #[serde(default)]
    pub kind: &'static str,
}

/// Из журнала: кого Жор ограничил и не вернул. Первое «ограничить» серии хранит режим до Жора; если и тогда
/// фон был запрещён (ignore/deny) — ограничение не наше. Текущий режим не проверяется (см. [restricted]).
fn claimed(all: &[Action]) -> Vec<Restr> {
    let mut state: std::collections::BTreeMap<(String, Option<u32>), Option<Restr>> = std::collections::BTreeMap::new();
    for a in all.iter().filter(|a| a.ok) {
        let e = state.entry((a.pkg.clone(), a.uid)).or_insert(None);
        match a.action.as_str() {
            "restrict" if e.is_none() => {
                let prev = a.prev.clone().unwrap_or_else(|| "allow".into());
                *e = Some(Restr { pkg: a.pkg.clone(), uid: a.uid, since: a.t, prev, by: a.by.clone(), kind: "bg" });
            }
            "unrestrict" => *e = None,
            _ => {}
        }
    }
    state.into_values().flatten().filter(|r| r.prev != "ignore" && r.prev != "deny").collect()
}

/// Кого Жор ограничил и кто до сих пор ограничен. Если режим с тех пор поменяли (сами, в настройках Android,
/// или приложение переустановили) — это уже не наше, не трогаем.
pub fn restricted(dir: &Path) -> Vec<Restr> {
    claimed(&all_actions(dir)).into_iter().filter(|r| bg_mode(&r.pkg, &user_of(r.uid)).as_deref() == Some("ignore")).collect()
}

/// (0.8) Все ограничения фона Жора по журналу (без проверки текущего режима).
pub fn claims(dir: &Path) -> Vec<Restr> {
    claimed(&all_actions(dir))
}

/// Пары действий «включил / вернул» из журнала: что Жор сделал и не вернул (по пакету и uid).
fn claimed_pair(all: &[Action], on: &str, off: &str, kind: &'static str) -> Vec<Restr> {
    let mut state: std::collections::BTreeMap<(String, Option<u32>), Option<Restr>> = std::collections::BTreeMap::new();
    for a in all.iter().filter(|a| a.ok) {
        let e = state.entry((a.pkg.clone(), a.uid)).or_insert(None);
        if a.action == on && e.is_none() {
            *e = Some(Restr { pkg: a.pkg.clone(), uid: a.uid, since: a.t, prev: a.prev.clone().unwrap_or_default(), by: a.by.clone(), kind });
        } else if a.action == off {
            *e = None;
        }
    }
    state.into_values().flatten().collect()
}

/// (0.8) Пакеты, которые Жор отключил (`pm disable-user`) и не включил обратно.
pub fn disable_claims(dir: &Path) -> Vec<Restr> {
    claimed_pair(&all_actions(dir), "disable", "enable", "disable")
}

/// (0.8) Компоненты, которые Жор отключил (`pm disable pkg/класс`) и не включил обратно.
pub fn comp_claims(dir: &Path) -> Vec<Restr> {
    claimed_pair(&all_actions(dir), "cdisable", "cenable", "component")
}

/// (0.8) Приложения, которым Жор закрыл сеть и не открыл.
pub fn nonet_claims(dir: &Path) -> Vec<Restr> {
    claimed_pair(&all_actions(dir), "nonet", "net", "nonet")
}

// ---- (0.8) отключить пакет ----

/// Отключён ли пакет у пользователя (`pm list packages -d`). None — не прочиталось.
fn pkg_disabled(pkg: &str, user: &str) -> Option<bool> {
    let t = util::run_cmd("pm", &["list", "packages", "-d", "--user", user], 20_000)?;
    Some(t.lines().any(|l| l.trim().strip_prefix("package:") == Some(pkg)))
}

/// Отключить пакет у пользователя копии (uid). None — и так отключён (не наше, не пишем).
pub fn disable_by(dir: &Path, pkg: &str, uid: Option<u32>, by: &str, why: &str) -> Option<Action> {
    if !valid_pkg(pkg) || pkg == crate::watch::APP_PKG || pkg == "android" {
        return None;
    }
    let user = user_of(uid);
    if pkg_disabled(pkg, &user) == Some(true) {
        return None;
    }
    let out = util::run_cmd("pm", &["disable-user", "--user", &user, pkg], 30_000).unwrap_or_default();
    let ok = pkg_disabled(pkg, &user) == Some(true);
    let a = Action { t: util::real_ms(), pkg: pkg.into(), uid, action: "disable".into(), ok, prev: Some("enabled".into()), out: format!("{why} {}", out.trim()).trim().chars().take(300).collect(), by: by.into() };
    journal(dir, &a);
    Some(a)
}

/// Включить обратно пакет, который отключил Жор.
pub fn enable_one(dir: &Path, r: &Restr, why: &str, by: &str) -> Unrestricted {
    let user = user_of(r.uid);
    let mark = |ok: bool, out: String| {
        let a = Action { t: util::real_ms(), pkg: r.pkg.clone(), uid: r.uid, action: "enable".into(), ok, prev: Some("disabled".into()), out, by: by.into() };
        journal(dir, &a);
    };
    if pkg_disabled(&r.pkg, &user) == Some(false) {
        // включили без Жора (или пакет удалили) — отпускаем
        mark(true, format!("{why}: уже включён — не трогаю"));
        return Unrestricted::Skipped(Some("enabled".into()));
    }
    let out = util::run_cmd("pm", &["enable", "--user", &user, &r.pkg], 30_000).unwrap_or_default();
    let ok = pkg_disabled(&r.pkg, &user) == Some(false);
    mark(ok, format!("{why}: {}", out.trim()).chars().take(300).collect());
    if ok { Unrestricted::Done } else { Unrestricted::Failed }
}

// ---- (0.8) отключить компонент (службу, провайдер и т. п.) внутри пакета ----

/// «pkg/полный.класс» или «pkg/.класс».
pub fn valid_comp(c: &str) -> bool {
    match c.split_once('/') {
        Some((p, cls)) => valid_pkg(p) && !cls.is_empty() && cls.len() < 256 && cls.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '$'),
        None => false,
    }
}

fn comp_class(c: &str) -> Option<(&str, String)> {
    let (p, cls) = c.split_once('/')?;
    Some((p, if cls.starts_with('.') { format!("{p}{cls}") } else { cls.to_string() }))
}

/// Отключённые компоненты пользователя из `dumpsys package <pkg>`: после строки «    User N: …» —
/// блок «      disabledComponents:» с классами.
fn parse_disabled_components(t: &str, user: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut cur: Option<String> = None;
    let mut inside = false;
    for line in t.lines() {
        let indent = line.len() - line.trim_start().len();
        let l = line.trim();
        if indent == 4 && l.starts_with("User ") {
            cur = l[5..].split(':').next().map(str::to_string);
            inside = false;
            continue;
        }
        if l == "disabledComponents:" {
            inside = true;
            continue;
        }
        if inside {
            if indent >= 8 && !l.is_empty() && !l.ends_with(':') {
                if cur.as_deref() == Some(user) {
                    out.insert(l.to_string());
                }
            } else {
                inside = false;
            }
        }
    }
    out
}

fn comp_disabled(c: &str, user: &str) -> Option<bool> {
    let (p, cls) = comp_class(c)?;
    let t = util::run_cmd("dumpsys", &["package", p], 30_000)?;
    Some(parse_disabled_components(&t, user).contains(&cls))
}

/// Отключить компонент у пользователя копии (uid). None — и так отключён (не наше).
pub fn disable_comp_by(dir: &Path, comp: &str, uid: Option<u32>, by: &str, why: &str) -> Option<Action> {
    if !valid_comp(comp) || comp.starts_with(crate::watch::APP_PKG) || comp.starts_with("android/") {
        return None;
    }
    let user = user_of(uid);
    if comp_disabled(comp, &user) == Some(true) {
        return None;
    }
    let out = util::run_cmd("pm", &["disable", "--user", &user, comp], 30_000).unwrap_or_default();
    let ok = comp_disabled(comp, &user) == Some(true);
    let a = Action { t: util::real_ms(), pkg: comp.into(), uid, action: "cdisable".into(), ok, prev: Some("enabled".into()), out: format!("{why} {}", out.trim()).trim().chars().take(300).collect(), by: by.into() };
    journal(dir, &a);
    Some(a)
}

/// Включить обратно компонент, который отключил Жор.
pub fn enable_comp_one(dir: &Path, r: &Restr, why: &str, by: &str) -> Unrestricted {
    let user = user_of(r.uid);
    let mark = |ok: bool, out: String| {
        let a = Action { t: util::real_ms(), pkg: r.pkg.clone(), uid: r.uid, action: "cenable".into(), ok, prev: Some("disabled".into()), out, by: by.into() };
        journal(dir, &a);
    };
    if comp_disabled(&r.pkg, &user) == Some(false) {
        mark(true, format!("{why}: уже включён — не трогаю"));
        return Unrestricted::Skipped(Some("enabled".into()));
    }
    let out = util::run_cmd("pm", &["enable", "--user", &user, &r.pkg], 30_000).unwrap_or_default();
    let ok = comp_disabled(&r.pkg, &user) == Some(false);
    mark(ok, format!("{why}: {}", out.trim()).chars().take(300).collect());
    if ok { Unrestricted::Done } else { Unrestricted::Failed }
}

// ---- (0.8) без сети: своя цепочка iptables с правилами по uid ----
//
// `cmd connectivity set-package-networking-enabled` (FIREWALL_CHAIN_OEM_DENY_3) берёт uid пакета только основного
// пользователя, а копиям во втором пространстве и двойных приложениях нужен свой uid. Поэтому — iptables/ip6tables:
// цепочка zhor_out, переход в неё первым в OUTPUT, правило «uid → REJECT» на приложение. Исходящие пакеты приложения
// (и ответы-подтверждения его соединений) не уходят — соединения рвутся, сервер перестаёт слать. После перезагрузки
// правил нет — ядро ставит их заново, пока правило профиля действует.

const NET_CHAIN: &str = "zhor_out";

fn ipt(v6: bool, args: &[&str]) -> bool {
    let mut a = vec!["-w", "5"];
    a.extend_from_slice(args);
    util::run_ok(if v6 { "ip6tables" } else { "iptables" }, &a, 10_000)
}

fn net_chain_ready() -> bool {
    let mut ok = true;
    for v6 in [false, true] {
        if !ipt(v6, &["-L", NET_CHAIN, "-n"]) {
            ipt(v6, &["-N", NET_CHAIN]);
        }
        if !ipt(v6, &["-C", "OUTPUT", "-j", NET_CHAIN]) {
            ok &= ipt(v6, &["-I", "OUTPUT", "1", "-j", NET_CHAIN]);
        }
    }
    ok
}

fn net_rule(uid: u32, add: bool) -> bool {
    let u = uid.to_string();
    let rule = ["-m", "owner", "--uid-owner", u.as_str(), "-j", "REJECT"];
    let mut ok = true;
    for v6 in [false, true] {
        let with = |cmd: &'static str| {
            let mut a: Vec<&str> = vec![cmd, NET_CHAIN];
            a.extend_from_slice(&rule);
            a
        };
        if add {
            if !ipt(v6, &with("-C")) {
                ok &= ipt(v6, &with("-A"));
            }
        } else {
            for _ in 0..5 {
                if !ipt(v6, &with("-D")) {
                    break;
                }
            }
            ok &= !ipt(v6, &with("-C"));
        }
    }
    ok
}

/// Закрыть сеть приложению; `journal` — записать (первый раз; после перезагрузки правило ставится заново молча).
pub fn nonet_by(dir: &Path, pkg: &str, uid: u32, by: &str, journal_it: bool) -> bool {
    if crate::explain::uid_kind(uid) != "app" || pkg == crate::watch::APP_PKG {
        return false;
    }
    let ok = net_chain_ready() && net_rule(uid, true);
    if journal_it {
        let a = Action { t: util::real_ms(), pkg: pkg.into(), uid: Some(uid), action: "nonet".into(), ok, prev: None, out: String::new(), by: by.into() };
        journal(dir, &a);
    }
    ok
}

/// Открыть сеть обратно.
pub fn net_one(dir: &Path, r: &Restr, why: &str, by: &str) -> Unrestricted {
    let ok = r.uid.map(|u| net_rule(u, false)).unwrap_or(true);
    let a = Action { t: util::real_ms(), pkg: r.pkg.clone(), uid: r.uid, action: "net".into(), ok, prev: None, out: why.chars().take(300).collect(), by: by.into() };
    journal(dir, &a);
    net_cleanup();
    if ok { Unrestricted::Done } else { Unrestricted::Failed }
}

/// Пустую цепочку убрать совсем, чтобы от Жора в iptables ничего не оставалось.
pub fn net_cleanup() {
    for v6 in [false, true] {
        let rules = util::run_cmd(if v6 { "ip6tables" } else { "iptables" }, &["-w", "5", "-S", NET_CHAIN], 10_000).unwrap_or_default();
        if rules.lines().filter(|l| l.starts_with("-A")).count() == 0 {
            while ipt(v6, &["-D", "OUTPUT", "-j", NET_CHAIN]) {}
            ipt(v6, &["-X", NET_CHAIN]);
        }
    }
}

/// Итог возврата одного ограничения.
pub enum Unrestricted {
    Done,
    Failed,
    /// режим с тех пор поменяли (сами, в настройках, переустановкой) — уже не наше
    Skipped(Option<String>),
}

/// Вернуть одно ограничение Жора в режим до него; пишется в журнал как «unrestrict».
pub fn unrestrict_one(dir: &Path, r: &Restr, why: &str, by: &str) -> Unrestricted {
    let user = user_of(r.uid);
    // режим не прочитался — по журналу ограничение наше: снимаем
    let now = bg_mode_sure(&r.pkg, &user);
    if now.is_some() && now.as_deref() != Some("ignore") {
        // режим поменяли без Жора: для режима отмечаем в журнале, что отпустили, — иначе он будет
        // пытаться вернуть это снова и снова
        if !by.is_empty() {
            let a = Action {
                t: util::real_ms(),
                pkg: r.pkg.clone(),
                uid: r.uid,
                action: "unrestrict".into(),
                ok: true,
                prev: now.clone(),
                out: format!("{why}: режим уже {} — не трогаю", now.as_deref().unwrap_or("?")),
                by: by.into(),
            };
            journal(dir, &a);
        }
        return Unrestricted::Skipped(now);
    }
    let mode = match r.prev.as_str() {
        m @ ("allow" | "foreground" | "default") => m,
        _ => "allow",
    };
    let out = util::run_cmd("cmd", &["appops", "set", "--user", &user, &r.pkg, "RUN_ANY_IN_BACKGROUND", mode], 10_000).unwrap_or_default();
    let after = bg_mode_sure(&r.pkg, &user);
    let ok = after.as_deref() == Some(mode) || (mode == "allow" && after.is_some() && after.as_deref() != Some("ignore"));
    let a = Action {
        t: util::real_ms(),
        pkg: r.pkg.clone(),
        uid: r.uid,
        action: "unrestrict".into(),
        ok,
        prev: Some("ignore".into()),
        out: format!("{why}: {}", out.trim()).chars().take(300).collect(),
        by: by.into(),
    };
    journal(dir, &a);
    if ok { Unrestricted::Done } else { Unrestricted::Failed }
}

/// Вернуть всё, что ограничил Жор (вручную и режимами), в режим до него.
pub fn revert_all(dir: &Path) -> Value {
    let mut done = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = Vec::new();
    let all = all_actions(dir);
    for r in claimed(&all) {
        match unrestrict_one(dir, &r, "вернуть всё", "") {
            Unrestricted::Done => done.push(r),
            Unrestricted::Failed => failed.push(r),
            Unrestricted::Skipped(mode) => skipped.push(json!({"pkg": r.pkg, "uid": r.uid, "mode": mode})),
        }
    }
    // (0.8) отключённые пакеты и закрытая сеть
    for r in claimed_pair(&all, "disable", "enable", "disable") {
        match enable_one(dir, &r, "вернуть всё", "revert") {
            Unrestricted::Done => done.push(r),
            Unrestricted::Failed => failed.push(r),
            Unrestricted::Skipped(_) => skipped.push(json!({"pkg": r.pkg, "uid": r.uid, "mode": "enabled"})),
        }
    }
    for r in claimed_pair(&all, "cdisable", "cenable", "component") {
        match enable_comp_one(dir, &r, "вернуть всё", "revert") {
            Unrestricted::Done => done.push(r),
            Unrestricted::Failed => failed.push(r),
            Unrestricted::Skipped(_) => skipped.push(json!({"pkg": r.pkg, "uid": r.uid, "mode": "enabled"})),
        }
    }
    for r in claimed_pair(&all, "nonet", "net", "nonet") {
        match net_one(dir, &r, "вернуть всё", "") {
            Unrestricted::Done => done.push(r),
            _ => failed.push(r),
        }
    }
    net_cleanup();
    json!({"ok": failed.is_empty(), "reverted": done, "failed": failed, "skipped": skipped})
}

/// Журнал действий, новые первыми; `uid` — только этого приложения.
pub fn actions(dir: &Path, uid: Option<u32>, keep_days: i64) -> Vec<Action> {
    let cut = util::real_ms() - keep_days.max(1) * 86_400_000;
    let mut v: Vec<Action> = std::fs::File::open(dir.join("actions.jsonl"))
        .map(|f| BufReader::new(f).lines().map_while(Result::ok).filter_map(|l| serde_json::from_str::<Action>(&l).ok()).collect())
        .unwrap_or_default();
    v.retain(|a| a.t >= cut && (uid.is_none() || a.uid == uid));
    v.reverse();
    v.truncate(100);
    v
}

/// Долгие периоды покоя по корзинам в памяти (≈3 дня): экран выключен, без зарядки; короткие
/// включения экрана склеиваются, зарядка и разрывы записи обрывают период.
pub fn rest_periods(d: &Daemon, now: i64) -> Vec<(i64, i64)> {
    let open = d.open_snapshot();
    let all: Vec<&Bucket> = d.store.mem.iter().chain(open.iter()).collect();
    rest_periods_in(&all, now)
}

fn rest_periods_in(all: &[&Bucket], now: i64) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    let mut cur: Option<(i64, i64)> = None;
    for b in all {
        if b.plug {
            if let Some(c) = cur.take() {
                out.push(c);
            }
        } else if !b.scr {
            cur = match cur {
                Some((s, e)) if b.t0 - e <= BLIP_MS => Some((s, b.t1)),
                Some(c) => {
                    out.push(c);
                    Some((b.t0, b.t1))
                }
                None => Some((b.t0, b.t1)),
            };
        }
    }
    if let Some(c) = cur {
        out.push((c.0, c.1.min(now)));
    }
    out.retain(|(s, e)| e - s >= REST_MIN_MS);
    out
}

fn win_json(d: &Daemon, s: &OffStats, from: i64, to: i64, uid: Option<u32>) -> Value {
    let h = s.hours().max(1e-6);
    let cap = d.full_mah.or(d.design_mah);
    let mut w = json!({
        "from": from, "to": to,
        "off_ms": s.off_ms as u64,
        "deep_sleep_pct": s.deep_pct(),
        "pct_h": s.pct_h(cap),
        "ma": s.avg_ma(),
        "mah": s.mah(),
        "wakeups_h": s.wakeups() / h,
        "suspends_h": s.suspends / h,
        "fails_h": s.fails / h,
        "cpu_ma": s.cpu_mas / (s.off_ms / 1000.0).max(1e-3),
        "temp_max": s.temp_max,
    });
    if let Some(u) = uid {
        let a = s.apps.get(&u);
        let mut app = json!({
            "uid": u,
            "cpu_ma": s.app_ma(u),
            "cpu_mah": a.map(|a| a.mas / 3600.0).unwrap_or(0.0),
            "starts_h": a.map(|a| a.starts / h).unwrap_or(0.0),
        });
        // будильники, wakelock и задания — из статистики Android за весь отрезок (не только покой)
        let rep = report::build(d, &Query { from: Some(from), to: Some(to), uid: Some(u), ..Default::default() }, None);
        if let Some(r) = rep.apps.first() {
            let span_h = ((to - from) as f64 / 3_600_000.0).max(1e-6);
            app["alarms_h"] = json!(r.alarms.iter().map(|i| i.count).sum::<u64>() as f64 / span_h);
            app["wakelock_min_h"] = json!(r.wakelocks.iter().map(|i| i.ms).sum::<u64>() as f64 / 60_000.0 / span_h);
            app["jobs_h"] = json!(r.jobs.iter().map(|i| i.count).sum::<u64>() as f64 / span_h);
            app["mah_h"] = json!(r.mah / span_h);
        }
        w["app"] = app;
    }
    w
}

fn stats(d: &Daemon, from: i64, to: i64) -> OffStats {
    let mut v = d.store.range(from, to);
    if let Some(ob) = d.open_snapshot() {
        if ob.t1 > from && ob.t0 < to {
            v.push(ob);
        }
    }
    OffStats::over(&v, from, to)
}

/// Сравнение двух отрезков по покою.
/// `mode`: `rest` — два последних долгих периода покоя; `action` + `t` — до и после момента `t`
/// (48 ч до против «с тех пор»); иначе явные `a:{from,to}` и `b:{from,to}`. `uid` — ещё и это приложение.
pub fn compare(d: &Daemon, req: &Value) -> Value {
    let now = util::real_ms();
    let uid = req["uid"].as_u64().map(|u| u as u32);
    let mode = req["mode"].as_str().unwrap_or("custom");
    let periods = rest_periods(d, now);
    let oldest = d.store.oldest().unwrap_or(d.started);
    let (a, b) = match mode {
        "rest" => {
            if periods.len() < 2 {
                return json!({
                    "ok": false, "mode": mode,
                    "reason": format!("Нужно два периода покоя (экран выключен, без зарядки) не короче {} за последние 3 дня — пока {}.", util::fmt_dur(REST_MIN_MS as u64), periods.len()),
                    "periods": periods,
                });
            }
            let n = periods.len();
            (periods[n - 2], periods[n - 1])
        }
        "action" => {
            let Some(t) = req["t"].as_i64() else { return json!({"error": "нужен t"}) };
            if t <= oldest || t >= now {
                return json!({"ok": false, "mode": mode, "reason": format!("Момент {} вне записей ядра (они есть с {}).", report::fmt_time(t), report::fmt_time(oldest)), "periods": periods});
            }
            (((t - BEFORE_MAX_MS).max(oldest), t), (t, now))
        }
        _ => {
            let get = |k: &str| -> Option<(i64, i64)> { Some((req[k]["from"].as_i64()?, req[k]["to"].as_i64()?)) };
            match (get("a"), get("b")) {
                (Some(a), Some(b)) if a.0 < a.1 && b.0 < b.1 => (a, b),
                _ => return json!({"error": "нужны a:{from,to} и b:{from,to}, либо mode rest / action"}),
            }
        }
    };
    let sa = stats(d, a.0, a.1);
    let sb = stats(d, b.0, b.1);
    // приложения: объединение топов обоих отрезков по CPU в покое
    let mut uids: Vec<u32> = sa.top_any(8).into_iter().chain(sb.top_any(8)).map(|(u, _)| u).collect();
    if let Some(u) = uid {
        uids.push(u);
    }
    uids.sort();
    uids.dedup();
    let mut apps: Vec<Value> = uids
        .iter()
        .map(|&u| json!({"uid": u, "label": label_for(u, &d.pkgs).0, "kind": explain::uid_kind(u), "a_ma": sa.app_ma(u), "b_ma": sb.app_ma(u)}))
        .collect();
    apps.sort_by(|x, y| {
        let m = |v: &Value| v["a_ma"].as_f64().unwrap_or(0.0).max(v["b_ma"].as_f64().unwrap_or(0.0));
        m(y).partial_cmp(&m(x)).unwrap_or(std::cmp::Ordering::Equal)
    });
    apps.truncate(12);
    let mut notes = Vec::new();
    if a.0 < oldest {
        notes.push(format!("Записи ядра есть с {} — раньше сравнивать не с чем.", report::fmt_time(oldest)));
    }
    let enough = sa.off_ms as i64 >= MIN_OFF_MS && sb.off_ms as i64 >= MIN_OFF_MS;
    if !enough {
        notes.push(format!(
            "Для честного сравнения нужно хотя бы {} покоя (экран выключен, без зарядки) в каждом отрезке: сейчас до — {}, после — {}.",
            util::fmt_dur(MIN_OFF_MS as u64),
            util::fmt_dur(sa.off_ms as u64),
            util::fmt_dur(sb.off_ms as u64)
        ));
    }
    json!({
        "ok": true, "mode": mode, "uid": uid, "enough": enough, "min_off_ms": MIN_OFF_MS,
        "a": win_json(d, &sa, a.0, a.1, uid),
        "b": win_json(d, &sb, b.0, b.1, uid),
        "apps": apps,
        "periods": periods,
        "notes": notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(t0: i64, scr: bool, plug: bool) -> Bucket {
        Bucket { t0, t1: t0 + 60_000, span: 60_000, scr, plug, ..Default::default() }
    }

    #[test]
    fn appop_output() {
        assert_eq!(parse_appop("RUN_ANY_IN_BACKGROUND: ignore; time=+3m ago\n").as_deref(), Some("ignore"));
        assert_eq!(parse_appop("RUN_ANY_IN_BACKGROUND: allow; time=+46d20h50m27s916ms ago").as_deref(), Some("allow"));
        assert_eq!(parse_appop("No operations.").as_deref(), Some("allow"));
        assert_eq!(parse_appop(""), None);
        assert!(!valid_pkg("com.x; rm"));
        assert!(valid_pkg("com.example.app_2"));
    }

    #[test]
    fn disabled_components_per_user() {
        let t = "Packages:\n  Package [com.example.sec] (abc):\n    User 0: ceDataInode=1 installed=true enabled=0\n      lastDisabledCaller: x\n      disabledComponents:\n        io.appmetrica.analytics.internal.AppMetricaService\n        com.example.sec.Other\n    User 10: ceDataInode=2 installed=true enabled=0\n      gids=[3003]\n";
        let d0 = parse_disabled_components(t, "0");
        assert!(d0.contains("io.appmetrica.analytics.internal.AppMetricaService") && d0.len() == 2);
        assert!(parse_disabled_components(t, "10").is_empty());
        assert!(valid_comp("com.miui.securitycenter/io.appmetrica.analytics.internal.AppMetricaService"));
        assert!(valid_comp("com.example/.Inner$Svc"));
        assert!(!valid_comp("com.example"));
        assert!(!valid_comp("com.example/a b"));
        assert_eq!(comp_class("com.example/.Svc").map(|x| x.1), Some("com.example.Svc".to_string()));
    }

    fn act(t: i64, pkg: &str, action: &str, prev: Option<&str>, ok: bool) -> Action {
        Action { t, pkg: pkg.into(), uid: Some(10_100), action: action.into(), ok, prev: prev.map(String::from), out: String::new(), by: String::new() }
    }

    #[test]
    fn claimed_follows_journal() {
        let all = vec![
            // ограничили, разрешили, снова ограничили: вернуть в режим до первого ограничения серии
            act(1, "com.a", "restrict", Some("allow"), true),
            act(2, "com.a", "unrestrict", Some("ignore"), true),
            act(3, "com.a", "restrict", Some("allow"), true),
            act(4, "com.a", "restrict", Some("ignore"), true),
            // было запрещено ещё до Жора — не наше
            act(5, "com.b", "restrict", Some("ignore"), true),
            // не вышло — не считается
            act(6, "com.c", "restrict", Some("allow"), false),
            // остановка — не ограничение
            act(7, "com.d", "stop", Some("allow"), true),
            // ограничили и вернули
            act(8, "com.e", "restrict", Some("allow"), true),
            act(9, "com.e", "unrestrict", Some("ignore"), true),
        ];
        let c = claimed(&all);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!((c[0].pkg.as_str(), c[0].since, c[0].prev.as_str()), ("com.a", 3, "allow"));
    }

    #[test]
    fn rest_periods_merge_blips_and_break_on_charge() {
        let m = 60_000;
        let mut v = Vec::new();
        // 3 ч покоя с включением экрана на 2 мин посередине
        for i in 0..90 {
            v.push(b(i * m, false, false));
        }
        v.push(b(90 * m, true, false));
        v.push(b(91 * m, true, false));
        for i in 92..182 {
            v.push(b(i * m, false, false));
        }
        // зарядка, затем 1 ч покоя (короче порога) и 2,5 ч после долгого включения экрана
        v.push(b(182 * m, false, true));
        for i in 183..243 {
            v.push(b(i * m, false, false));
        }
        for i in 243..263 {
            v.push(b(i * m, true, false));
        }
        for i in 263..413 {
            v.push(b(i * m, false, false));
        }
        let r: Vec<&Bucket> = v.iter().collect();
        let p = rest_periods_in(&r, 413 * m);
        assert_eq!(p, vec![(0, 182 * m), (263 * m, 413 * m)]);
    }
}
