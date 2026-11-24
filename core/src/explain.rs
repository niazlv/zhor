//! «Почему»: словарь известных процессов, потоков, wakelock, источников и причин
//! пробуждения с объяснением по-русски.

/// Системные UID (android_filesystem_config.h).
pub fn aid_name(uid: u32) -> Option<&'static str> {
    Some(match uid % 100_000 {
        0 => "root (ядро и нативные демоны)",
        1000 => "Android OS (system)",
        1001 => "Телефония (radio)",
        1002 => "Bluetooth",
        1003 => "Графика (graphics)",
        1004 => "input",
        1005 => "audio",
        1006 => "camera",
        1010 => "Wi-Fi",
        1012 => "install",
        1013 => "Медиасервер (media)",
        1017 => "keystore",
        1019 => "DRM",
        1021 => "GPS (gps)",
        1027 => "NFC",
        1036 => "logd",
        1037 => "shared_relro",
        1040 => "mediaex",
        1041 => "audioserver",
        1046 => "mediacodec",
        1047 => "cameraserver",
        1049 => "tombstoned",
        1051 => "network_stack",
        1053 => "webview_zygote",
        1062 => "LMKD",
        1066 => "statsd",
        1067 => "incidentd",
        1068 => "secure_element",
        1069 => "lmkd",
        1072 => "artd",
        1073 => "uwb",
        1076 => "prng_seeder",
        2000 => "shell (adb)",
        2001 => "cache",
        2002 => "diag",
        9999 => "nobody",
        _ => return None,
    })
}

pub fn uid_kind(uid: u32) -> &'static str {
    let a = uid % 100_000;
    if a == 0 {
        "kernel"
    } else if (99_000..100_000).contains(&a) || (90_000..99_000).contains(&a) {
        "isolated"
    } else if a >= 10_000 {
        "app"
    } else {
        "system"
    }
}

fn starts(s: &str, p: &[&str]) -> bool {
    p.iter().any(|x| s.starts_with(x))
}
fn has(s: &str, p: &[&str]) -> bool {
    p.iter().any(|x| s.contains(x))
}

