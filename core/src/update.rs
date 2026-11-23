//! Обновление приложения: раз в сутки напомнить приложению проверить новую версию и поставить
//! скачанный им APK.
//!
//! В сеть ядро не ходит: проверяет, скачивает и сверяет подпись приложение. Ядро только будит его
//! рассылкой (как тревоги сторожа) и по просьбе ставит APK через `pm install` — после того как
//! владелец нажал «Обновить». Ядро живёт отдельно от процессов приложения и переживает его обновление.

use crate::util;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Как часто напоминать о проверке.
const EVERY_MS: i64 = 24 * 3_600_000;
const CHECK_ACTION: &str = "dev.niazlv.zhor.CHECK_UPDATE";
/// Куда ядро копирует APK перед установкой: файлы приложения system_server читать не может.
const STAGE: &str = "/data/local/tmp/zhor-update.apk";

/// Ежедневное напоминание приложению. Время последнего — в файле, чтобы перезапуск ядра не будил приложение зря.
pub struct Ping {
    file: PathBuf,
    last: i64,
    last_check: i64,
}

impl Ping {
    pub fn new(dir: &Path) -> Self {
        let file = dir.join("update_ping");
        let last = std::fs::read_to_string(&file).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        Ping { file, last, last_check: 0 }
    }

    /// Пора напомнить: проверка включена, приложение установлено и с прошлого раза прошли сутки.
    /// Первое напоминание — через сутки после запуска ядра (при открытии приложение проверяет само).
    pub fn due(&mut self, now: i64, enabled: bool) -> bool {
        if !enabled || now - self.last_check < 60_000 {
            return false;
        }
        self.last_check = now;
        if self.last == 0 {
            self.mark(now);
            return false;
        }
        now - self.last >= EVERY_MS && Path::new(crate::guard::APP_DIR).exists()
    }

    pub fn mark(&mut self, now: i64) {
        self.last = now;
        let _ = std::fs::write(&self.file, now.to_string());
    }
}

/// Разбудить приложение: пусть проверит, нет ли новой версии. → доставлено
pub fn ping_app() -> bool {
    let receiver = crate::watch::ALERT_RECEIVER;
    util::run_cmd("cmd", &["activity", "broadcast", "--user", "0", "--include-stopped-packages", "-a", CHECK_ACTION, "-n", receiver], 15_000)
        .map(|o| o.contains("Broadcast completed") || o.contains("Broadcast sent"))
        .unwrap_or(false)
}

/// APK — только из папки приложения: его скачало и проверило оно само.
fn path_ok(p: &str) -> bool {
    let app = p.starts_with("/data/user/0/dev.niazlv.zhor/") || p.starts_with("/data/data/dev.niazlv.zhor/");
    app && p.ends_with(".apk") && !p.contains("..") && Path::new(p).is_file()
}

/// Поставить APK, который скачало и проверило приложение, и открыть его снова.
/// Приложение при установке закроется — ответа оно может уже не получить; итог пишется в журнал ядра.
pub fn install_apk(path: &str) -> Value {
    if !path_ok(path) {
        return json!({"error": format!("путь не разрешён: {path}")});
    }
    if let Err(e) = std::fs::copy(path, STAGE) {
        return json!({"error": format!("не скопировался во временную папку: {e}")});
    }
    let _ = std::fs::set_permissions(STAGE, std::os::unix::fs::PermissionsExt::from_mode(0o644));
    let out = util::run_cmd("pm", &["install", "-r", STAGE], 180_000).unwrap_or_default();
    let _ = std::fs::remove_file(STAGE);
    let ok = out.contains("Success");
    if ok {
        // открыть новую версию: она сама поставит своё ядро, если оно новее
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let _ = util::run_cmd("cmd", &["activity", "start-activity", "-n", "dev.niazlv.zhor/.MainActivity"], 15_000);
        });
    }
    json!({"ok": ok, "out": out.trim()})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_waits_a_day_and_survives_restart() {
        let dir = std::env::temp_dir().join(format!("zhor-ping-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::remove_file(dir.join("update_ping"));
        let mut p = Ping::new(&dir);
        // выключено — никогда
        assert!(!p.due(1_000_000, false));
        // первый раз только отмечает время
        assert!(!p.due(2_000_000, true));
        // перезапуск ядра: время прочитано с диска, сутки ещё не прошли
        let mut p2 = Ping::new(&dir);
        assert!(!p2.due(2_000_000 + 3_600_000, true));
        assert!(!path_ok("/data/local/tmp/x.apk"));
        assert!(!path_ok("/data/user/0/dev.niazlv.zhor/../x.apk"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
