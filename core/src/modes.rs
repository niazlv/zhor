//! (0.8) Группы приложений и профили.
//!
//! **Группа** — свой список копий приложений (uid: в основном профиле и во втором пространстве — свои), например
//! «Банки», «Мессенджеры РФ», «Акции и скидки». Приложение может быть в нескольких группах. Встроенная группа
//! `keep` «Важное» — Жор её никогда не трогает, ни в одном профиле.
//!
//! **Профиль** — когда включаться (вручную, всегда, с экономией энергии Android, при заряде ниже порога, когда сеть
//! другой страны, ночью, когда рядом нет Bluetooth-устройства (часов) — достаточно любого условия) и что делать с каждой группой и с приложениями без группы:
//! * `none` — не трогать;
//! * `watch` — только если много ест: при выключенном экране тратит в фоне больше порога (процессор или входящие
//!   пакеты) и давно не открывалось — остановить и запретить фон до конца профиля, прислать уведомление;
//! * `bg` — запрет работы в фоне (то же, что «Ограничено» в настройках батареи);
//! * `hard` — запрет фона и остановка при каждом выключении экрана;
//! * `nonet` — как `hard`, и ещё без сети (свои правила iptables по uid);
//! * `disable` — отключить пакет (`pm disable-user`): не запускается вовсе, пока правило действует.
//!
//! В группу можно класть и пакеты по имени (`packages`) — во всех пространствах сразу, в том числе системные с общим
//! uid 1000; им доступно только «отключить». И отдельные компоненты (`components`, «пакет/класс») — службы и
//! провайдеры внутри пакета, который целиком трогать нельзя: им тоже только «отключить» (`pm disable pkg/класс`).
//! «Все остальные» нельзя «отключить» и лишить сети.
//!
//! Правило приложения в профиле: своё (если задано поверх групп) → самое мягкое из правил его групп, у которых в
//! этом профиле правило есть (белый список сильнее чёрного) → правило «все остальные». Активных профилей может быть
//! несколько — из них берётся самое строгое. Всё, что ограничили профили, пишется в общий журнал действий (actions.jsonl, by = mode) и
//! при выходе возвращается в то, что было до Жора; «Вернуть всё» и уборка при удалении Жора снимают и это.
//! Остановка (force-stop) возврата не требует: приложение запустится, когда его откроют.
//!
//! Рычаги — только обратимые и стандартные: appop RUN_ANY_IN_BACKGROUND и `am force-stop`. Системные и нужные
//! для работы телефона пакеты защищены (см. [Protect]).

use crate::compare::{self, Restr};
use crate::store::Bucket;
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

pub const LEVELS: [&str; 6] = ["none", "watch", "bg", "hard", "nonet", "disable"];
const NONET: u8 = 4;
const DISABLE: u8 = 5;
/// Встроенная группа «Важное».
pub const KEEP: &str = "keep";
/// Ключ правила для приложений без группы.
pub const REST: &str = "rest";

/// Как часто пересматривать (и на каждой смене экрана).
const EVAL_MS: i64 = 60_000;
/// Повторная остановка, если приложение снова поднялось при выключенном экране.
const STOP_AGAIN_MS: i64 = 30 * 60_000;
/// «Много ест»: окно, по которому судим, и как часто смотрим.
const WATCH_WINDOW_MS: i64 = 30 * 60_000;
const WATCH_EVERY_MS: i64 = 5 * 60_000;
/// Возврат ограничения, которое не вышло вернуть, — не чаще.
const RETRY_MS: i64 = 10 * 60_000;

fn lvl(s: &str) -> u8 {
    LEVELS.iter().position(|x| *x == s).unwrap_or(0) as u8
}

pub fn level_name(l: u8) -> &'static str {
    LEVELS.get(l as usize).copied().unwrap_or("none")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub members: Vec<u32>,
    /// пакеты по имени — во всех пространствах (и системные с общим uid)
    pub packages: Vec<String>,
    /// компоненты «пакет/класс» — во всех пространствах, где есть пакет
    pub components: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct When {
    pub manual: bool,
    pub always: bool,
    /// с экономией энергии Android (не на зарядке)
    pub saver: bool,
    /// при заряде не выше, % (0 — нет; не на зарядке)
    pub below_pct: u8,
    /// сеть другой страны, чем домашняя
    pub abroad: bool,
    /// ночью: минуты местного времени, окно может переходить через полночь
    pub night: bool,
    pub night_from: u16,
    pub night_to: u16,
    /// когда ни одно из этих Bluetooth-устройств (по имени) не подключено дольше BT_GRACE_MS
    pub bt_away: Vec<String>,
}

/// Устройство должно пропасть хотя бы на столько, чтобы профиль «нет устройства» включился: короткий обрыв
/// связи с часами не должен гасить их приложение.
const BT_GRACE_MS: i64 = 10 * 60_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub on: bool,
    pub when: When,
    /// id группы или `rest` → none | watch | bg | hard
    pub rules: BTreeMap<String, String>,
    /// uid (строкой) → уровень поверх группы
    pub apps: BTreeMap<String, String>,
    /// «много ест»: CPU в фоне, мА (в среднем за 30 мин покоя) или входящих пакетов в час
    pub heavy_ma: f64,
    pub heavy_rx_h: u32,
    /// … и не открывали столько часов
    pub idle_h: u32,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            id: String::new(),
            name: String::new(),
            on: true,
            when: When::default(),
            rules: BTreeMap::new(),
            apps: BTreeMap::new(),
            heavy_ma: 5.0,
            heavy_rx_h: 2000,
            idle_h: 3,
        }
    }
}