/// Объяснение процесса (по имени).
pub fn process(name: &str) -> Option<&'static str> {
    let n = name;
    let l = n.to_ascii_lowercase();
    Some(match () {
        _ if n == "kernel" => "Потоки ядра Linux: драйверы, прерывания, отложенная работа (kworker), управление памятью. Смотрите потоки — по имени видно подсистему.",
        _ if n == "system_server" => "Системный сервер Android: все системные службы (уведомления, будильники, геолокация, сеть, батарея). Нагрузка обычно «по просьбе» приложений через binder.",
        _ if n == "surfaceflinger" => "Композитор экрана: собирает кадр из окон. Растёт от анимаций, видео, 120 Гц и постоянно перерисовывающихся приложений.",
        _ if has(&l, &["composer", "hwcomposer", "display.allocator"]) => "Драйвер дисплея (HWC): вывод кадров на панель.",
        _ if n == "android.system.suspend-service" || n.starts_with("android.system.suspend") => "Служба засыпания Android: принимает захват/освобождение каждого wakelock от всех процессов и пытается усыпить систему. Высокая нагрузка = очень частые wakelock или бесконечные сорванные попытки уснуть (см. «Сон и ядро»).",
        _ if n == "zhord" => "Этот профилировщик (Жор). Честно показывает свою цену.",
        _ if n == "kswapd0" || n.starts_with("kswapd") => "Освобождение памяти ядром: ОЗУ не хватает, страницы выгружаются в zram. Много — признак нехватки памяти.",
        _ if n == "lmkd" => "Low Memory Killer: убивает фоновые процессы при нехватке памяти.",
        _ if n == "logd" => "Системный журнал. Растёт, если кто-то пишет в лог очень много.",
        _ if has(&l, &["statsd", "traced", "perfetto", "heapprofd"]) => "Телеметрия и трассировка системы.",
        _ if starts(&l, &["dex2oat", "artd", "installd", "odrefresh"]) => "Компиляция приложений (после установки/обновления или в фоновом обслуживании ночью).",
        _ if has(&l, &["wpa_supplicant", "wificond", "cnss", "wlan"]) => "Wi-Fi: поиск сетей, роуминг, поддержание соединения.",
        _ if has(&l, &["rild", "qcrild", "ril-daemon", "radio", "modem", "qmi", "cnd", "imsdatadaemon", "ims_rtp", "imsd"]) => "Модем и сотовая связь (RIL/IMS).",
        _ if has(&l, &["gnss", "lowi", "location", "loc_launcher", "xtra", "izat"]) => "Геолокация и GPS.",
        _ if has(&l, &["sensors", "sscrpcd", "adsprpcd", "sensor"]) => "Датчики и сенсорный хаб.",
        _ if has(&l, &["audioserver", "audio", "pal_"]) => "Звук: воспроизведение, запись, обработка.",
        _ if has(&l, &["camera"]) => "Камера.",
        _ if has(&l, &["media.codec", "media.swcodec", "mediaserver", "media.extractor", "c2", "codec"]) => "Декодирование/кодирование видео и аудио.",
        _ if has(&l, &["netd", "mdnsd", "dnsmasq", "clatd", "tethering"]) => "Сеть: маршрутизация, DNS, фаервол.",
        _ if n == "adbd" => "ADB (отладка). Беспроводной ADB держит Wi-Fi активным.",
        _ if n == "init" || n == "ueventd" => "init: запуск служб, события устройств.",
        _ if has(&l, &["keystore", "gatekeeper", "qseecom", "tee", "keymaster", "keymint"]) => "Безопасность: ключи, TEE.",
        _ if has(&l, &["thermal"]) => "Термоконтроль.",
        _ if has(&l, &["health", "battery", "charger"]) => "Батарея и зарядка (HAL).",
        _ if n.starts_with("com.google.android.gms") => "Сервисы Google Play: геолокация, push (FCM), синхронизация, Find My Device, проверка безопасности.",
        _ if n.starts_with("com.android.vending") => "Google Play Маркет: обновления приложений, проверка лицензий.",
        _ if n.starts_with("com.google.android.googlequicksearchbox") => "Google / Ассистент: распознавание «Окей, Google», лента.",
        _ if n.starts_with("com.android.systemui") => "Системный интерфейс: шторка, строка состояния, экран блокировки, AOD.",
        _ if n.starts_with("com.miui.powerkeeper") => "MIUI/HyperOS контроль питания (сам тоже работает).",
        _ if n.starts_with("com.xiaomi.joyose") => "Xiaomi Joyose: игровые профили и телеметрия производительности.",
        _ if has(n, &["com.miui.analytics", "com.miui.daemon", "com.xiaomi.mi_connect_service", "com.miui.securitycenter"]) => "Службы Xiaomi (аналитика, связь между устройствами, безопасность).",
        _ if n.starts_with("WebView:") || n.contains("webview:sandboxed") => "Рендер веб-страниц WebView, запущенный этим приложением (реклама, встроенный браузер, мини-приложения). Работает в отдельной песочнице, но тратит за счёт приложения.",
        _ if n.contains("js_sandbox") => "Песочница JavaScript WebView, запущенная этим приложением.",
        _ if n.contains(":sandboxed") || n.contains("isolated") => "Изолированный процесс приложения (песочница: рендер страниц, медиа, плагины).",
        _ if n.starts_with("zygote") || n == "webview_zygote" => "Zygote: запускает процессы приложений.",
        _ => return None,
    })
}

