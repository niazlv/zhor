//! Часы, чтение файлов, мелочи.

use std::fs::File;
use std::io::Read;
use std::path::Path;

fn clock_ns(clk: libc::clockid_t) -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe {
        libc::clock_gettime(clk, &mut ts);
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Процессорное время текущего потока, мкс (сколько реально стоил кусок работы).
pub fn thread_cpu_us() -> u64 {
    clock_ns(libc::CLOCK_THREAD_CPUTIME_ID) / 1000
}

/// Время с загрузки, включая сон (CLOCK_BOOTTIME), мс.
pub fn boot_ms() -> u64 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        clock_ns(libc::CLOCK_BOOTTIME) / 1_000_000
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        clock_ns(libc::CLOCK_MONOTONIC) / 1_000_000
    }
}

/// Время с загрузки без сна (CLOCK_MONOTONIC), мс.
pub fn mono_ms() -> u64 {
    clock_ns(libc::CLOCK_MONOTONIC) / 1_000_000
}

/// Настенное время, мс с эпохи.
pub fn real_ms() -> i64 {
    (clock_ns(libc::CLOCK_REALTIME) / 1_000_000) as i64
}

/// Читает файл в переиспользуемый буфер. Возвращает false, если файла нет.
/// Напрямую через open/read/close: у std `read_to_end` на каждый файл ещё statx и lseek,
/// а сканер читает тысячи мелких файлов /proc за шаг.
pub fn read_into(path: &str, buf: &mut Vec<u8>) -> bool {
    buf.clear();
    let mut p = [0u8; 256];
    let b = path.as_bytes();
    if b.len() >= p.len() {
        return match File::open(path) {
            Ok(mut f) => f.read_to_end(buf).is_ok(),
            Err(_) => false,
        };
    }
    p[..b.len()].copy_from_slice(b);
    unsafe {
        let fd = libc::open(p.as_ptr() as *const libc::c_char, libc::O_RDONLY | libc::O_CLOEXEC);
        if fd < 0 {
            return false;
        }
        if buf.capacity() < 4096 {
            buf.reserve(4096);
        }
        loop {
            let spare = buf.capacity() - buf.len();
            if spare < 512 {
                buf.reserve(4096);
                continue;
            }
            let n = libc::read(fd, buf.as_mut_ptr().add(buf.len()) as *mut libc::c_void, spare);
            if n <= 0 {
                break;
            }
            buf.set_len(buf.len() + n as usize);
        }
        libc::close(fd);
    }
    true
}

pub fn read_str(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

pub fn read_i64(path: impl AsRef<Path>) -> Option<i64> {
    read_str(path)?.parse().ok()
}

pub fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_str(path)?.parse().ok()
}

/// Разбор беззнакового десятичного числа из байтов (без аллокаций).
pub fn parse_u64(b: &[u8]) -> u64 {
    let mut v: u64 = 0;
    for &c in b {
        if c.is_ascii_digit() {
            v = v.wrapping_mul(10).wrapping_add((c - b'0') as u64);
        } else {
            break;
        }
    }
    v
}

/// Заменяет серии цифр на «#»: binder:1234_5 → binder:#_#, kworker/u16:3 → kworker/u#:#.
pub fn normalize_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_digits = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            out.push(ch);
        }
    }
    out
}

pub fn is_zero(v: &u64) -> bool {
    *v == 0
}
pub fn is_zero_f(v: &f64) -> bool {
    *v == 0.0
}

/// Для `skip_serializing_if`: пустая коллекция.
pub trait Len {
    fn is_empty_c(&self) -> bool;
}
impl<K, V> Len for std::collections::BTreeMap<K, V> {
    fn is_empty_c(&self) -> bool {
        self.is_empty()
    }
}
impl<T> Len for Vec<T> {
    fn is_empty_c(&self) -> bool {
        self.is_empty()
    }
}
pub fn is_empty_coll<T: Len>(v: &T) -> bool {
    v.is_empty_c()
}

/// мА·с → мА·ч
pub fn mas_to_mah(mas: f64) -> f64 {
    mas / 3600.0
}

/// Человеческая длительность: «1 ч 05 мин», «3 мин 12 с», «850 мс».
pub fn fmt_dur(ms: u64) -> String {
    if ms < 1000 {
        return format!("{ms} мс");
    }
    let s = ms / 1000;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h} ч {m:02} мин")
    } else if m > 0 {
        format!("{m} мин {sec:02} с")
    } else {
        format!("{}.{} с", sec, (ms % 1000) / 100)
    }
}

/// Запуск внешней команды с ограничением по времени; stdout как строка.
pub fn run_cmd(prog: &str, args: &[&str], timeout_ms: u64) -> Option<String> {
    run_cmd_bytes(prog, args, timeout_ms).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// То же, но сырые байты (для двоичного вывода, например `dumpsys batterystats --proto`).
pub fn run_cmd_bytes(prog: &str, args: &[&str], timeout_ms: u64) -> Option<Vec<u8>> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = out.read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > Duration::from_millis(timeout_ms) {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
    reader.join().ok()
}

/// Разбор длительности из dumpsys: «1h 2m 3s 4ms», «15m 32s 444ms», «+1d0h0m0s0ms», «737ms».
pub fn parse_dumpsys_dur(s: &str) -> u64 {
    let mut total: u64 = 0;
    let mut num: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            num = num * 10 + (c - b'0') as u64;
        } else if c == b'd' {
            total += num * 86_400_000;
            num = 0;
        } else if c == b'h' {
            total += num * 3_600_000;
            num = 0;
        } else if c == b'm' {
            if i + 1 < b.len() && b[i + 1] == b's' {
                total += num;
                i += 1;
            } else {
                total += num * 60_000;
            }
            num = 0;
        } else if c == b's' {
            total += num * 1000;
            num = 0;
        }
        i += 1;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn norm() {
        assert_eq!(normalize_name("binder:1234_5"), "binder:#_#");
        assert_eq!(normalize_name("kworker/u19:2-hal_wq"), "kworker/u#:#-hal_wq");
        assert_eq!(normalize_name("RenderThread"), "RenderThread");
    }
    #[test]
    fn dur() {
        assert_eq!(parse_dumpsys_dur("15m 32s 444ms"), 932_444);
        assert_eq!(parse_dumpsys_dur("737ms"), 737);
        assert_eq!(parse_dumpsys_dur("1h 2m"), 3_720_000);
        assert_eq!(parse_dumpsys_dur("+1d0h0m0s0ms"), 86_400_000);
    }
}