fn compare_valid_pkg(p: &str) -> bool {
    p.len() < 256 && p.contains('.') && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

impl Default for When {
    fn default() -> Self {
        When { manual: false, always: false, saver: false, below_pct: 0, abroad: false, night: false, night_from: 23 * 60, night_to: 7 * 60, bt_away: Vec::new() }
    }
}

impl Profile {
    /// Правило для приложения: своё → самое мягкое из правил его групп (только тех, у которых в этом профиле
    /// правило есть) → «все остальные».
    fn level(&self, uid: u32, groups: &[&str]) -> u8 {
        if let Some(l) = self.apps.get(&uid.to_string()) {
            return lvl(l);
        }
        groups
            .iter()
            .filter_map(|g| self.rules.get(*g))
            .map(|l| lvl(l))
            .min()
            .or_else(|| self.rules.get(REST).map(|l| lvl(l)))
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Modes {
    pub groups: Vec<Group>,
    pub profiles: Vec<Profile>,
    /// домашняя страна (ISO, строчными); пусто — страна первой SIM
    pub home_country: String,
    /// какие заготовки уже предлагались (удалённую заново не добавляем)
    pub presets: Vec<String>,
}

/// Заготовки: группы Xiaomi (слежка, реклама, сбор отчётов; «по желанию» — с побочными эффектами) и профиль
/// «Приватность», который их отключает. Профиль сам не включается — только вручную или «всегда».
/// Yandex AppMetrica внутри «Безопасности» Xiaomi: пакет целиком трогать нельзя (держит второе пространство), только эти
/// компоненты. Отдельная заготовка: группа «Слежка и реклама Xiaomi» у кого-то уже есть — их туда добавим.
const APPMETRICA: [&str; 2] = [
    "com.miui.securitycenter/io.appmetrica.analytics.internal.AppMetricaService",
    "com.miui.securitycenter/io.appmetrica.analytics.internal.PreloadInfoContentProvider",
];

fn presets() -> (Vec<Group>, Vec<Profile>) {
    let g = |id: &str, name: &str, p: &[&str]| Group {
        id: id.into(),
        name: name.into(),
        members: Vec::new(),
        packages: p.iter().map(|s| s.to_string()).collect(),
        components: Vec::new(),
    };
    (
        vec![
            g(
                "xiaomi_spy",
                "Слежка и реклама Xiaomi",
                &[
                    "com.miui.analytics",
                    "com.miui.msa.global",
                    "com.miui.misightservice",
                    "com.miui.bugreport",
                    "com.bsp.catchlog",
                    "com.xiaomi.mipicks",
                    "com.xiaomi.discover",
                    "com.xiaomi.glgm",
                ],
            ),
            g("xiaomi_opt", "Xiaomi: по желанию", &["com.xiaomi.joyose", "com.xiaomi.xmsf", "com.xiaomi.xmsfkeeper", "com.xiaomi.aicr", "com.xiaomi.aiservice"]),
        ],
        vec![Profile {
            id: "privacy".into(),
            name: "Приватность".into(),
            rules: [("xiaomi_spy".to_string(), "disable".to_string())].into_iter().collect(),
            ..Default::default()
        }],
    )
}

impl Default for Modes {
    fn default() -> Self {
        let g = |id: &str, name: &str| Group { id: id.into(), name: name.into(), members: Vec::new(), packages: Vec::new(), components: Vec::new() };
        let rules = |v: &[(&str, &str)]| v.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect();
        Modes {
            groups: vec![g(KEEP, "Важное"), g("hold", "Придержать"), g("junk", "Мусор")],
            profiles: vec![
                Profile {
                    id: "saver".into(),
                    name: "Экономия".into(),
                    when: When { saver: true, ..Default::default() },
                    rules: rules(&[("hold", "bg"), ("junk", "hard"), (REST, "watch")]),
                    ..Default::default()
                },
                Profile {
                    id: "abroad".into(),
                    name: "За границей".into(),
                    when: When { abroad: true, ..Default::default() },
                    ..Default::default()
                },
            ],
            home_country: String::new(),
            presets: Vec::new(),
        }
    }
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn clip(s: &str, n: usize) -> String {
    s.trim().chars().take(n).collect()
}

impl Modes {
    fn path(dir: &Path) -> std::path::PathBuf {
        dir.join("modes.json")
    }

    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(dir)).ok().and_then(|t| serde_json::from_str::<Modes>(&t).ok()).unwrap_or_default().clamped()
    }

    pub fn save(&self, dir: &Path) {
        let tmp = dir.join("modes.json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, Self::path(dir));
        }
    }

    /// Привести присланное к допустимому: «Важное» есть и первая, уровни из списка.
    pub fn clamped(mut self) -> Self {
        // заготовки — один раз
        let (pg, pp) = presets();
        for g in pg {
            if !self.presets.contains(&g.id) {
                self.presets.push(g.id.clone());
                if !self.groups.iter().any(|x| x.id == g.id) {
                    self.groups.push(g);
                }
            }
        }
        for p in pp {
            if !self.presets.contains(&p.id) {
                self.presets.push(p.id.clone());
                if !self.profiles.iter().any(|x| x.id == p.id) {
                    self.profiles.push(p);
                }
            }
        }
        if !self.presets.iter().any(|x| x == "xiaomi_appmetrica") {
            self.presets.push("xiaomi_appmetrica".into());
            if let Some(g) = self.groups.iter_mut().find(|g| g.id == "xiaomi_spy") {
                g.components.extend(APPMETRICA.iter().map(|c| c.to_string()));
            }
        }
        let mut seen_g = BTreeSet::new();
        self.groups.retain(|g| valid_id(&g.id) && g.id != REST && seen_g.insert(g.id.clone()));
        if !self.groups.iter().any(|g| g.id == KEEP) {
            self.groups.insert(0, Group { id: KEEP.into(), name: "Важное".into(), members: Vec::new(), packages: Vec::new(), components: Vec::new() });
        }
        self.groups.sort_by_key(|g| g.id != KEEP);
        self.groups.truncate(30);
        for g in &mut self.groups {
            g.name = clip(&g.name, 60);
            if g.name.is_empty() {
                g.name = g.id.clone();
            }
            let mut seen_u = BTreeSet::new();
            g.members.retain(|u| crate::explain::uid_kind(*u) == "app" && seen_u.insert(*u));
            let mut seen_p = BTreeSet::new();
            g.packages.retain(|p| compare_valid_pkg(p) && seen_p.insert(p.clone()));
            g.packages.truncate(200);
            let mut seen_c = BTreeSet::new();
            g.components.retain(|c| compare::valid_comp(c) && seen_c.insert(c.clone()));
            g.components.truncate(200);
        }
        let groups: BTreeSet<String> = self.groups.iter().map(|g| g.id.clone()).collect();
        let mut seen_p = BTreeSet::new();
        self.profiles.retain(|p| valid_id(&p.id) && seen_p.insert(p.id.clone()));
        self.profiles.truncate(12);
        for p in &mut self.profiles {
            p.name = clip(&p.name, 60);
            if p.name.is_empty() {
                p.name = p.id.clone();
            }
            p.rules.retain(|k, l| (k == REST || (groups.contains(k) && k != KEEP)) && LEVELS.contains(&l.as_str()));
            // «все остальные» нельзя отключить и лишить сети — это весь телефон
            if let Some(l) = p.rules.get_mut(REST) {
                if lvl(l) >= NONET {
                    *l = "hard".into();
                }
            }
            p.apps.retain(|k, l| k.parse::<u32>().map(|u| crate::explain::uid_kind(u) == "app").unwrap_or(false) && LEVELS.contains(&l.as_str()));
            p.when.below_pct = p.when.below_pct.min(90);
            p.when.night_from %= 1440;
            p.when.night_to %= 1440;
            p.when.bt_away.retain(|n| !n.trim().is_empty() && n.len() <= 100);
            p.when.bt_away.truncate(8);
            if p.when.night_from == p.when.night_to {
                p.when.night_from = 23 * 60;
                p.when.night_to = 7 * 60;
            }
            p.heavy_ma = if p.heavy_ma.is_finite() { p.heavy_ma.clamp(1.0, 500.0) } else { 5.0 };
            p.heavy_rx_h = p.heavy_rx_h.clamp(100, 1_000_000);
            p.idle_h = p.idle_h.clamp(1, 48);
        }
        self.home_country = clip(&self.home_country, 3).to_ascii_lowercase();
        self
    }

    /// Группы копии приложения: по uid и по имени любого из её пакетов.
    pub fn groups_of(&self, uid: u32, pkgs: &[String]) -> Vec<&str> {
        self.groups.iter().filter(|g| g.members.contains(&uid) || pkgs.iter().any(|p| g.packages.contains(p))).map(|g| g.id.as_str()).collect()
    }

    /// Добавить приложение в группу или убрать из неё.
    pub fn set_member(&mut self, uid: u32, group: &str, on: bool) -> bool {
        let Some(g) = self.groups.iter_mut().find(|g| g.id == group) else { return false };
        g.members.retain(|u| *u != uid);
        if on {
            g.members.push(uid);
        }
        true
    }

    /// Правило приложения в профиле (None — как у группы).
    pub fn set_app_rule(&mut self, profile: &str, uid: u32, level: Option<&str>) -> bool {
        let Some(p) = self.profiles.iter_mut().find(|p| p.id == profile) else { return false };
        match level {
            Some(l) if LEVELS.contains(&l) => {
                p.apps.insert(uid.to_string(), l.to_string());
            }
            Some(_) => return false,
            None => {
                p.apps.remove(&uid.to_string());
            }
        }
        true
    }
}

/// Пакеты, которые Жор не ограничивает и не останавливает ни при каком правиле: без них ломается телефон,
/// push у всех приложений, ввод текста, звонки и SMS, root. VPN-клиенты (пакеты с VpnService) — мягко: общие
/// правила («все остальные») их не трогают, но если вы сами положили такое приложение в группу или дали ему своё
/// правило — действует оно (у GetApps, например, есть VPN-служба «ускорителя игр»).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Protect {
    pub pkgs: BTreeSet<String>,
    #[serde(skip)]
    pub vpn: BTreeSet<String>,
    /// предустановленные (системные) пакеты: правило «все остальные» их не трогает — только явные группы
    #[serde(skip)]
    pub system: BTreeSet<String>,
}

const ALWAYS: &[&str] = &[
    crate::watch::APP_PKG,
    "android",
    "com.android.systemui",
    "com.android.phone",
    "com.android.settings",
    "com.android.shell",
    "com.android.vending",
    "com.google.android.gms",
    "com.google.android.gsf",
    "com.google.android.ext.services",
    "com.google.android.permissioncontroller",
    "com.android.permissioncontroller",
    "com.android.networkstack",
    "com.google.android.networkstack",
    "com.android.bluetooth",
    "com.android.nfc",
    "com.miui.home",
    "com.miui.securitycenter",
    // «Безопасность»: securitycore держит второе пространство, lbe — разрешения, powerkeeper — питание
    "com.miui.securitycore",
    "com.lbe.security.miui",
    "com.miui.powerkeeper",
    // удалённая SIM — не слежка
    "com.qualcomm.uimremoteclient",
    "com.qualcomm.uimremoteserver",
    "me.weishu.kernelsu",
    "com.rifsxd.ksunext",
    "com.topjohnwu.magisk",
];

impl Protect {
    /// Постоянный список + держатели ролей (рабочий стол, SMS, звонки, ассистент), клавиатура и VPN-клиенты.
    /// Дёргает `cmd` — звать редко (при обновлении списка пакетов).
    pub fn load(users: &BTreeSet<u32>, vpn: &BTreeSet<String>) -> Self {
        let mut pkgs: BTreeSet<String> = ALWAYS.iter().map(|s| s.to_string()).collect();
        for u in users {
            let user = u.to_string();
            for role in ["android.app.role.HOME", "android.app.role.SMS", "android.app.role.DIALER", "android.app.role.ASSISTANT"] {
                if let Some(t) = util::run_cmd("cmd", &["role", "get-role-holders", "--user", &user, role], 10_000) {
                    pkgs.extend(t.split(|c: char| c.is_whitespace() || c == ';').filter(|p| p.contains('.')).map(str::to_string));
                }
            }
        }
        if let Some(t) = util::run_cmd("settings", &["get", "secure", "default_input_method"], 10_000) {
            if let Some(p) = t.trim().split('/').next().filter(|p| p.contains('.')) {
                pkgs.insert(p.to_string());
            }
        }
        let system: BTreeSet<String> = util::run_cmd("pm", &["list", "packages", "-s"], 20_000)
            .map(|t| t.lines().filter_map(|l| l.trim().strip_prefix("package:")).map(str::to_string).collect())
            .unwrap_or_default();
        Protect { pkgs, vpn: vpn.clone(), system }
    }

    pub fn has(&self, pkg: &str) -> bool {
        self.pkgs.contains(pkg) || pkg.starts_with("com.android.providers.")
    }
}

/// Окружение, которое цикл читает раз в минуту вне замка: экономия энергии, страна сети и SIM.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub saver: bool,
    pub net_country: String,
    pub sim_country: String,
    /// сопряжённые Bluetooth-устройства: (имя, подключено ли сейчас); None — не читали
    pub bt: Option<Vec<(String, bool)>>,
}

