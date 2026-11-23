//! Разбор `dumpsys batterystats --usage`: модель самого Android —
//! мА·ч по каждому UID с разбивкой по компонентам (cpu, экран, GPS, радио, Wi-Fi,
//! датчики, аудио, видео, камера, wakelock…) и по состояниям (fg/bg/fgs/cached).
//! Счётчики накопительные «с последней зарядки».

use crate::util::parse_dumpsys_dur;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Comp {
    #[serde(default, skip_serializing_if = "crate::util::is_zero_f")]
    pub mah: f64,
    #[serde(default, skip_serializing_if = "crate::util::is_zero")]
    pub ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UidUsage {
    pub total: f64,
    /// fg / bg / fgs / cached → (мА·ч, мс)
    pub states: BTreeMap<String, Comp>,
    /// компонент («cpu», «cpu:bg», «gnss», …)
    pub comps: BTreeMap<String, Comp>,
    pub screen_on: BTreeMap<String, Comp>,
    pub screen_off: BTreeMap<String, Comp>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageSnap {
    pub capacity: f64,
    pub computed: f64,
    pub actual_lo: f64,
    pub actual_hi: f64,
    pub global: BTreeMap<String, Comp>,
    pub global_on: BTreeMap<String, Comp>,
    pub global_off: BTreeMap<String, Comp>,
    pub uids: BTreeMap<u32, UidUsage>,
}

/// «u0a429» → 10429, «u10a439» → 1010439, «1000» → 1000, «u10s1000» → 1001000, «u0i5» → 99005.
pub fn parse_uid_label(s: &str) -> Option<u32> {
    if let Ok(v) = s.parse::<u32>() {
        return Some(v);
    }
    let s = s.strip_prefix('u')?;
    let upos = s.find(|c: char| !c.is_ascii_digit())?;
    let user: u32 = s[..upos].parse().ok()?;
    let rest = &s[upos..];
    let base = user * 100_000;
    if let Some(a) = rest.strip_prefix("ai") {
        return Some(base + 90_000 + a.parse::<u32>().ok()?);
    }
    if let Some(a) = rest.strip_prefix('a') {
        return Some(base + 10_000 + a.parse::<u32>().ok()?);
    }
    if let Some(a) = rest.strip_prefix('i') {
        return Some(base + 99_000 + a.parse::<u32>().ok()?);
    }
    if let Some(a) = rest.strip_prefix('s') {
        return Some(base + a.parse::<u32>().ok()?);
    }
    None
}

/// «screen=6.03 cpu=113 wakelock=0.00623 (737ms) …» → компоненты.
fn parse_comps(s: &str) -> BTreeMap<String, Comp> {
    let mut out: BTreeMap<String, Comp> = BTreeMap::new();
    let mut last: Option<String> = None;
    let mut in_dur = false;
    let mut dur = String::new();
    for tok in s.split_whitespace() {
        if in_dur || tok.starts_with('(') {
            in_dur = true;
            dur.push_str(tok.trim_start_matches('('));
            dur.push(' ');
            if tok.ends_with(')') {
                in_dur = false;
                if let Some(k) = &last {
                    if let Some(c) = out.get_mut(k) {
                        c.ms = parse_dumpsys_dur(dur.trim_end_matches([')', ' ']));
                    }
                }
                dur.clear();
            }
            continue;
        }
        if let Some((k, v)) = tok.split_once('=') {
            // «cpu:bg» и т.п. дублируют состояния из заголовка UID — не храним
            if k.contains(':') {
                last = None;
                continue;
            }
            if let Ok(mah) = v.parse::<f64>() {
                out.insert(k.to_string(), Comp { mah, ms: 0 });
                last = Some(k.to_string());
            }
        }
    }
    out
}

/// « fg: 44.6 (19m 15s 605ms) bg: 0.362 (294ms) cached: 7.94 (35m 4s 201ms)»
fn parse_states(s: &str) -> BTreeMap<String, Comp> {
    let mut out = BTreeMap::new();
    let toks: Vec<&str> = s.split_whitespace().collect();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        if let Some(name) = t.strip_suffix(':') {
            if let Some(v) = toks.get(i + 1).and_then(|x| x.parse::<f64>().ok()) {
                let mut ms = 0;
                let mut j = i + 2;
                if toks.get(j).map(|x| x.starts_with('(')).unwrap_or(false) {
                    let mut d = String::new();
                    while j < toks.len() {
                        d.push_str(toks[j].trim_start_matches('('));
                        d.push(' ');
                        if toks[j].ends_with(')') {
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    ms = parse_dumpsys_dur(d.trim_end_matches([')', ' ']));
                }
                out.insert(name.to_string(), Comp { mah: v, ms });
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn parse_global_line(line: &str) -> Option<(String, Comp)> {
    // «cpu: 370 apps: 370 duration: 53m 8s 502ms»
    let (name, rest) = line.split_once(':')?;
    if name.contains(' ') || name.starts_with('(') {
        return None;
    }
    let mut it = rest.split_whitespace();
    let mah: f64 = it.next()?.parse().ok()?;
    let ms = rest.split_once("duration:").map(|(_, d)| parse_dumpsys_dur(d.trim())).unwrap_or(0);
    Some((name.to_string(), Comp { mah, ms }))
}

pub fn parse(text: &str) -> UsageSnap {
    let mut s = UsageSnap::default();
    #[derive(PartialEq)]
    enum Sec {
        None,
        Global,
        GlobalOn,
        GlobalOff,
    }
    let mut sec = Sec::None;
    let mut cur_uid: Option<u32> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Capacity:") {
            for part in rest.split(',') {
                let part = part.trim();
                if let Some(v) = part.strip_prefix("Computed drain:") {
                    s.computed = v.trim().parse().unwrap_or(0.0);
                } else if let Some(v) = part.strip_prefix("actual drain:") {
                    let v = v.trim();
                    let (lo, hi) = v.split_once('-').unwrap_or((v, v));
                    s.actual_lo = lo.parse().unwrap_or(0.0);
                    s.actual_hi = hi.parse().unwrap_or(0.0);
                } else if let Ok(c) = part.parse::<f64>() {
                    s.capacity = c;
                }
            }
            continue;
        }
        if line == "Global" {
            sec = Sec::Global;
            cur_uid = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("UID ") {
            sec = Sec::None;
            let Some((label, tail)) = rest.split_once(':') else { continue };
            let Some(uid) = parse_uid_label(label.trim()) else { continue };
            let tail = tail.trim();
            let (tot, states) = match tail.split_once(' ') {
                Some((t, r)) => (t, r),
                None => (tail, ""),
            };
            let e = s.uids.entry(uid).or_default();
            e.total += tot.parse::<f64>().unwrap_or(0.0);
            for (k, v) in parse_states(states) {
                let x = e.states.entry(k).or_default();
                x.mah += v.mah;
                x.ms += v.ms;
            }
            cur_uid = Some(uid);
            continue;
        }
        if sec != Sec::None && cur_uid.is_none() {
            if line.starts_with("(on battery, screen on)") {
                sec = Sec::GlobalOn;
                continue;
            }
            if line.starts_with("(on battery, screen off") {
                sec = Sec::GlobalOff;
                continue;
            }
            if let Some((k, c)) = parse_global_line(line) {
                match sec {
                    Sec::Global => s.global.insert(k, c),
                    Sec::GlobalOn => s.global_on.insert(k, c),
                    Sec::GlobalOff => s.global_off.insert(k, c),
                    Sec::None => None,
                };
            }
            continue;
        }
        if let Some(uid) = cur_uid {
            let e = s.uids.get_mut(&uid).unwrap();
            if let Some(r) = line.strip_prefix("(on battery, screen on)") {
                merge(&mut e.screen_on, parse_comps(r));
            } else if line.starts_with("(on battery, screen off") {
                let r = line.split_once(')').map(|x| x.1).unwrap_or("");
                merge(&mut e.screen_off, parse_comps(r));
            } else if line.contains('=') && !line.starts_with('(') {
                merge(&mut e.comps, parse_comps(line));
            } else if !line.starts_with('(') {
                cur_uid = None;
            }
        }
    }
    s
}

fn merge(dst: &mut BTreeMap<String, Comp>, src: BTreeMap<String, Comp>) {
    for (k, v) in src {
        let x = dst.entry(k).or_default();
        x.mah += v.mah;
        x.ms += v.ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const T: &str = "    Estimated power use (mAh):
      Capacity: 4673, Computed drain: 464, actual drain: 460-470
    Global
      screen: 64.7 apps: 64.7
      cpu: 370 apps: 370 duration: 53m 8s 502ms
      (on battery, screen on)
        cpu: 340 apps: 340 duration: 46m 43s 756ms
      (on battery, screen off/doze)
        cpu: 29.9 apps: 29.8 duration: 6m 24s 746ms
    UID u0a429: 106 fg: 44.6 (19m 15s 605ms) bg: 0.362 (294ms) cached: 7.94 (35m 4s 201ms)
      screen=52.6 cpu=19.5 cpu:fg=12.0 audio=11.8 (15m 12s 979ms) audio:fg=11.7 (15m 8s 913ms)
      (on battery, screen on) screen=52.6 cpu=19.5
      (on battery, screen off/doze) cpu=0.0545 cpu:cached=0.0545
    UID 1000: 122 bg: 116
      screen=6.03 cpu=113 wakelock=0.00623 (737ms)
";
    #[test]
    fn usage() {
        let s = parse(T);
        assert_eq!(s.capacity, 4673.0);
        assert_eq!(s.actual_hi, 470.0);
        assert_eq!(s.global["cpu"].ms, 3_188_502);
        assert_eq!(s.global_off["cpu"].mah, 29.9);
        let u = &s.uids[&10429];
        assert_eq!(u.total, 106.0);
        assert_eq!(u.states["fg"].ms, 1_155_605);
        assert_eq!(u.comps["audio"].ms, 912_979);
        assert_eq!(u.screen_off["cpu"].mah, 0.0545);
        assert_eq!(s.uids[&1000].comps["wakelock"].ms, 737);
        assert_eq!(parse_uid_label("u10a439"), Some(1_010_439));
        assert_eq!(parse_uid_label("u10s1000"), Some(1_001_000));
    }
}