/// Объяснение потока.
pub fn thread(name: &str) -> Option<&'static str> {
    let n = name;
    Some(match () {
        _ if n == "<main>" => "Главный поток: интерфейс и обработка событий приложения.",
        _ if n == "<завершённые потоки>" => "Потоки, которые завершились между замерами (короткие задачи). Время известно из процесса, энергия — по средней цене процесса.",
        _ if n == "<прочие потоки>" => "Сумма мелких потоков.",
        _ if n == "RenderThread" || n.starts_with("hwuiTask") => "Отрисовка интерфейса (GPU-команды). Растёт от анимаций и перерисовки.",
        _ if n == "HeapTaskDaemon" => "Сборщик мусора ART: приложение много аллоцирует.",
        _ if n.starts_with("Jit thread pool") => "JIT-компилятор ART.",
        _ if starts(n, &["binder:", "HwBinder:"]) => "Обработка IPC-вызовов от других процессов: кто-то просит этот процесс что-то сделать.",
        _ if starts(n, &["Signal Catcher", "ReferenceQueueD", "FinalizerDaemon", "FinalizerWatchd", "perfetto_hprof"]) => "Служебный поток ART.",
        _ if starts(n, &["OkHttp", "Okio", "Chrome_IOThread", "NetworkService", "Cronet", "grpc", "Network"]) => "Сеть: HTTP-запросы, сокеты.",
        _ if starts(n, &["CrRendererMain", "Compositor", "VizCompositor", "ThreadPoolForeg", "ThreadPoolServ", "CrGpuMain", "Chrome_"]) => "Движок Chromium/WebView: рендер веб-страниц.",
        _ if starts(n, &["DefaultDispatch", "DefaultDispatcher", "kotlinx"]) => "Фоновые корутины Kotlin.",
        _ if starts(n, &["WM.task", "androidx.work"]) => "WorkManager: фоновые задачи.",
        _ if starts(n, &["Firebase", "FirebaseMessagin", "GoogleApiHandle", "gms_scheduler"]) => "Google/Firebase: push, синхронизация.",
        _ if starts(n, &["queued-work-loo"]) => "Асинхронная запись настроек (SharedPreferences).",
        _ if starts(n, &["AudioTrack", "AudioRecord", "AudioOut", "AudioIn", "FastMixer", "TimeCheck"]) => "Звук.",
        _ if starts(n, &["MediaCodec", "CodecLooper", "ExoPlayer", "MediaPlayer", "C2"]) => "Декодирование медиа.",
        _ if starts(n, &["GLThread", "UnityMain", "UnityGfx", "Thread-#Gfx", "RHIThread", "GameThread"]) => "Игровой/3D движок.",
        _ if starts(n, &["kworker/"]) => "Отложенная работа ядра. После «-» — очередь (подсистема), для которой работает поток.",
        _ if starts(n, &["irq/"]) => "Обработчик прерывания устройства (имя после номера — драйвер).",
        _ if starts(n, &["ksoftirqd"]) => "Программные прерывания: пакеты сети, таймеры, блочный ввод-вывод, когда их слишком много.",
        _ if starts(n, &["rcu", "rcuop", "rcuog"]) => "RCU: синхронизация в ядре.",
        _ if starts(n, &["migration/", "cpuhp/", "watchdog"]) => "Планировщик/горячее подключение CPU.",
        _ if starts(n, &["kswapd", "kcompactd", "khugepaged"]) => "Управление памятью: выгрузка/уплотнение страниц.",
        _ if starts(n, &["kgsl", "adreno", "mali"]) => "Драйвер GPU.",
        _ if starts(n, &["crtc_commit", "crtc_event", "sde_", "disp_", "dsi_", "mdss", "panel"]) => "Драйвер дисплея: коммит кадров на панель.",
        _ if starts(n, &["f#fs_", "f2fs", "jbd2", "ext4", "kverityd", "dm_bufio", "loop#", "ufs", "mmc", "blk"]) => "Файловая система и накопитель.",
        _ if starts(n, &["msm_watchdog", "qcom", "glink", "smem", "remoteproc", "subsys", "sysmon"]) => "Связь с сопроцессорами Qualcomm (модем, ADSP, CDSP).",
        _ if starts(n, &["wlan", "cnss", "wpa", "dp_rx", "dp_tx", "wlan_"]) => "Wi-Fi драйвер (dp_rx — приём пакетов).",
        _ if n.contains("mhi") => "MHI: шина PCIe до модема — обмен данными с сотовым модемом.",
        _ if n.contains("ipa") || n.contains("rmnet") => "Мобильные данные: ускоритель пакетов IPA / интерфейс rmnet.",
        _ if starts(n, &["Thread-#", "pool-#-thread-#", "AsyncTask #", "Timer-#"]) => "Безымянный фоновый поток приложения.",
        _ => return None,
    })
}

