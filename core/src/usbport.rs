//! (0.8.3) Мигание порта USB-C.
//!
//! Порт по нескольку раз в секунду включает режим USB-хоста и тут же выключает, хотя так не ведёт
//! себя ни зарядка, ни подключённое устройство: влага или грязь в разъёме, повреждённый кабель или
//! переходник. Каждое включение подаёт питание на порт (OTG), телефон не засыпает, а на некоторых
//! телефонах контроллер USB в какой-то момент ловит сбой защиты памяти (SMMU) — паника ядра и
//! перезагрузка через несколько минут после начала мигания.
//!
//! Источник — строки ядра «cur_role:NONE new_role:HOST refcnt:0» в /dev/kmsg (так пишет
//! переключатель роли USB на Qualcomm). Нет таких строк — правило просто молчит.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Read;

/// окно, в котором считаем включения режима хоста, мкс
pub const WINDOW_US: u64 = 3 * 60 * 1_000_000;
/// столько включений за окно — уже мигание (живое устройство даёт одно на подключение)
pub const MIN_FLIPS: usize = 30;
/// столько без включений — мигание кончилось; следующее даст новую тревогу
pub const QUIET_US: u64 = 15 * 60 * 1_000_000;
/// за один проход читаем не больше стольких записей ядра
const MAX_RECORDS: usize = 200_000;
const HOST_ON: &[u8] = b"new_role:HOST";

/// Запись /dev/kmsg: «prio,seq,usec,flags[,…];сообщение» → (seq, usec, сообщение).
pub fn parse_record(rec: &str) -> Option<(u64, u64, &str)> {
    let (head, msg) = rec.split_once(';')?;
    let mut f = head.split(',');
    let _prio = f.next()?;
    let seq = f.next()?.parse().ok()?;
    let us = f.next()?.parse().ok()?;
    Some((seq, us, msg.lines().next().unwrap_or("")))
}

/// «cur_role:NONE new_role:HOST refcnt:0» → включение режима хоста.
pub fn is_host_on(msg: &str) -> bool {
    let role = |key: &str| msg.split_once(key).and_then(|(_, r)| r.split_whitespace().next());
    role("cur_role:") == Some("NONE") && role("new_role:") == Some("HOST")
}

/// Мигание, которое пора показать.
#[derive(Debug, Clone, PartialEq)]
pub struct Flap {
    /// включений за окно
    pub n: usize,
    /// от первого до последнего из них, мкс
    pub span_us: u64,
    /// первое включение этого мигания: seq записи ядра и её время с загрузки, мкс
    pub first_seq: u64,
    pub first_us: u64,
}

/// Считает включения режима хоста; тревога — одна на мигание.
#[derive(Debug, Default)]
pub struct Detector {
    flips: VecDeque<u64>,
    /// начало текущего мигания (seq, мкс) и последнее включение в нём
    ep: Option<(u64, u64)>,
    ep_last_us: u64,
    shown: Option<u64>,
}

impl Detector {
    pub fn push(&mut self, seq: u64, us: u64) {
        if self.ep.is_none() || us.saturating_sub(self.ep_last_us) > QUIET_US {
            self.ep = Some((seq, us));
        }
        self.ep_last_us = self.ep_last_us.max(us);
        self.flips.push_back(us);
    }

    /// Мигание идёт и о нём ещё не говорили → Some.
    pub fn check(&mut self, now_us: u64) -> Option<Flap> {
        let from = now_us.saturating_sub(WINDOW_US);
        while self.flips.front().is_some_and(|&t| t < from) {
            self.flips.pop_front();
        }
        let (first_seq, first_us) = self.ep?;
        if self.flips.len() < MIN_FLIPS || self.shown == Some(first_seq) {
            return None;
        }
        self.shown = Some(first_seq);
        let span_us = self.flips.back().copied().unwrap_or(0).saturating_sub(self.flips.front().copied().unwrap_or(0));
        Some(Flap { n: self.flips.len(), span_us, first_seq, first_us })
    }
}

