//! (0.8.1) Системные свойства Android без запуска `getprop`.
//!
//! Свойства лежат в файлах /dev/__properties__/<контекст SELinux> (их отображает в память init,
//! он же меняет значения на месте). Каждое свойство — запись `prop_info`:
//! `u32 serial` + `char value[92]` + полное имя с нулём. Запись не переезжает, поэтому её смещение
//! находим один раз, а потом читаем 96 байт — микросекунды против ~5 мс на `getprop`.
//!
//! serial: младший бит — значение сейчас переписывается (тогда читаем ещё раз), бит 16 — длинное
//! свойство (значение лежит в другом месте, нам такие не нужны), старший байт — длина значения.

use std::fs::File;
use std::os::unix::fs::FileExt;

const DIR: &str = "/dev/__properties__";
const VALUE_MAX: usize = 92;
/// serial + value
const INFO_HEAD: usize = 4 + VALUE_MAX;
/// Заголовок области: bytes_used, serial, magic, version, reserved[28].
const AREA_HEAD: usize = 128;
const LONG_FLAG: u32 = 1 << 16;

pub struct Prop {
    file: File,
    off: u64,
}

impl Prop {
    /// Найти свойство. Сначала смотрим области, в имени которых есть первое слово свойства
    /// (`debug.*` → `u:object_r:debug_prop:s0`); `all` — если там нет, то и все остальные (их сотни,
    /// по 128 КБ, поэтому полный обход — только один раз).
    pub fn find(name: &str, all: bool) -> Option<Prop> {
        let first = name.split('.').next().unwrap_or(name);
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(DIR)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| p.file_name().map(|n| n != "property_info").unwrap_or(false))
            .collect();
        let near = |p: &std::path::PathBuf| p.to_string_lossy().contains(first);
        files.sort_by_key(|p| !near(p));
        for p in files {
            if !all && !near(&p) {
                break;
            }
            let Ok(data) = std::fs::read(&p) else { continue };
            if let Some(off) = locate(&data, name) {
                if let Ok(file) = File::open(&p) {
                    return Some(Prop { file, off });
                }
            }
        }
        None
    }

    /// Текущее значение. None — не прочиталось (или три раза подряд попали на запись).
    pub fn get(&self) -> Option<String> {
        let mut b = [0u8; INFO_HEAD];
        for _ in 0..3 {
            self.file.read_exact_at(&mut b, self.off).ok()?;
            if let Some(v) = decode(&b) {
                return Some(v);
            }
        }
        None
    }
}

/// Смещение записи `prop_info` с именем `name` в образе области.
fn locate(data: &[u8], name: &str) -> Option<u64> {
    let mut needle = name.as_bytes().to_vec();
    needle.push(0);
    let mut from = 0;
    while from + needle.len() <= data.len() {
        let p = data[from..].windows(needle.len()).position(|w| w == needle.as_slice())?;
        let pos = from + p;
        // перед именем — последний байт value (ноль); внутри чужого, более длинного имени
        // («persist.debug.…») там была бы буква или точка
        if pos >= AREA_HEAD + INFO_HEAD && (pos - INFO_HEAD) % 4 == 0 && data[pos - 1] == 0 {
            let start = pos - INFO_HEAD;
            if decode(&data[start..pos]).is_some() {
                return Some(start as u64);
            }
        }
        from = pos + 1;
    }
    None
}

fn decode(b: &[u8]) -> Option<String> {
    let serial = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    if serial & 1 != 0 || serial & LONG_FLAG != 0 {
        return None;
    }
    let len = (serial >> 24) as usize;
    if len >= VALUE_MAX {
        return None;
    }
    let v = &b[4..4 + len];
    if b[4 + len] != 0 || v.contains(&0) {
        return None;
    }
    std::str::from_utf8(v).ok().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Область: заголовок, узел дерева с куском имени (как у настоящего prop_bt) и две записи.
    fn area(entries: &[(&str, &str, bool)]) -> Vec<u8> {
        let mut d = vec![0u8; AREA_HEAD];
        d.extend_from_slice(&[0u8; 20]);
        d.extend_from_slice(b"screen_state\0\0\0\0");
        for (name, value, dirty) in entries {
            while d.len() % 4 != 0 {
                d.push(0);
            }
            let serial = ((value.len() as u32) << 24) | (7 << 1) | (*dirty as u32);
            d.extend_from_slice(&serial.to_le_bytes());
            let mut v = value.as_bytes().to_vec();
            v.resize(VALUE_MAX, 0);
            d.extend_from_slice(&v);
            d.extend_from_slice(name.as_bytes());
            d.push(0);
        }
        d
    }

    #[test]
    fn finds_and_reads() {
        let d = area(&[("debug.tracing.screen_brightness", "0.0016", false), ("debug.tracing.screen_state", "4", false)]);
        let off = locate(&d, "debug.tracing.screen_state").unwrap() as usize;
        assert_eq!(decode(&d[off..off + INFO_HEAD]).as_deref(), Some("4"));
        let off = locate(&d, "debug.tracing.screen_brightness").unwrap() as usize;
        assert_eq!(decode(&d[off..off + INFO_HEAD]).as_deref(), Some("0.0016"));
        assert!(locate(&d, "screen_state").is_none());
        assert!(locate(&d, "tracing.screen_state").is_none());
        assert!(locate(&d, "debug.tracing.screen").is_none());
    }

    #[test]
    fn skips_dirty_and_long() {
        let d = area(&[("a.b", "2", true)]);
        assert!(locate(&d, "a.b").is_none());
        let mut b = [0u8; INFO_HEAD];
        b[..4].copy_from_slice(&((1u32 << 24) | LONG_FLAG).to_le_bytes());
        b[4] = b'x';
        assert!(decode(&b).is_none());
    }
}