/// Источник пробуждения ядра (wakeup source).
pub fn wakeup_source(name: &str) -> Option<&'static str> {
    let l = name.to_ascii_lowercase();
    Some(match () {
        _ if name == "PowerManagerService.WakeLocks" => "Сумма wakelock приложений и системы — детали в разделе «Wakelock приложений».",
        _ if name == "PowerManagerService.Display" || name == "PowerManager.SuspendLockout" => "Экран включён или переключается.",
        _ if name == "PowerManagerService.Broadcasts" => "Рассылка системных событий (broadcast).",
        _ if l.contains("qcom_rx_wakelock") => "Wi-Fi: пришёл пакет — драйвер Qualcomm будит процессор и держит его, пока пакет не дойдёт до приложения. Кто шлёт пакеты — «Кто будит по сети» на экране «Сон».",
        _ if l.contains("ipa_ws") || l.contains("ipa") => "Мобильные данные: аппаратный ускоритель пакетов IPA обрабатывает трафик.",
        _ if l.contains("wlan") || l.contains("cnss") || l.contains("wifi") => "Wi-Fi драйвер: пакеты, сканирование, роуминг.",
        _ if l.contains("timerfd") && l.contains("без пробуждения") => "Таймер без права будить: срабатывает, только когда телефон и так не спит, — сам не будит.",
        _ if l.contains("timerfd") && l.contains("смена часов") => "Таймер AlarmManager, который следит за сменой часов. Ядро отмечает его на каждом выходе из сна, поэтому большое «будил» здесь — эхо других пробуждений, а не их причина: ищите тех, кто будит, — сеть, модем, приложения.",
        _ if l.contains("timerfd") && l.contains("будильники") => "Будильники AlarmManager с правом будить: приложения и система заказали пробуждение — будят по-настоящему. Кто именно — «Будильники» в карточках приложений и «Что происходило» на экране отрезка.",
        _ if l.contains("timerfd") => "Таймер timerfd с правом будить: таймеры системных служб и демонов (AlarmManager, звук). После имени — чей он.",
        _ if l.contains("alarmtimer") || l.contains("rtc") => "Аппаратный будильник (RTC): срабатывают будильники AlarmManager.",
        _ if l.contains("eventpoll") => "epoll: ожидание событий (ввод, датчики, сокеты).",
        _ if l.contains("sensor") => "Датчики с пробуждением (шагомер, поднятие телефона, значительное движение).",
        _ if l.contains("radio") || l.contains("qcril") || l.contains("rmt_storage") || l.contains("modem") || l.contains("rild") => "Модем: обмен с сотовой сетью, хранение данных модема.",
        _ if l.contains("netlink") => "Сетевые события ядра (смена интерфейсов, маршрутов).",
        _ if l.contains("ucsi") || l.contains("usb") || l.contains("pmic") || l.contains("charger") || l.contains("battery") => "USB/зарядка: события контроллера питания.",
        _ if l.contains("userspace-abort") => "Засыпание прервано из пространства пользователя (кто-то взял wakelock в момент засыпания).",
        _ if l.starts_with("suspendcontrol") => "Служба засыпания Android: задержки после неудачных попыток уснуть.",
        _ if l.contains("bluetooth") || l.contains("bt_") => "Bluetooth.",
        _ if l.contains("uart") || l.contains("qup") => "Последовательный порт (UART) — обычно канал до Bluetooth-чипа: активен, пока идёт обмен с BT-устройствами.",
        _ if l.contains("nfc") => "NFC.",
        _ if l.contains("audio") || l.contains("adsp") => "Аудио-DSP.",
        _ if l.contains("gpio") || l.contains("pon") || l.contains("key") => "Кнопки / прерывания GPIO.",
        _ => return None,
    })
}

/// Причина пробуждения/срыва сна (/sys/kernel/wakeup_reasons, «wr» из batterystats).
pub fn wakeup_reason(reason: &str) -> Option<String> {
    let l = reason.to_ascii_lowercase();
    if let Some(rest) = reason.strip_prefix("Abort: Pending Wakeup Sources: ") {
        return Some(format!("Сон сорван: в момент засыпания был активен источник «{rest}»."));
    }
    if let Some(rest) = reason.strip_prefix("Abort: Last active Wakeup Source: ") {
        return Some(format!("Сон сорван источником «{rest}»."));
    }
    if reason.starts_with("Abort: Device") {
        return Some("Сон сорван: драйвер устройства отказался засыпать.".into());
    }
    let s: &str = match () {
        _ if l.contains("adsp") => "Аудио/сенсорный DSP (ADSP) разбудил процессор: датчики, «Окей, Google», звук.",
        _ if l.contains("cdsp") => "Вычислительный DSP (CDSP).",
        _ if l.contains("modem") || l.contains("mpss") || l.contains("ipa") || l.contains("qmi") => "Модем: входящие данные или сигнализация сети.",
        _ if l.contains("wlan") || l.contains("cnss") || l.contains("wifi") => "Wi-Fi: входящие пакеты.",
        _ if l.contains("rtc") || l.contains("alarm") => "Будильник (AlarmManager через RTC).",
        _ if l.contains("slpi") || l.contains("sensor") => "Сенсорный хаб.",
        _ if l.contains("gpio") || l.contains("pon") || l.contains("pwrkey") => "Кнопка питания / GPIO.",
        _ if l.contains("usb") || l.contains("pmic") || l.contains("glink") => "Питание/USB/зарядка.",
        _ if l.contains("bt") || l.contains("bluetooth") => "Bluetooth.",
        _ if l.contains("unknown") => "Неизвестная причина.",
        _ => return None,
    };
    Some(s.to_string())
}