/// Читатель /dev/kmsg без блокировки. Первый проход отдаёт всё, что ещё лежит в буфере ядра с
/// загрузки: мигание сразу после перезагрузки (частый след такой паники) тоже попадёт.
pub struct UsbPort {
    f: Option<File>,
    buf: Vec<u8>,
    /// начало boot_id: тревога одна на мигание и после перезапуска демона в той же загрузке
    pub boot: String,
    pub det: Detector,
    /// включений с прошлой строки в журнал и когда она была (mono, мкс)
    pub pending: usize,
    pub logged_us: u64,
}

impl UsbPort {
    pub fn open() -> Self {
        use std::os::unix::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open("/dev/kmsg").ok();
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").map(|s| s.trim().chars().take(8).collect()).unwrap_or_default();
        UsbPort { f, buf: vec![0u8; 16 * 1024], boot, det: Detector::default(), pending: 0, logged_us: 0 }
    }

    /// Дочитать новые записи ядра. → сколько включений режима хоста нашли.
    pub fn poll(&mut self) -> usize {
        let Some(f) = self.f.as_mut() else { return 0 };
        let mut found = 0;
        for _ in 0..MAX_RECORDS {
            match f.read(&mut self.buf) {
                Ok(0) => break,
                Ok(n) => {
                    // разбираем только нужные: ядро на некоторых прошивках пишет сотни строк в минуту
                    if !self.buf[..n].windows(HOST_ON.len()).any(|w| w == HOST_ON) {
                        continue;
                    }
                    let rec = String::from_utf8_lossy(&self.buf[..n]);
                    if let Some((seq, us, msg)) = parse_record(&rec) {
                        if is_host_on(msg) {
                            self.det.push(seq, us);
                            found += 1;
                        }
                    }
                }
                // буфер ядра перетёр непрочитанные записи — читаем со следующей уцелевшей
                Err(e) if e.raw_os_error() == Some(libc::EPIPE) => continue,
                Err(_) => break,
            }
        }
        self.pending += found;
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn parses_kmsg_record_and_role() {
        let (seq, us, msg) = parse_record("3,48123,100454206,-;cur_role:NONE new_role:HOST refcnt:0\n").unwrap();
        assert_eq!((seq, us), (48123, 100_454_206));
        assert!(is_host_on(msg));
        assert!(!is_host_on("cur_role:HOST new_role:NONE refcnt:1"));
        assert!(!is_host_on("cur_role:HOST new_role:HOST refcnt:1"));
        assert!(!is_host_on("usb: new device"));
        assert!(parse_record("мусор").is_none());
    }

    #[test]
    fn flapping_alerts_once_per_episode() {
        let mut d = Detector::default();
        // 2.5 раза в секунду, как перед паникой: 40 включений за 16 с
        for i in 0..40u64 {
            d.push(1000 + i, 4 * S + i * 400_000);
        }
        let f = d.check(20 * S).expect("мигание");
        assert_eq!(f.n, 40);
        assert_eq!(f.first_seq, 1000);
        assert_eq!(f.span_us, 39 * 400_000);
        // продолжается — та же тревога не повторяется
        for i in 0..40u64 {
            d.push(2000 + i, 30 * S + i * 400_000);
        }
        assert!(d.check(50 * S).is_none());
        // 20 минут тишины, потом снова — новое мигание, новая тревога
        let t = 50 * S + 20 * 60 * S;
        for i in 0..35u64 {
            d.push(5000 + i, t + i * 300_000);
        }
        assert_eq!(d.check(t + 20 * S).map(|f| f.first_seq), Some(5000));
    }

    #[test]
    fn plugging_devices_is_not_flapping() {
        let mut d = Detector::default();
        // флешку вставляли раз в 10 с в течение трёх минут — 18 включений
        for i in 0..18u64 {
            d.push(i, i * 10 * S);
        }
        assert!(d.check(180 * S).is_none());
        // старые включения уходят из окна
        let mut d = Detector::default();
        for i in 0..40u64 {
            d.push(i, i * S);
        }
        assert!(d.check(40 * S + WINDOW_US).is_none());
    }
}
