//! (0.8) Сеть по приложениям: сколько пакетов каждое приняло и отправило.
//!
//! Счётчики — из `dumpsys netstats`, раздел «BPF map content» → `mAppUidStatsMap`: принято и
//! отправлено байт и пакетов на uid с загрузки, по всем интерфейсам. Дамп короткий (~30 мс), его
//! снимаем раз в 5 минут и на каждой смене экрана или зарядки — тогда разность между замерами
//! целиком относится к одному состоянию (покой или экран включён).
//!
//! Зачем: при выключенном экране каждый входящий пакет будит процессор (у Qualcomm — wakelock
//! драйвера Wi-Fi `qcom_rx_wakelock`). Какое приложение получило конкретный будящий пакет, Android
//! знает, только если драйвер помечает такие пакеты (`dumpsys connmetrics` → packet wakeup events);
//! Xiaomi их не помечает. Принятые в покое пакеты по приложениям — ближайшая замена.
//!
//! VPN: трафик приложений через туннель считается им самим, а зашифрованный — ещё и VPN-клиенту.
//! Поэтому клиент VPN в долях не участвует (иначе всё посчитается дважды).

use crate::util;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cnt {
    pub rb: u64,
    pub rp: u64,
    pub tb: u64,
    pub tp: u64,
}

/// Разбор `dumpsys netstats`: строки «uid rxBytes rxPackets txBytes txPackets» после
/// заголовка `mAppUidStatsMap:` (строку статуса «mAppUidStatsMap: OK» пропускаем).
pub fn parse(text: &str) -> BTreeMap<u32, Cnt> {
    let mut out = BTreeMap::new();
    let mut inside = false;
    for line in text.lines() {
        let l = line.trim();
        if l == "mAppUidStatsMap:" {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if l.starts_with("uid ") {
            continue;
        }
        let f: Vec<&str> = l.split_whitespace().collect();
        let nums: Option<Vec<u64>> = if f.len() == 5 { f.iter().map(|x| x.parse::<u64>().ok()).collect() } else { None };
        match nums {
            Some(n) => {
                let e: &mut Cnt = out.entry(n[0] as u32).or_default();
                e.rb += n[1];
                e.rp += n[2];
                e.tb += n[3];
                e.tp += n[4];
            }
            None => break,
        }
    }
    out
}

/// Снять счётчики. None — дамп не прочитался или в нём нет раздела.
pub fn take() -> Option<BTreeMap<u32, Cnt>> {
    util::run_cmd("dumpsys", &["netstats"], 10_000).map(|t| parse(&t)).filter(|m| !m.is_empty())
}

/// Разность b − a по uid; пропадает uid, у которого ничего не изменилось. Счётчик uid, ставший
/// меньше (сброс карты), не учитываем — иначе в покой попадёт трафик за всё время.
pub fn delta(a: &BTreeMap<u32, Cnt>, b: &BTreeMap<u32, Cnt>) -> Vec<(u32, Cnt)> {
    let mut out = Vec::new();
    for (uid, cb) in b {
        let ca = a.get(uid).copied().unwrap_or_default();
        if cb.rp < ca.rp || cb.tp < ca.tp || cb.rb < ca.rb || cb.tb < ca.tb {
            continue;
        }
        let d = Cnt { rb: cb.rb - ca.rb, rp: cb.rp - ca.rp, tb: cb.tb - ca.tb, tp: cb.tp - ca.tp };
        if d.rp > 0 || d.tp > 0 {
            out.push((*uid, d));
        }
    }
    out
}

/// Пакеты с VPN-службой (`android.net.VpnService`) во всех пользователях телефона.
pub fn vpn_packages(users: &BTreeSet<u32>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for u in users {
        let user = u.to_string();
        let Some(text) = util::run_cmd("pm", &["query-services", "--brief", "--user", &user, "-a", "android.net.VpnService"], 15_000) else {
            continue;
        };
        out.extend(parse_services(&text));
    }
    out
}

fn parse_services(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.contains(' ') && l.contains('/'))
        .filter_map(|l| l.split('/').next())
        .filter(|p| p.contains('.'))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = "BPF map status:\n  mAppUidStatsMap: OK\n  mStatsMapA: OK\nBPF map content:\n  mAppUidStatsMap:\n    uid rxBytes rxPackets txBytes txPackets\n    10123 9000000 40000 4000000 30000\n    1010123 1500000000 1200000 100000000 900000\n  mStatsMapA:\n    ifaceIndex ifaceName tag_hex uid_int cnt_set rxBytes rxPackets txBytes txPackets\n    23 wlan0 0x0 10456 0 104 2 104 2\n";

    #[test]
    fn parses_uid_map_only() {
        let m = parse(DUMP);
        assert_eq!(m.len(), 2);
        assert_eq!(m[&10123], Cnt { rb: 9000000, rp: 40000, tb: 4000000, tp: 30000 });
        assert_eq!(m[&1010123].rp, 1200000);
        assert!(!m.contains_key(&23));
    }

    #[test]
    fn delta_skips_resets_and_idle() {
        let a = parse(DUMP);
        let mut b = a.clone();
        b.get_mut(&10123).unwrap().rp += 10;
        b.get_mut(&10123).unwrap().rb += 1500;
        b.get_mut(&1010123).unwrap().rp = 5; // сброс
        b.insert(10999, Cnt { rb: 100, rp: 1, tb: 0, tp: 0 });
        let d = delta(&a, &b);
        assert_eq!(d, vec![(10123, Cnt { rb: 1500, rp: 10, tb: 0, tp: 0 }), (10999, Cnt { rb: 100, rp: 1, tb: 0, tp: 0 })]);
    }

    #[test]
    fn vpn_services() {
        let t = "9 services found:\n  Service #0:\n    priority=0 preferredOrder=0 match=0x108000 specificIndex=-1 isDefault=false\n    com.example.vpnone/.service.VpnService\n  Service #1:\n    com.example.vpntwo/.service.CoreVpnService\n";
        let s = parse_services(t);
        assert!(s.contains("com.example.vpnone") && s.contains("com.example.vpntwo"));
        assert_eq!(s.len(), 2);
    }
}