/// Тег wakelock (userspace, PowerManager).
pub fn wakelock(tag: &str) -> Option<String> {
    if let Some(r) = tag.strip_prefix("*job*/").or_else(|| tag.strip_prefix("*job*r/")).or_else(|| tag.strip_prefix("*job*e/")) {
        return Some(format!("Фоновое задание JobScheduler: {r}"));
    }
    if let Some(r) = tag.strip_prefix("*sync*/") {
        return Some(format!("Синхронизация аккаунта: {r}"));
    }
    if let Some(r) = tag.strip_prefix("*alarm*") {
        return Some(format!("Доставка будильника AlarmManager{r}"));
    }
    if let Some(r) = tag.strip_prefix("*walarm*:") {
        return Some(format!("Будильник с пробуждением: {r}"));
    }
    if let Some(r) = tag.strip_prefix("NotificationManagerService:post:") {
        return Some(format!("Публикация уведомлений от {r}"));
    }
    let l = tag.to_ascii_lowercase();
    let s: &str = match () {
        _ if l.contains("*location*") || l.contains("nlp") || l.contains("gnss") || l.contains("location") || l.contains("geofenc") => "Геолокация.",
        _ if l.contains("gcm") || l.contains("fcm") || l.contains("firebase") => "Push-сообщения (FCM).",
        _ if l.contains("audiomix") || l.contains("audio") => "Воспроизведение звука.",
        _ if l.contains("vibrator") => "Вибрация.",
        _ if l.contains("telephony") || l.contains("rilj") || l.contains("radio") => "Обмен с модемом.",
        _ if l.contains("sync") => "Синхронизация.",
        _ if l.contains("wifi") => "Wi-Fi.",
        _ if l.contains("bluetooth") => "Bluetooth.",
        _ if l.contains("networkstats") => "Подсчёт сетевой статистики.",
        _ if l.contains("job") || l.contains("work") => "Фоновая задача.",
        _ => return None,
    };
    Some(s.to_string())
}

/// Ключи компонентов batterystats --usage → по-русски.
pub fn component(name: &str) -> &str {
    match name {
        "screen" => "экран",
        "cpu" => "процессор",
        "bluetooth" => "Bluetooth",
        "camera" => "камера",
        "audio" => "звук",
        "video" => "видео",
        "flashlight" => "фонарик",
        "mobile_radio" => "мобильная связь",
        "sensors" => "датчики",
        "gnss" => "GPS",
        "wifi" => "Wi-Fi",
        "wakelock" => "wakelock",
        "memory" => "память",
        "phone" => "звонки",
        "ambient_display" => "AOD",
        "idle" => "простой",
        "reattributed" => "переадресовано",
        "any" => "всего",
        other => other,
    }
}

/// Датчик по handle из checkin.
pub fn sensor(handle: &str) -> String {
    match handle {
        "-10000" => "GPS".into(),
        h => format!("датчик #{h}"),
    }
}