/// Сопряжённые устройства из `dumpsys bluetooth_manager` (~60 мс): строки списка «Bonded devices» вида
/// «XX:…:00:01(Public ) => … [ACL BR/EDR:Y LE:N] … Watch Example-001». Подключено — есть ACL по BR/EDR или LE.
pub fn parse_bt(t: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in t.lines() {
        let l = line.trim();
        if l.starts_with("Bonded devices") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if l.ends_with(':') && !l.contains("UUIDs") || l.is_empty() || l.starts_with("Other devices") {
            if l.starts_with("Other devices") || l.is_empty() {
                break;
            }
            continue;
        }
        let Some(i) = l.find("[ACL ") else { continue };
        let acl = &l[i..];
        let connected = acl.split(']').next().map(|a| a.contains(":Y")).unwrap_or(false);
        let name = l.rsplit("] ").next().unwrap_or("").trim().to_string();
        if !name.is_empty() && !name.starts_with('[') {
            out.push((name, connected));
        }
    }
    out
}

/// Прочитать Bluetooth сейчас (для выбора устройства в приложении).
pub fn read_bt() -> Option<Vec<(String, bool)>> {
    util::run_cmd("dumpsys", &["bluetooth_manager"], 10_000).map(|t| parse_bt(&t))
}

impl Runtime {
    /// Обновить список устройств для приложения (без влияния на отсчёт «нет устройства»).
    pub fn set_bt(&mut self, bt: &[(String, bool)]) {
        self.bt = bt.iter().map(|(n, c)| BtDev { name: n.clone(), connected: *c }).collect();
    }
}

