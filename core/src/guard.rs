//! Приложение удалили — убрать за собой то, что Жор менял в системе.
//!
//! Android не сообщает приложению о его собственном удалении, а ядро — отдельный root-процесс и
//! переживает удаление APK. Поэтому следит само: если приложение было установлено, а потом пропало
//! дольше чем на `GRACE_MS` (переустановка с другой подписью укладывается в секунды), ядро возвращает
//! ограничения фона, выключает подробную историю Android, убирает свой автозапуск и останавливается.
//! Записи в каталоге данных остаются: удалять их без спроса — необратимо.

use std::path::{Path, PathBuf};

/// Каталог приложения в device-encrypted хранилище: есть, пока оно установлено (и до разблокировки).
pub const APP_DIR: &str = "/data/user_de/0/dev.niazlv.zhor";
/// Скрипт автозапуска, который ставит приложение (Core.kt).
pub const BOOT_SCRIPT: &str = "/data/adb/service.d/zhor.sh";
/// Сколько приложение должно отсутствовать, чтобы считать его удалённым.
const GRACE_MS: i64 = 10 * 60_000;
/// Как часто проверять.
const CHECK_MS: i64 = 60_000;

pub struct Guard {
    marker: PathBuf,
    missing_since: Option<i64>,
    last_check: i64,
    grace_ms: i64,
    /// попыток уборки: если что-то не вернулось, ядро не останавливается и пробует снова
    attempts: u32,
}

/// Сколько раз пробовать вернуть ограничения, прежде чем остановиться всё равно.
const MAX_ATTEMPTS: u32 = 10;

/// Шаг решения: (с какого момента приложения нет, пора убирать).
pub fn step(seen: bool, installed: bool, missing_since: Option<i64>, now: i64, grace: i64) -> (Option<i64>, bool) {
    if installed || !seen {
        return (None, false);
    }
    let since = missing_since.unwrap_or(now);
    (Some(since), now - since >= grace)
}

impl Guard {
    pub fn new(dir: &Path) -> Self {
        // ZHOR_UNINSTALL_GRACE_MS — только для проверки, чтобы не ждать 10 минут
        let grace_ms = std::env::var("ZHOR_UNINSTALL_GRACE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(GRACE_MS);
        Guard { marker: dir.join("app_seen"), missing_since: None, last_check: 0, grace_ms, attempts: 0 }
    }

    /// Раз в минуту: true — приложение удалено, пора убирать за собой и останавливаться.
    pub fn check(&mut self, now: i64) -> bool {
        if now - self.last_check < CHECK_MS {
            return false;
        }
        self.last_check = now;
        let installed = Path::new(APP_DIR).exists();
        if installed && !self.marker.exists() {
            let _ = std::fs::write(&self.marker, now.to_string());
        }
        let (since, go) = step(self.marker.exists(), installed, self.missing_since, now, self.grace_ms);
        self.missing_since = since;
        go
    }

    /// Убрать за собой. → (что сделано — для журнала ядра, можно останавливаться). Не всё вернулось —
    /// автозапуск и история не трогаются, ядро пробует снова через минуту (до MAX_ATTEMPTS раз).
    pub fn cleanup(&mut self, dir: &Path, full_history: bool) -> (Vec<String>, bool) {
        let mut out = Vec::new();
        self.attempts += 1;
        let r = crate::compare::revert_all(dir);
        let names = |k: &str| -> Vec<String> {
            r[k].as_array().map(|a| a.iter().map(|x| format!("{}{}", x["pkg"].as_str().unwrap_or("?"), x["mode"].as_str().map(|m| format!(" ({m})")).unwrap_or_default())).collect()).unwrap_or_default()
        };
        let (n, f, sk) = (names("reverted"), names("failed"), names("skipped"));
        out.push(format!("ограничения фона возвращены: {}{}", n.len(), if n.is_empty() { String::new() } else { format!(" — {}", n.join(", ")) }));
        if !f.is_empty() {
            out.push(format!("не вышло вернуть: {}", f.join(", ")));
            if self.attempts < MAX_ATTEMPTS {
                out.push(format!("попытка {} из {MAX_ATTEMPTS}, повторю через минуту", self.attempts));
                return (out, false);
            }
            out.push("попытки кончились: снимите ограничение в настройках батареи приложения или `zhord revert-all`".into());
        }
        if !sk.is_empty() {
            out.push(format!("уже не наше (режим поменяли): {}", sk.join(", ")));
        }
        if full_history {
            crate::history::set_android_full_history(false);
            out.push("подробная история Android выключена".into());
        }
        // автозапуск — только свой скрипт (его пишет приложение и в нём есть zhord)
        if std::fs::read_to_string(BOOT_SCRIPT).map(|s| s.contains("zhord")).unwrap_or(false) && std::fs::remove_file(BOOT_SCRIPT).is_ok() {
            out.push(format!("автозапуск убран ({BOOT_SCRIPT})"));
        }
        let _ = std::fs::remove_file(&self.marker);
        out.push(format!("записи оставлены в {}", dir.display()));
        (out, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_grace_and_ignores_never_seen() {
        let g = 600_000;
        // приложения не было никогда (ядро без приложения, CLI) — ничего не делать
        assert_eq!(step(false, false, None, 1000, g), (None, false));
        // установлено — сброс
        assert_eq!(step(true, true, Some(5), 1000, g), (None, false));
        // пропало: отсчёт пошёл, но рано
        assert_eq!(step(true, false, None, 1000, g), (Some(1000), false));
        assert_eq!(step(true, false, Some(1000), 300_000, g), (Some(1000), false));
        // переустановка за пару минут — снова установлено, отсчёт сброшен
        assert_eq!(step(true, true, Some(1000), 120_000, g), (None, false));
        // нет дольше 10 минут — пора
        assert_eq!(step(true, false, Some(1000), 601_000, g), (Some(1000), true));
    }
}