/// Binder-вызов в system_server («Класс#метод») → что это значит.
pub fn binder_call(call: &str) -> Option<&'static str> {
    let (cls, m) = call.split_once('#').unwrap_or((call, ""));
    Some(match () {
        _ if cls.starts_with("AccountManagerService") => "Опрос аккаунтов (Google и др.): приложение снова и снова спрашивает список аккаунтов и их данные.",
        _ if m == "sendAccessibilityEvent" => "Событие спецвозможностей: шлётся при каждом изменении экрана, пока включена какая-то служба спецвозможностей.",
        _ if cls.starts_with("NotificationManagerService") && m.starts_with("enqueueNotification") => "Публикация или обновление уведомления. Если часто — приложение обновляет уведомление (счётчик, скорость, прогресс) каждую секунду.",
        _ if cls.starts_with("NotificationManagerService") => "Работа с уведомлениями.",
        _ if cls.starts_with("PackageManagerService") || cls.starts_with("PackageManager") => "Запросы к менеджеру пакетов (какие приложения установлены, кто принимает событие). Частые — признак опроса в цикле.",
        _ if cls.starts_with("RoleService") => "Вопрос «какое приложение отвечает за SMS / звонки / браузер». Частые — опрос в цикле.",
        _ if m.starts_with("broadcastIntent") => "Рассылка системного события (broadcast) — будит все подписанные приложения.",
        _ if cls.starts_with("ActivityManagerService") && m.contains("Service") => "Запуск, привязка или отвязка службы приложения.",
        _ if m == "getRunningAppProcesses" || m == "getProcessMemoryInfo" || m == "getUidProcessState" => "Опрос состояния процессов и памяти.",
        _ if cls.starts_with("ActivityManagerService") => "Управление процессами и компонентами приложений.",
        _ if cls.starts_with("Session") || cls.starts_with("WindowManagerService") => "Окна: перерисовка, смена размеров (relayout).",
        _ if cls.starts_with("PowerManagerService") => "Блокировки сна (wakelock) и управление питанием.",
        _ if cls.starts_with("AlarmManagerService") => "Установка будильников.",
        _ if cls.starts_with("JobSchedulerService") => "Планирование фоновых заданий.",
        _ if cls.starts_with("LocationManagerService") || cls.contains("Location") => "Запросы геолокации.",
        _ if cls.starts_with("NetdEventListenerService") => "Сетевые события от netd: каждый DNS-запрос и подключение. Много — приложения очень часто открывают соединения.",
        _ if cls.contains("Connectivity") || cls.contains("NetworkStats") || cls.contains("NetworkPolicy") => "Состояние сети и сетевая статистика.",
        _ if cls.starts_with("VibratorManagerService") => "Вибрация.",
        _ if cls.starts_with("TelephonyRegistry") => "Рассылка состояния сотовой сети подписчикам.",
        _ if cls.starts_with("ShortcutService") || cls.starts_with("LauncherApps") => "Ярлыки и список приложений для лаунчера.",
        _ if cls.starts_with("ContentService") || cls.contains("Provider") => "Синхронизация и наблюдение за контент-провайдерами.",
        _ if cls.starts_with("ClipboardService") => "Буфер обмена.",
        _ if cls.starts_with("InputMethodManager") => "Клавиатура.",
        _ if cls.contains("Sensor") => "Датчики.",
        _ if cls.starts_with("BaseParceledListSlice") => "Передача длинного списка по частям (часто — список пакетов).",
        _ if cls.starts_with("PowerInsight") || cls.contains("miui") || cls.contains("Miui") => "Служба Xiaomi.",
        _ => return None,
    })
}

/// Внутренняя очередь (поток) system_server → что это.
pub fn system_queue(thread: &str) -> Option<&'static str> {
    let t = thread;
    Some(match () {
        _ if t.starts_with("UMMS") => "Xiaomi UMMS: освобождение памяти (вытеснение кэша и страниц). Много — признак нехватки ОЗУ.",
        _ if t.starts_with("PowerStrategyMode") || t.starts_with("SmartPower") || t.starts_with("bgres") => "Стратегии питания Xiaomi: следят за фоном приложений.",
        _ if t == "main" => "Главный поток system_server: уведомления, broadcast-события, многое другое.",
        _ if t.starts_with("PowerManagerService") => "Управление питанием: wakelock, экран, засыпание.",
        _ if t.starts_with("NetworkPolicy") => "Сетевые политики: ограничения фона и трафика, пересчитываются при каждой смене состояния приложений.",
        _ if t.starts_with("ActivityManager") => "Менеджер процессов: запуск, убийство, смена приоритетов приложений.",
        _ if t.starts_with("Connectivity") => "Состояние сети, VPN, переключения Wi-Fi/мобильной сети.",
        _ if t.starts_with("android.anim") || t.starts_with("android.display") || t.starts_with("android.ui") => "Анимации окон и дисплей.",
        _ if t.starts_with("AlarmManager") => "Будильники.",
        _ if t.starts_with("JobScheduler") || t.starts_with("jobscheduler") => "Фоновые задания.",
        _ if t.starts_with("batterystats") => "Учёт батареи (в том числе снимки статистики).",
        _ if t.starts_with("NetworkStats") => "Подсчёт сетевой статистики.",
        _ if t.starts_with("SchedBoost") || t.starts_with("Ssru") || t.starts_with("sentinel") || t.starts_with("Scenario") => "Службы производительности Xiaomi.",
        _ if t.starts_with("android.fg") || t.starts_with("android.bg") || t.starts_with("android.io") => "Общие фоновые потоки системных служб.",
        _ if t.starts_with("LocationManager") || t.contains("Location") || t.starts_with("Gnss") => "Геолокация.",
        _ if t.contains("Sensor") => "Датчики.",
        _ if t.contains("Audio") => "Звук.",
        _ if t.contains("Wifi") || t.contains("wifi") => "Wi-Fi.",
        _ => return None,
    })
}