impl Env {
    /// `bt` — ещё и Bluetooth (только когда условие кому-то нужно или список давно не обновлялся).
    pub fn read(bt: bool) -> Self {
        let first = |prop: &str| {
            util::run_cmd("getprop", &[prop], 5_000)
                .and_then(|t| t.trim().split(',').map(|s| s.trim().to_ascii_lowercase()).find(|s| s.len() == 2))
                .unwrap_or_default()
        };
        Env {
            saver: util::run_cmd("settings", &["get", "global", "low_power"], 5_000).map(|t| t.trim() == "1").unwrap_or(false),
            net_country: first("gsm.operator.iso-country"),
            sim_country: first("gsm.sim.operator.iso-country"),
            bt: if bt { util::run_cmd("dumpsys", &["bluetooth_manager"], 10_000).map(|t| parse_bt(&t)) } else { None },
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ActiveProfile {
    pub id: String,
    pub name: String,
    /// почему включён: «вручную», «сеть AE», …
    pub why: Vec<String>,
    pub since: i64,
}

/// Правило, которое сейчас действует на приложение (самое строгое из активных профилей).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Effective {
    pub uid: u32,
    /// системный пакет с общим uid (только «отключить»)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkg: Option<String>,
    pub level: &'static str,
    /// профиль, давший это правило
    pub profile: String,
    /// «много ест» уже сработало: фон запрещён до конца профиля
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tripped: bool,
}

/// Что сейчас с профилями (для приложения и цикла).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Runtime {
    pub active: Vec<ActiveProfile>,
    pub saver: bool,
    /// страна сети (последняя известная) и домашняя
    pub country: String,
    pub home: String,
    pub abroad: bool,
    /// сопряжённые Bluetooth-устройства по последнему чтению (для выбора в профиле)
    pub bt: Vec<BtDev>,
    /// uid → (уровень, id профиля)
    #[serde(skip)]
    pub effective: BTreeMap<u32, (u8, String)>,
    #[serde(skip)]
    applied: BTreeSet<u32>,
    /// сработало «много ест»: uid → когда
    #[serde(skip)]
    tripped: HashMap<u32, i64>,
    #[serde(skip)]
    last_eval: i64,
    #[serde(skip)]
    last_screen: Option<bool>,
    #[serde(skip)]
    last_stop_round: i64,
    #[serde(skip)]
    last_watch: i64,
    #[serde(skip)]
    retry: HashMap<u32, i64>,
    /// системные пакеты под правилом «отключить»: (uid, пакет) → профиль
    #[serde(skip)]
    pub sys: BTreeMap<(u32, String), String>,
    #[serde(skip)]
    dis_applied: BTreeSet<(u32, String)>,
    #[serde(skip)]
    net_applied: BTreeSet<u32>,
    #[serde(skip)]
    retry_k: HashMap<(u32, String), i64>,
    /// компоненты под правилом «отключить»: (uid копии пакета, «пакет/класс») → профиль
    #[serde(skip)]
    pub comps: BTreeMap<(u32, String), String>,
    #[serde(skip)]
    comp_applied: BTreeSet<(u32, String)>,
    /// когда устройство последний раз было подключено (или когда начали следить)
    #[serde(skip)]
    bt_seen: HashMap<String, i64>,
    /// когда Bluetooth читали последний раз (давно — условие «нет устройства» не решаем)
    #[serde(skip)]
    bt_at: i64,
    /// `tripped` уже восстановлен из журнала после запуска ядра
    #[serde(skip)]
    trips_restored: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BtDev {
    pub name: String,
    pub connected: bool,
}

impl Runtime {
    pub fn trips_restored(&self) -> bool {
        self.trips_restored
    }

    /// (0.8.1) После перезапуска ядра вспомнить, кого поймало «много ест»: остановка по правилу и
    /// запрет фона, который Жор поставил тогда же и ещё не снял. Иначе первый же пересмотр снял бы
    /// запрет — «до конца профиля» превращалось в «до обновления Жора».
    pub fn restore_trips(&mut self, trips: &[(u32, i64)], claims: &[crate::compare::Restr]) {
        self.trips_restored = true;
        for &(uid, t) in trips {
            if claims.iter().any(|c| c.uid == Some(uid) && c.since >= t - 60_000) {
                self.tripped.insert(uid, t);
            }
        }
    }

    /// Пересмотреть на следующем шаге (поменяли настройки).
    pub fn poke(&mut self) {
        self.last_eval = 0;
    }

    pub fn effective_list(&self) -> Vec<Effective> {
        self.effective
            .iter()
            .map(|(u, (l, p))| Effective { uid: *u, pkg: None, level: level_name(*l), profile: p.clone(), tripped: self.tripped.contains_key(u) })
            .chain(self.sys.iter().map(|((u, pkg), p)| Effective { uid: *u, pkg: Some(pkg.clone()), level: "disable", profile: p.clone(), tripped: false }))
            .chain(self.comps.iter().map(|((u, c), p)| Effective { uid: *u, pkg: Some(c.clone()), level: "disable", profile: p.clone(), tripped: false }))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// запретить фон
    Restrict { uid: u32, pkg: String },
    /// вернуть ограничение профиля
    Unrestrict(Restr),
    /// остановить: by = mode | watch; profile — имя профиля
    Stop { uid: u32, pkg: String, by: &'static str, why: String, profile: String },
    /// отключить пакет у пользователя копии
    Disable { uid: u32, pkg: String, profile: String },
    /// включить обратно
    Enable(Restr),
    /// закрыть сеть; journal — записать в журнал (в первый раз)
    NoNet { uid: u32, pkg: String, journal: bool },
    /// открыть сеть
    Net(Restr),
    /// отключить компонент «пакет/класс» у пользователя копии
    CompDisable { uid: u32, comp: String, profile: String },
    /// включить компонент обратно
    CompEnable(Restr),
}

pub struct Inputs<'a> {
    pub now: i64,
    /// минуты местного времени от полуночи
    pub local_min: u16,
    pub screen_on: bool,
    pub plugged: bool,
    pub level: u8,
    pub env: &'a Env,
    pub pkgs: &'a BTreeMap<u32, Vec<String>>,
    pub protect: &'a Protect,
    /// uid, у которых сейчас есть процессы
    pub running: &'a BTreeSet<u32>,
    /// ограничения профилей по журналу (by == "mode"): запрет фона, отключённые пакеты, закрытая сеть
    pub claims: &'a [Restr],
    pub dis_claims: &'a [Restr],
    pub net_claims: &'a [Restr],
    pub comp_claims: &'a [Restr],
    /// корзины за последние ~35 мин (для «много ест»)
    pub tail: &'a [&'a Bucket],
    /// когда приложение последний раз было на экране (uid → (конец, на экране сейчас)) и с какого
    /// момента есть история; None — истории Android нет, «много ест» не срабатывает
    pub last_top: Option<(&'a HashMap<u32, (i64, bool)>, i64)>,
}

/// Пора ли пересмотреть.
pub fn due(rt: &Runtime, now: i64, screen_on: bool) -> bool {
    rt.last_screen != Some(screen_on) || now - rt.last_eval >= EVAL_MS
}

fn in_window(m: u16, from: u16, to: u16) -> bool {
    if from == to {
        false
    } else if from < to {
        m >= from && m < to
    } else {
        m >= from || m < to
    }
}

fn hhmm(m: u16) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// Почему профиль включён (пусто — выключен). `bt_seen` — когда каждое устройство было подключено последний раз.
fn reasons(p: &Profile, i: &Inputs, abroad: bool, country: &str, bt_seen: &HashMap<String, i64>) -> Vec<String> {
    let w = &p.when;
    let mut out = Vec::new();
    if !p.on {
        return out;
    }
    if w.manual {
        out.push("включён вручную".into());
    }
    if w.always {
        out.push("всегда".into());
    }
    if w.saver && i.env.saver && !i.plugged {
        out.push("экономия энергии Android".into());
    }
    if w.below_pct > 0 && i.level <= w.below_pct && !i.plugged {
        out.push(format!("заряд {} %", i.level));
    }
    if w.abroad && abroad {
        out.push(format!("сеть {}", country.to_ascii_uppercase()));
    }
    if w.night && in_window(i.local_min, w.night_from, w.night_to) {
        out.push(format!("ночь {}–{}", hhmm(w.night_from), hhmm(w.night_to)));
    }
    if !w.bt_away.is_empty() {
        // ни одно не подключено, и каждое пропало дольше BT_GRACE_MS (не знаем — не включаемся)
        let away = w.bt_away.iter().all(|n| bt_seen.get(n).map(|t| i.now - t >= BT_GRACE_MS).unwrap_or(false));
        if away {
            out.push(format!("нет рядом: {}", w.bt_away.join(", ")));
        }
    }
    out
}

fn app_pkg<'a>(pkgs: &'a BTreeMap<u32, Vec<String>>, uid: u32) -> Option<&'a str> {
    if crate::explain::uid_kind(uid) != "app" {
        return None;
    }
    pkgs.get(&uid).and_then(|v| v.first()).map(String::as_str)
}

/// Решить, что сделать. Состояние меняется сразу; сами команды выполняет [execute].
pub fn plan(m: &Modes, rt: &mut Runtime, i: &Inputs) -> Vec<Op> {
    let mut ops = Vec::new();
    let screen_off_now = rt.last_screen == Some(true) && !i.screen_on;
    rt.last_screen = Some(i.screen_on);
    rt.last_eval = i.now;
    rt.saver = i.env.saver;
    // страна: без сети — последняя известная
    if !i.env.net_country.is_empty() {
        rt.country = i.env.net_country.clone();
    }
    rt.home = if m.home_country.is_empty() { i.env.sim_country.clone() } else { m.home_country.clone() };
    rt.abroad = !rt.home.is_empty() && !rt.country.is_empty() && rt.country != rt.home;
    // Bluetooth: подключённые сейчас — «видели только что»; впервые увиденные отключёнными — отсчёт с этого момента
    if let Some(bt) = &i.env.bt {
        rt.bt_at = i.now;
        rt.bt = bt.iter().map(|(n, c)| BtDev { name: n.clone(), connected: *c }).collect();
        for (n, c) in bt {
            if *c {
                rt.bt_seen.insert(n.clone(), i.now);
            } else {
                rt.bt_seen.entry(n.clone()).or_insert(i.now);
            }
        }
        // выбранные в профилях, но не сопряжённые — тоже «пропали» с этого момента
        for p in &m.profiles {
            for n in &p.when.bt_away {
                rt.bt_seen.entry(n.clone()).or_insert(i.now);
            }
        }
    }

    // активные профили
    let mut active = Vec::new();
    for p in &m.profiles {
        // свежего чтения Bluetooth нет — про «нет устройства» не знаем, считаем, что рядом
        let empty = HashMap::new();
        let seen = if i.now - rt.bt_at <= 3 * 60_000 { &rt.bt_seen } else { &empty };
        let why = reasons(p, i, rt.abroad, &rt.country, seen);
        if !why.is_empty() {
            let since = rt.active.iter().find(|a| a.id == p.id).map(|a| a.since).unwrap_or(i.now);
            active.push(ActiveProfile { id: p.id.clone(), name: p.name.clone(), why, since });
        }
    }
    rt.active = active;

    // самое строгое правило каждого приложения
    let mut eff: BTreeMap<u32, (u8, String)> = BTreeMap::new();
    let keep: BTreeSet<u32> = m.groups.iter().filter(|g| g.id == KEEP).flat_map(|g| g.members.iter().copied()).collect();
    let profiles: Vec<&Profile> = rt.active.iter().filter_map(|a| m.profiles.iter().find(|p| p.id == a.id)).collect();
    if !profiles.is_empty() {
        for &uid in i.pkgs.keys() {
            if keep.contains(&uid) {
                continue;
            }
            let Some(pkg) = app_pkg(i.pkgs, uid) else { continue };
            if i.protect.has(pkg) {
                continue;
            }
            let groups = m.groups_of(uid, &i.pkgs[&uid]);
            if groups.contains(&KEEP) {
                continue;
            }
            // VPN-клиент и предустановленное приложение без группы и своего правила — не трогаем:
            // «все остальные» — это ваши приложения, а не системные Google и производителя
            let implicit = groups.is_empty() && !profiles.iter().any(|p| p.apps.contains_key(&uid.to_string()));
            if implicit && (i.protect.vpn.contains(pkg) || i.protect.system.contains(pkg)) {
                continue;
            }
            let best = profiles.iter().map(|p| (p.level(uid, &groups), p.id.clone())).max_by_key(|x| x.0);
            if let Some(b) = best.filter(|x| x.0 > 0) {
                eff.insert(uid, b);
            }
        }
    }
    rt.effective = eff;
    // системные пакеты (общий uid) из групп по имени: им доступно только «отключить», без «всех остальных»
    let mut sys: BTreeMap<(u32, String), String> = BTreeMap::new();
    if !profiles.is_empty() {
        for (&uid, ps) in i.pkgs.iter() {
            if crate::explain::uid_kind(uid) == "app" {
                continue;
            }
            for pkg in ps {
                if i.protect.has(pkg) {
                    continue;
                }
                let groups: Vec<&str> = m.groups.iter().filter(|g| g.packages.contains(pkg)).map(|g| g.id.as_str()).collect();
                if groups.is_empty() || groups.contains(&KEEP) {
                    continue;
                }
                let best = profiles
                    .iter()
                    .filter_map(|p| groups.iter().filter_map(|g| p.rules.get(*g)).map(|l| lvl(l)).min().map(|l| (l, p.id.clone())))
                    .max_by_key(|x| x.0);
                if let Some((DISABLE, prof)) = best {
                    sys.insert((uid, pkg.clone()), prof);
                }
            }
        }
    }
    rt.sys = sys;
    // компоненты: только «отключить»; во всех пространствах, где есть пакет
    let mut comps: BTreeMap<(u32, String), String> = BTreeMap::new();
    if !profiles.is_empty() {
        let all: BTreeSet<&String> = m.groups.iter().filter(|g| g.id != KEEP).flat_map(|g| g.components.iter()).collect();
        for c in all {
            let Some(pkg) = c.split_once('/').map(|x| x.0) else { continue };
            if pkg == crate::watch::APP_PKG || pkg == "android" {
                continue;
            }
            let groups: Vec<&str> = m.groups.iter().filter(|g| g.components.contains(c)).map(|g| g.id.as_str()).collect();
            let best = profiles
                .iter()
                .filter_map(|p| groups.iter().filter_map(|g| p.rules.get(*g)).map(|l| lvl(l)).min().map(|l| (l, p.id.clone())))
                .max_by_key(|x| x.0);
            if let Some((DISABLE, prof)) = best {
                for (&uid, ps) in i.pkgs.iter() {
                    if ps.iter().any(|p| p == pkg) {
                        comps.insert((uid, c.clone()), prof.clone());
                    }
                }
            }
        }
    }
    rt.comps = comps;
    // «много ест» действует, пока правило не ниже watch
    let effective = &rt.effective;
    rt.tripped.retain(|u, _| effective.get(u).map(|x| x.0 >= 1).unwrap_or(false));

    let want_bg = |rt: &Runtime, u: u32| rt.effective.get(&u).map(|x| x.0 >= 2).unwrap_or(false) || rt.tripped.contains_key(&u);
    // ограничения, которые уже не нужны
    for c in i.claims {
        let uid = c.uid.unwrap_or(0);
        if want_bg(rt, uid) {
            rt.applied.insert(uid);
            continue;
        }
        rt.applied.remove(&uid);
        if rt.retry.get(&uid).map(|t| i.now - t < RETRY_MS).unwrap_or(false) {
            continue;
        }
        rt.retry.insert(uid, i.now);
        ops.push(Op::Unrestrict(c.clone()));
    }
    // новые ограничения
    let uids: Vec<u32> = rt.effective.keys().copied().collect();
    for uid in uids {
        if want_bg(rt, uid) && rt.applied.insert(uid) {
            if let Some(pkg) = app_pkg(i.pkgs, uid) {
                ops.push(Op::Restrict { uid, pkg: pkg.to_string() });
            }
        }
    }
    // отключить пакет: приложения с правилом disable и системные пакеты групп
    let mut want_dis: BTreeMap<(u32, String), String> = rt.sys.clone();
    for (&uid, (l, prof)) in &rt.effective {
        if *l >= DISABLE {
            if let Some(pkg) = app_pkg(i.pkgs, uid) {
                want_dis.insert((uid, pkg.to_string()), prof.clone());
            }
        }
    }
    for c in i.dis_claims {
        let k = (c.uid.unwrap_or(0), c.pkg.clone());
        if want_dis.contains_key(&k) {
            rt.dis_applied.insert(k);
            continue;
        }
        rt.dis_applied.remove(&k);
        if rt.retry_k.get(&k).map(|t| i.now - t < RETRY_MS).unwrap_or(false) {
            continue;
        }
        rt.retry_k.insert(k, i.now);
        ops.push(Op::Enable(c.clone()));
    }
    for (k, prof) in &want_dis {
        if rt.dis_applied.insert(k.clone()) {
            let name = m.profiles.iter().find(|p| &p.id == prof).map(|p| p.name.clone()).unwrap_or_default();
            ops.push(Op::Disable { uid: k.0, pkg: k.1.clone(), profile: name });
        }
    }
    // компоненты
    for c in i.comp_claims {
        let k = (c.uid.unwrap_or(0), c.pkg.clone());
        if rt.comps.contains_key(&k) {
            rt.comp_applied.insert(k);
            continue;
        }
        rt.comp_applied.remove(&k);
        let rk = (k.0, format!("c:{}", k.1));
        if rt.retry_k.get(&rk).map(|t| i.now - t < RETRY_MS).unwrap_or(false) {
            continue;
        }
        rt.retry_k.insert(rk, i.now);
        ops.push(Op::CompEnable(c.clone()));
    }
    for (k, prof) in rt.comps.clone() {
        if rt.comp_applied.insert(k.clone()) {
            let name = m.profiles.iter().find(|p| p.id == prof).map(|p| p.name.clone()).unwrap_or_default();
            ops.push(Op::CompDisable { uid: k.0, comp: k.1, profile: name });
        }
    }
    // без сети
    let want_net: BTreeSet<u32> = rt.effective.iter().filter(|(_, x)| x.0 == NONET).map(|(u, _)| *u).collect();
    for c in i.net_claims {
        let uid = c.uid.unwrap_or(0);
        if want_net.contains(&uid) {
            continue;
        }
        rt.net_applied.remove(&uid);
        if rt.retry_k.get(&(uid, "net".to_string())).map(|t| i.now - t < RETRY_MS).unwrap_or(false) {
            continue;
        }
        rt.retry_k.insert((uid, "net".to_string()), i.now);
        ops.push(Op::Net(c.clone()));
    }
    for &uid in &want_net {
        // после перезапуска ядра (и перезагрузки) правило ставится заново; в журнал — только первый раз
        if rt.net_applied.insert(uid) {
            if let Some(pkg) = app_pkg(i.pkgs, uid) {
                let journal = !i.net_claims.iter().any(|c| c.uid == Some(uid));
                ops.push(Op::NoNet { uid, pkg: pkg.to_string(), journal });
            }
        }
    }
    // «жёстко»: остановить при выключении экрана и, если снова поднялись, раз в 30 мин
    let round = screen_off_now || (!i.screen_on && rt.last_stop_round > 0 && i.now - rt.last_stop_round >= STOP_AGAIN_MS);
    if round {
        rt.last_stop_round = i.now;
        for (&uid, (l, prof)) in &rt.effective {
            if (*l == 3 || *l == NONET) && i.running.contains(&uid) {
                if let Some(pkg) = app_pkg(i.pkgs, uid) {
                    let name = m.profiles.iter().find(|p| &p.id == prof).map(|p| p.name.clone()).unwrap_or_default();
                    ops.push(Op::Stop { uid, pkg: pkg.to_string(), by: "mode", why: "жёстко: экран выключен".into(), profile: name });
                }
            }
        }
    }
    if i.screen_on {
        rt.last_stop_round = 0;
    }
    // «только если много ест»
    if !i.screen_on && !i.plugged && i.now - rt.last_watch >= WATCH_EVERY_MS && rt.effective.values().any(|x| x.0 == 1) {
        rt.last_watch = i.now;
        ops.extend(watch(m, rt, i));
    }
    ops
}

/// «Только если много ест»: при выключенном экране за последние 30 мин покоя приложение тратит больше порога —
/// процессор (мА) или входящие пакеты в час — и давно не открывалось. Остановить и запретить фон до конца профиля.
fn watch(m: &Modes, rt: &mut Runtime, i: &Inputs) -> Vec<Op> {
    let Some((last_top, top_since)) = i.last_top else { return Vec::new() };
    let from = i.now - WATCH_WINDOW_MS;
    let mut off_ms = 0f64;
    let mut cpu: HashMap<u32, f64> = HashMap::new();
    let mut rx: HashMap<u32, u64> = HashMap::new();
    for b in i.tail.iter().filter(|b| b.t1 > from && !b.scr && !b.plug) {
        off_ms += b.span as f64;
        for p in &b.procs {
            *cpu.entry(p.h.unwrap_or(p.uid)).or_default() += p.e;
        }
        for r in &b.net {
            *rx.entry(r.u).or_default() += r.rp;
        }
    }
    // меньше 20 минут покоя в окне — судить рано
    if off_ms < 20.0 * 60_000.0 {
        return Vec::new();
    }
    let hours = off_ms / 3_600_000.0;
    let mut out = Vec::new();
    let cands: Vec<(u32, String)> = rt.effective.iter().filter(|(_, x)| x.0 == 1).map(|(u, x)| (*u, x.1.clone())).collect();
    for (uid, prof_id) in cands {
        if rt.tripped.contains_key(&uid) || !i.running.contains(&uid) {
            continue;
        }
        let Some(p) = m.profiles.iter().find(|p| p.id == prof_id) else { continue };
        let ma = cpu.get(&uid).copied().unwrap_or(0.0) / (off_ms / 1000.0);
        let rx_h = rx.get(&uid).copied().unwrap_or(0) as f64 / hours;
        let heavy = if ma >= p.heavy_ma {
            format!("тратило ~{ma:.0} мА в фоне")
        } else if rx_h >= p.heavy_rx_h as f64 {
            format!("принимало ~{rx_h:.0} пакетов в час")
        } else {
            continue;
        };
        let idle_ms = p.idle_h as i64 * 3_600_000;
        // история короче порога — «не открывали» не доказать
        if i.now - top_since < idle_ms {
            continue;
        }
        let opened = match last_top.get(&uid) {
            Some((_, true)) => continue,
            Some((t, false)) if i.now - t < idle_ms => continue,
            Some((t, false)) => format!("не открывали {}", util::fmt_dur((i.now - t) as u64)),
            None => format!("не открывали как минимум {}", util::fmt_dur((i.now - top_since) as u64)),
        };
        let Some(pkg) = app_pkg(i.pkgs, uid) else { continue };
        rt.tripped.insert(uid, i.now);
        out.push(Op::Stop { uid, pkg: pkg.to_string(), by: "watch", why: format!("при выключенном экране {heavy}, {opened}"), profile: p.name.clone() });
        if rt.applied.insert(uid) {
            out.push(Op::Restrict { uid, pkg: pkg.to_string() });
        }
    }
    out
}

/// Остановку пропускаем, если у приложения служба с таким типом: музыка, звонок, навигация,
/// подключённые устройства, камера, микрофон.
const FGS_KEEP: u64 = 0x2 | 0x4 | 0x8 | 0x10 | 0x40 | 0x80;

fn fgs_busy(pkg: &str, uid: u32) -> Option<String> {
    let user = (uid / 100_000).to_string();
    let t = util::run_cmd("dumpsys", &["activity", "services", "--user", &user, pkg], 10_000)?;
    parse_fgs(&t)
}

fn parse_fgs(t: &str) -> Option<String> {
    for tok in t.split_whitespace() {
        if let Some(h) = tok.strip_prefix("types=0x") {
            if let Ok(v) = u64::from_str_radix(h.trim_end_matches(|c: char| !c.is_ascii_hexdigit()), 16) {
                if v & FGS_KEEP != 0 {
                    return Some(format!("служба переднего плана (типы 0x{v:x})"));
                }
            }
        }
    }
    None
}

/// Выполнить решения (вне замка демона: `cmd` — сотни мс). Возвращает строки журнала ядра и тревоги
/// сторожу: «много ест» сообщает, кого и почему остановил.
pub fn execute(dir: &Path, ops: Vec<Op>, mute: &[String]) -> (Vec<String>, Vec<crate::watch::Alert>) {
    let mut log = Vec::new();
    let mut alerts = Vec::new();
    for op in ops {
        match op {
            Op::Restrict { uid, pkg } => match compare::restrict_by(dir, &pkg, Some(uid), "mode") {
                Some(a) => log.push(format!("профили: запрет фона {pkg} ({uid}) — {}", if a.ok { "ок" } else { "не вышло" })),
                None => log.push(format!("профили: {pkg} ({uid}) — фон и так запрещён")),
            },
            Op::Unrestrict(r) => {
                let res = match compare::unrestrict_one(dir, &r, "профиль больше не требует", "mode") {
                    compare::Unrestricted::Done => "вернул".to_string(),
                    compare::Unrestricted::Failed => "не вышло".to_string(),
                    compare::Unrestricted::Skipped(m) => format!("режим уже {}", m.unwrap_or_default()),
                };
                log.push(format!("профили: фон {} ({}) — {res}", r.pkg, r.uid.unwrap_or(0)));
            }
            Op::Disable { uid, pkg, profile } => match compare::disable_by(dir, &pkg, Some(uid), "mode", &format!("«{profile}»")) {
                Some(a) => log.push(format!("профиль «{profile}»: отключил {pkg} ({uid}) — {}", if a.ok { "ок" } else { "не вышло" })),
                None => log.push(format!("профиль «{profile}»: {pkg} ({uid}) и так отключён")),
            },
            Op::Enable(r) => {
                let res = match compare::enable_one(dir, &r, "профиль больше не требует", "mode") {
                    compare::Unrestricted::Done => "включил".to_string(),
                    compare::Unrestricted::Failed => "не вышло".to_string(),
                    compare::Unrestricted::Skipped(_) => "уже включён".to_string(),
                };
                log.push(format!("профили: {} ({}) — {res}", r.pkg, r.uid.unwrap_or(0)));
            }
            Op::NoNet { uid, pkg, journal } => {
                let ok = compare::nonet_by(dir, &pkg, uid, "mode", journal);
                log.push(format!("профили: без сети {pkg} ({uid}) — {}", if ok { "ок" } else { "не вышло" }));
            }
            Op::Net(r) => {
                let ok = matches!(compare::net_one(dir, &r, "профиль больше не требует", "mode"), compare::Unrestricted::Done);
                log.push(format!("профили: сеть {} ({}) — {}", r.pkg, r.uid.unwrap_or(0), if ok { "вернул" } else { "не вышло" }));
            }
            Op::CompDisable { uid, comp, profile } => match compare::disable_comp_by(dir, &comp, Some(uid), "mode", &format!("«{profile}»")) {
                Some(a) => log.push(format!("профиль «{profile}»: отключил компонент {comp} ({uid}) — {}", if a.ok { "ок" } else { "не вышло" })),
                None => log.push(format!("профиль «{profile}»: компонент {comp} ({uid}) и так отключён")),
            },
            Op::CompEnable(r) => {
                let res = match compare::enable_comp_one(dir, &r, "профиль больше не требует", "mode") {
                    compare::Unrestricted::Done => "включил".to_string(),
                    compare::Unrestricted::Failed => "не вышло".to_string(),
                    compare::Unrestricted::Skipped(_) => "уже включён".to_string(),
                };
                log.push(format!("профили: компонент {} ({}) — {res}", r.pkg, r.uid.unwrap_or(0)));
            }
            Op::Stop { uid, pkg, by, why, profile } => {
                if let Some(busy) = fgs_busy(&pkg, uid) {
                    log.push(format!("профиль «{profile}»: {pkg} не останавливаю — {busy}"));
                    continue;
                }
                let a = compare::stop_by(dir, &pkg, Some(uid), by, &format!("«{profile}»: {why}"));
                log.push(format!("профиль «{profile}»: остановил {pkg} ({uid}) — {why}{}", if a.ok { "" } else { " — не вышло" }));
                if by == "watch" && a.ok {
                    let key = format!("watch:{pkg}");
                    if mute.iter().any(|k| k == &key || k == "watch") {
                        continue;
                    }
                    let now = util::real_ms();
                    alerts.push(crate::watch::Alert {
                        id: format!("{key}@{now}"),
                        t: now,
                        rule: "watch".into(),
                        key,
                        level: "info".into(),
                        title: format!("Жор остановил {pkg}"),
                        text: format!(
                            "Профиль «{profile}», правило «только если много ест»: {pkg} {why}. Фон ему запрещён, пока профиль включён; откроете — запустится как обычно. Чтобы не трогать, поставьте ему «не трогать» в этом профиле или добавьте в «Важное»."
                        ),
                        uid: Some(uid),
                        pkg: Some(pkg.clone()),
                        from: now - WATCH_WINDOW_MS,
                        to: now,
                        sent: false,
                    });
                }
            }
        }
    }
    (log, alerts)
}

/// Минуты местного времени от полуночи.
pub fn local_min(now: i64) -> u16 {
    let t = now + crate::store::local_offset_ms(now);
    (t.rem_euclid(86_400_000) / 60_000) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NetRec, ProcRec};

    fn pk() -> BTreeMap<u32, Vec<String>> {
        let mut m = BTreeMap::new();
        for (u, p) in [(10_100, "com.example.social"), (10_200, "com.example.junk"), (10_300, "com.example.bank"), (10_400, "com.example.hog"), (10_500, "com.example.vpn")] {
            m.insert(u, vec![p.to_string()]);
        }
        m
    }

    fn modes() -> Modes {
        let mut m = Modes::default();
        m.groups.push(Group { id: "banks".into(), name: "Банки".into(), members: vec![10_300], packages: vec![], components: vec![] });
        m.set_member(10_100, "hold", true);
        m.set_member(10_200, "junk", true);
        m.profiles[1].rules.insert("banks".into(), "watch".into());
        m.clamped()
    }

    struct Fx {
        p: BTreeMap<u32, Vec<String>>,
        pr: Protect,
        running: BTreeSet<u32>,
    }

    fn fx() -> Fx {
        let mut pr = Protect::default();
        pr.vpn.insert("com.example.vpn".into());
        Fx { p: pk(), pr, running: [10_100, 10_200, 10_300, 10_400, 10_500].into_iter().collect() }
    }

    fn inp<'a>(f: &'a Fx, env: &'a Env, now: i64, screen_on: bool, claims: &'a [Restr]) -> Inputs<'a> {
        Inputs {
            now,
            local_min: 12 * 60,
            screen_on,
            plugged: false,
            level: 50,
            env,
            pkgs: &f.p,
            protect: &f.pr,
            running: &f.running,
            claims,
            dis_claims: &[],
            net_claims: &[],
            comp_claims: &[],
            tail: &[],
            last_top: None,
        }
    }

    #[test]
    fn saver_profile_applies_and_reverts() {
        let f = fx();
        let m = modes();
        let mut rt = Runtime::default();
        let home = Env { saver: false, net_country: "ru".into(), sim_country: "ru".into(), bt: None };
        assert!(plan(&m, &mut rt, &inp(&f, &home, 0, true, &[])).is_empty());
        assert!(rt.active.is_empty());
        let saver = Env { saver: true, ..home.clone() };
        let ops = plan(&m, &mut rt, &inp(&f, &saver, 60_000, true, &[]));
        assert_eq!(rt.active.len(), 1);
        assert_eq!(rt.active[0].why, vec!["экономия энергии Android".to_string()]);
        // «Придержать» — запрет фона, «Мусор» — жёстко, без группы — watch (сразу ничего), VPN без группы не трогаем
        assert_eq!(ops, vec![Op::Restrict { uid: 10_100, pkg: "com.example.social".into() }, Op::Restrict { uid: 10_200, pkg: "com.example.junk".into() }]);
        assert_eq!(rt.effective.get(&10_400).map(|x| x.0), Some(1));
        assert!(!rt.effective.contains_key(&10_500));
        // экран выключился — «Мусор» останавливаем
        let ops = plan(&m, &mut rt, &inp(&f, &saver, 120_000, false, &[]));
        assert!(matches!(&ops[..], [Op::Stop { uid: 10_200, by: "mode", .. }]), "{ops:?}");
        // экономия выключилась — ограничения профиля возвращаем
        let c = Restr { pkg: "com.example.social".into(), uid: Some(10_100), since: 60_000, prev: "allow".into(), by: "mode".into(), kind: "bg" };
        let ops = plan(&m, &mut rt, &inp(&f, &home, 180_000, true, std::slice::from_ref(&c)));
        assert!(rt.active.is_empty());
        assert_eq!(ops, vec![Op::Unrestrict(c)]);
    }

    #[test]
    fn abroad_and_strictest_wins() {
        let f = fx();
        let mut m = modes();
        let mut rt = Runtime::default();
        let abroad = Env { saver: false, net_country: "ae".into(), sim_country: "ru".into(), bt: None };
        let ops = plan(&m, &mut rt, &inp(&f, &abroad, 0, true, &[]));
        assert!(rt.abroad);
        assert_eq!(rt.active[0].why, vec!["сеть AE".to_string()]);
        assert!(ops.is_empty());
        assert_eq!(rt.effective.get(&10_300).map(|x| x.0), Some(1));
        // нет сети — страна последняя известная
        let nosig = Env { saver: false, net_country: String::new(), sim_country: "ru".into(), bt: None };
        plan(&m, &mut rt, &inp(&f, &nosig, 60_000, true, &[]));
        assert!(rt.abroad);
        // правило приложения поверх группы
        m.set_app_rule("abroad", 10_300, Some("hard"));
        rt.poke();
        let ops = plan(&m, &mut rt, &inp(&f, &abroad, 120_000, true, &[]));
        assert_eq!(ops, vec![Op::Restrict { uid: 10_300, pkg: "com.example.bank".into() }]);
        // две причины сразу — берётся самое строгое: экономия даёт «Придержать» запрет фона
        let both = Env { saver: true, ..abroad.clone() };
        plan(&m, &mut rt, &inp(&f, &both, 150_000, true, &[]));
        assert_eq!(rt.active.len(), 2);
        assert_eq!(rt.effective.get(&10_100).map(|x| x.0), Some(2));
        // «Важное» не трогают никакие правила
        m.set_member(10_300, KEEP, true);
        let ops = plan(&m, &mut rt, &inp(&f, &abroad, 180_000, true, &[]));
        assert!(!rt.effective.contains_key(&10_300), "{ops:?}");
        // дома — профиль выключен
        let home = Env { saver: false, net_country: "ru".into(), sim_country: "ru".into(), bt: None };
        plan(&m, &mut rt, &inp(&f, &home, 240_000, true, &[]));
        assert!(rt.active.is_empty() && !rt.abroad);
    }

    #[test]
    fn several_groups_mildest_wins_in_profile() {
        let mut m = Modes::default();
        m.groups.push(Group { id: "social".into(), name: "Соцсети".into(), members: vec![10_100], packages: vec![], components: vec![] });
        m.groups.push(Group { id: "banks".into(), name: "Банки".into(), members: vec![10_100, 10_300], packages: vec![], components: vec![] });
        m.groups.push(Group { id: "promo".into(), name: "Акции".into(), members: vec![10_400], packages: vec![], components: vec![] });
        let mut p = Profile::default();
        p.rules.insert("social".into(), "hard".into());
        p.rules.insert("banks".into(), "none".into());
        p.rules.insert(REST.into(), "watch".into());
        let m = m.clamped();
        // соцсеть и банк сразу: «не трогать» сильнее «жёстко»
        let no: Vec<String> = Vec::new();
        assert_eq!(p.level(10_100, &m.groups_of(10_100, &no)), 0);
        assert_eq!(p.level(10_300, &m.groups_of(10_300, &no)), 0);
        // группа без правила в этом профиле — как «все остальные»
        assert_eq!(p.level(10_400, &m.groups_of(10_400, &no)), 1);
        assert_eq!(p.level(10_999, &[]), 1);
        // своё правило поверх групп
        p.apps.insert("10100".into(), "bg".into());
        assert_eq!(p.level(10_100, &m.groups_of(10_100, &no)), 2);
        // по имени пакета — во всех пространствах
        let mut m2 = m.clone();
        m2.groups.push(Group { id: "spy".into(), name: "Слежка".into(), members: vec![], packages: vec!["com.example.ads".into()], components: vec![] });
        assert_eq!(m2.groups_of(1_010_777, &["com.example.ads".to_string()]), vec!["spy"]);
    }

    #[test]
    fn privacy_disables_spy_packages_everywhere() {
        let mut p = pk();
        p.insert(1000, vec!["android".into(), "com.miui.bugreport".into(), "com.miui.securitycenter".into(), "com.miui.securitycore".into()]);
        p.insert(1_001_000, vec!["com.miui.securitycenter".into()]);
        p.insert(10_191, vec!["com.miui.analytics".into()]);
        p.insert(1_010_191, vec!["com.miui.analytics".into()]);
        let mut pr = Protect::default();
        pr.pkgs.insert("com.miui.securitycore".into());
        pr.pkgs.insert("com.miui.securitycenter".into());
        pr.pkgs.insert("android".into());
        let f = Fx { p, pr, running: BTreeSet::new() };
        let mut m = Modes::default().clamped();
        // убрать securitycore нельзя даже если положить в группу
        m.groups.iter_mut().find(|g| g.id == "xiaomi_spy").unwrap().packages.push("com.miui.securitycore".into());
        let env = Env { saver: false, net_country: "ru".into(), sim_country: "ru".into(), bt: None };
        let mut rt = Runtime::default();
        // профиль выключен — ничего
        assert!(plan(&m, &mut rt, &inp(&f, &env, 0, true, &[])).is_empty());
        m.profiles.iter_mut().find(|p| p.id == "privacy").unwrap().when.manual = true;
        let ops = plan(&m, &mut rt, &inp(&f, &env, 60_000, true, &[]));
        let dis: Vec<(u32, String)> = ops.iter().filter_map(|o| if let Op::Disable { uid, pkg, .. } = o { Some((*uid, pkg.clone())) } else { None }).collect();
        assert_eq!(dis, vec![(1000, "com.miui.bugreport".into()), (10_191, "com.miui.analytics".into()), (1_010_191, "com.miui.analytics".into())]);
        // AppMetrica внутри защищённого securitycenter — только компоненты, в обоих пространствах
        let comps: Vec<(u32, String)> = ops.iter().filter_map(|o| if let Op::CompDisable { uid, comp, .. } = o { Some((*uid, comp.clone())) } else { None }).collect();
        assert_eq!(comps.len(), 4, "{comps:?}");
        assert!(comps.contains(&(1000, APPMETRICA[0].to_string())) && comps.contains(&(1_001_000, APPMETRICA[1].to_string())));
        assert!(!dis.iter().any(|(_, p)| p == "com.miui.securitycenter"));
        // выключили профиль — включить обратно то, что отключил профиль
        m.profiles.iter_mut().find(|p| p.id == "privacy").unwrap().when.manual = false;
        let c = Restr { pkg: "com.miui.bugreport".into(), uid: Some(1000), since: 60_000, prev: "enabled".into(), by: "mode".into(), kind: "disable" };
        let mut i = inp(&f, &env, 120_000, true, &[]);
        let cs = [c.clone()];
        i.dis_claims = &cs;
        assert_eq!(plan(&m, &mut rt, &i), vec![Op::Enable(c)]);
    }

    #[test]
    fn nonet_rule_once_and_back() {
        let f = fx();
        let mut m = modes();
        m.profiles[1].when.manual = true;
        m.profiles[1].rules.insert("banks".into(), "nonet".into());
        let env = Env { saver: false, net_country: "ru".into(), sim_country: "ru".into(), bt: None };
        let mut rt = Runtime::default();
        let ops = plan(&m, &mut rt, &inp(&f, &env, 0, true, &[]));
        assert!(ops.contains(&Op::NoNet { uid: 10_300, pkg: "com.example.bank".into(), journal: true }), "{ops:?}");
        assert!(ops.contains(&Op::Restrict { uid: 10_300, pkg: "com.example.bank".into() }));
        assert!(!plan(&m, &mut rt, &inp(&f, &env, 60_000, true, &[])).iter().any(|o| matches!(o, Op::NoNet { .. })));
        m.profiles[1].when.manual = false;
        let c = Restr { pkg: "com.example.bank".into(), uid: Some(10_300), since: 0, prev: String::new(), by: "mode".into(), kind: "nonet" };
        let cs = [c.clone()];
        let mut i = inp(&f, &env, 120_000, true, &[]);
        i.net_claims = &cs;
        assert!(plan(&m, &mut rt, &i).contains(&Op::Net(c)));
    }

    #[test]
    fn rest_rule_skips_preinstalled_apps() {
        let mut f = fx();
        f.pr.system.insert("com.example.hog".into());
        let m = modes();
        let env = Env { saver: true, net_country: "ru".into(), sim_country: "ru".into(), bt: None };
        let mut rt = Runtime::default();
        plan(&m, &mut rt, &inp(&f, &env, 0, true, &[]));
        // «Экономия»: все остальные — watch, но предустановленный hog не трогаем
        assert!(!rt.effective.contains_key(&10_400));
        // а в явной группе — да
        let mut m2 = m.clone();
        m2.set_member(10_400, "junk", true);
        rt.poke();
        plan(&m2, &mut rt, &inp(&f, &env, 60_000, true, &[]));
        assert_eq!(rt.effective.get(&10_400).map(|x| x.0), Some(3));
    }

    #[test]
    fn vpn_client_only_by_explicit_group() {
        let f = fx();
        let mut m = modes();
        let env = Env { saver: true, net_country: "ru".into(), sim_country: "ru".into(), bt: None };
        let mut rt = Runtime::default();
        plan(&m, &mut rt, &inp(&f, &env, 0, true, &[]));
        assert!(!rt.effective.contains_key(&10_500));
        // сами положили в «Мусор» — правило действует
        m.set_member(10_500, "junk", true);
        rt.poke();
        let ops = plan(&m, &mut rt, &inp(&f, &env, 60_000, true, &[]));
        assert!(ops.contains(&Op::Restrict { uid: 10_500, pkg: "com.example.vpn".into() }), "{ops:?}");
    }

    #[test]
    fn bt_device_away_with_grace() {
        let f = fx();
        let mut m = modes();
        m.groups.push(Group { id: "watch".into(), name: "Часы".into(), members: vec![], packages: vec!["com.example.junk".into()], components: vec![] });
        m.profiles.push(Profile {
            id: "nowatch".into(),
            name: "Без часов".into(),
            when: When { bt_away: vec!["Watch Example-001".into()], ..Default::default() },
            rules: [("watch".to_string(), "bg".to_string())].into_iter().collect(),
            ..Default::default()
        });
        let m = m.clamped();
        let on = Env { bt: Some(vec![("Watch Example-001".into(), true), ("Headphones X".into(), false)]), ..Default::default() };
        let off = Env { bt: Some(vec![("Watch Example-001".into(), false)]), ..Default::default() };
        let mut rt = Runtime::default();
        plan(&m, &mut rt, &inp(&f, &on, 0, true, &[]));
        assert!(rt.active.is_empty());
        assert_eq!(rt.bt.len(), 2);
        // часы пропали — 5 минут ещё ждём
        plan(&m, &mut rt, &inp(&f, &off, 5 * 60_000, true, &[]));
        assert!(rt.active.is_empty());
        // больше 10 минут — профиль включился
        let ops = plan(&m, &mut rt, &inp(&f, &off, 11 * 60_000, true, &[]));
        assert_eq!(rt.active[0].why, vec!["нет рядом: Watch Example-001".to_string()]);
        assert!(ops.contains(&Op::Restrict { uid: 10_200, pkg: "com.example.junk".into() }), "{ops:?}");
        // вернулись — выключился сразу
        plan(&m, &mut rt, &inp(&f, &on, 12 * 60_000, true, &[]));
        assert!(rt.active.is_empty());
        // без чтения Bluetooth — ничего не решаем
        let none = Env::default();
        plan(&m, &mut rt, &inp(&f, &none, 30 * 60_000, true, &[]));
        assert!(rt.active.is_empty());
    }

    #[test]
    fn parses_bonded_devices() {
        let t = "BluetoothRemoteDevices\n  Bonded devices: 2\n    XX:XX:XX:XX:00:01(Public ) => XX:XX:XX:XX:00:01(Public ) [ DUAL ] [0x280704] [Pairing Algorithm BR/EDR: N/A LE: N/A] [ACL BR/EDR:Y LE:N] [ Encryption status(BR/EDR): EncryptionStatus{keySize=16, algorithm=2] LE: N/A] Watch Example-001\n        [BR/EDR UUIDs]: 0000110a-0000\n    XX:XX:XX:XX:9B:A1(Public ) => XX:XX:XX:XX:9B:A1(Public ) [ DUAL ] [0x240404] [Pairing Algorithm BR/EDR: N/A LE: N/A] [ACL BR/EDR:N LE:N] [ Encryption status(BR/EDR): N/A LE: N/A] Headphones X\n\n  Other devices: 4\n    XX:XX(Public ) [ACL BR/EDR:N LE:N] [x] Xiaomi Tag\n";
        assert_eq!(parse_bt(t), vec![("Watch Example-001".to_string(), true), ("Headphones X".to_string(), false)]);
    }

    #[test]
    fn night_window() {
        assert!(in_window(23 * 60, 22 * 60, 7 * 60));
        assert!(in_window(3 * 60, 22 * 60, 7 * 60));
        assert!(!in_window(12 * 60, 22 * 60, 7 * 60));
        assert!(in_window(2 * 60, 60, 8 * 60));
        assert!(!in_window(5, 0, 0));
    }

    #[test]
    fn watch_needs_heavy_idle_and_history() {
        let f = fx();
        let mut m = modes();
        m.profiles[1].when = When { manual: true, ..Default::default() };
        let now = 10 * 3_600_000;
        // покой с 30 мин до now и ещё 30 мин после: банк — 1 мА CPU, но 6000 пакетов/ч;
        // «прожору» без группы в этом профиле не трогаем (rest = none)
        let bs: Vec<Bucket> = (0..60)
            .map(|k| Bucket {
                t0: now - (30 - k) * 60_000,
                t1: now - (29 - k) * 60_000,
                span: 60_000,
                procs: vec![
                    ProcRec { uid: 10_300, n: "com.example.bank".into(), e: 60.0, ..Default::default() },
                    ProcRec { uid: 10_400, n: "com.example.hog".into(), e: 600.0, ..Default::default() },
                ],
                net: vec![NetRec { u: 10_300, rp: 100, rb: 100_000, tp: 0, tb: 0 }],
                ..Default::default()
            })
            .collect();
        let tail: Vec<&Bucket> = bs.iter().collect();
        let env = Env { saver: false, net_country: "ru".into(), sim_country: "ru".into(), bt: None };
        let empty: HashMap<u32, (i64, bool)> = HashMap::new();
        let recent: HashMap<u32, (i64, bool)> = [(10_300u32, (now - 3_600_000, false))].into_iter().collect();
        let mut rt = Runtime { last_screen: Some(false), ..Default::default() };
        let mut i = inp(&f, &env, now, false, &[]);
        i.tail = &tail;
        // нет истории — не трогаем
        assert!(plan(&m, &mut rt, &i).is_empty());
        // открывали час назад — не трогаем
        i.last_top = Some((&recent, now - 20 * 3_600_000));
        i.now += WATCH_EVERY_MS;
        assert!(plan(&m, &mut rt, &i).is_empty());
        // давно не открывали: много пакетов → стоп и запрет фона
        i.last_top = Some((&empty, now - 20 * 3_600_000));
        i.now += WATCH_EVERY_MS;
        let ops = plan(&m, &mut rt, &i);
        assert!(matches!(&ops[..], [Op::Stop { uid: 10_300, by: "watch", .. }, Op::Restrict { uid: 10_300, .. }]), "{ops:?}");
        if let Op::Stop { why, .. } = &ops[0] {
            assert!(why.contains("пакетов"), "{why}");
        }
        assert!(rt.effective_list().iter().any(|e| e.uid == 10_300 && e.tripped));
        assert!(!rt.effective.contains_key(&10_400));
        // повторно — нет
        i.now += WATCH_EVERY_MS;
        assert!(plan(&m, &mut rt, &i).is_empty());
        let c = Restr { pkg: "com.example.bank".into(), uid: Some(10_300), since: i.now - WATCH_EVERY_MS, prev: "allow".into(), by: "mode".into(), kind: "bg" };
        // ядро перезапустили: без журнала запрет сняли бы сразу, с журналом — держим до конца профиля
        let mut fresh = Runtime::default();
        i.claims = std::slice::from_ref(&c);
        i.now += WATCH_EVERY_MS;
        assert!(matches!(plan(&m, &mut fresh, &i).first(), Some(Op::Unrestrict(_))));
        let mut fresh = Runtime::default();
        fresh.restore_trips(&[(10_300, c.since)], std::slice::from_ref(&c));
        assert_eq!(plan(&m, &mut fresh, &i), vec![]);
        assert!(fresh.effective_list().iter().any(|e| e.uid == 10_300 && e.tripped));
        // профиль выключили — запрет фона по «много ест» вернуть
        m.profiles[1].when.manual = false;
        i.now += WATCH_EVERY_MS;
        assert_eq!(plan(&m, &mut rt, &i), vec![Op::Unrestrict(c.clone())]);
    }

    #[test]
    fn config_clamps() {
        let m: Modes = serde_json::from_str(
            r#"{"groups":[{"id":"banks","name":"Банки","members":[10100,10100,1000]},{"id":"x y","name":"плохой"},{"id":"chat","members":[10100,10200]}],
                "profiles":[{"id":"p","name":"","rules":{"banks":"watch","keep":"hard","nope":"bg","rest":"weird"},"heavy_ma":-1,"idle_h":0}],"home_country":"RU"}"#,
        )
        .unwrap();
        let m = m.clamped();
        assert_eq!(m.groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec![KEEP, "banks", "chat", "xiaomi_spy", "xiaomi_opt"]);
        assert_eq!(m.groups[1].members, vec![10_100]);
        assert_eq!(m.groups[2].members, vec![10_100, 10_200]);
        assert_eq!(m.groups[2].name, "chat");
        let p = &m.profiles[0];
        assert_eq!(p.rules.len(), 1);
        assert_eq!(p.name, "p");
        assert_eq!(p.heavy_ma, 1.0);
        assert_eq!(p.idle_h, 1);
        assert_eq!(m.home_country, "ru");
        let d = Modes::default().clamped();
        assert_eq!(d.profiles.len(), 3);
        assert!(d.profiles[1].when.abroad);
        // заготовки: удалённая не возвращается
        assert!(d.groups.iter().any(|g| g.id == "xiaomi_spy"));
        let mut d2 = d.clone();
        d2.groups.retain(|g| g.id != "xiaomi_spy");
        assert!(!d2.clamped().groups.iter().any(|g| g.id == "xiaomi_spy"));
        // «все остальные» нельзя отключить
        let mut d3 = d.clone();
        d3.profiles[0].rules.insert(REST.into(), "disable".into());
        assert_eq!(d3.clamped().profiles[0].rules[REST], "hard");
    }

    #[test]
    fn fgs_types() {
        assert!(parse_fgs("    isForeground=true foregroundId=9999 types=0x00000001 foregroundNoti=").is_none());
        assert!(parse_fgs("isForeground=true types=0x00000002").is_some());
    }
}
