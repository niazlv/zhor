# API ядра zhord

Приложение говорит с ядром через root-оболочку: `/data/adb/zhor/bin/zhord query '<json>'`
(см. `android/.../Core.kt`). Запрос — одна строка JSON с полем `cmd`, ответ — один JSON
(или текст у `report_text`). Ошибка — `{"error": "..."}`.

Пометки **(0.3)**, **(0.4)**, **(0.5)** — с какой версии ядра есть поле или команда. Разбирать их
необязательными (`opt*` с умолчаниями): старое ядро их не отдаёт.

Доступ: сокет принимает только root (проверка SO_PEERCRED) — приложение ходит через `su`.

## Команды

### `status` / `ping`
```json
{"ok":true,"version":"0.3.0","build":1795394350,"pid":12390,"started":1795391820774,
 "profile":"dumpsys batterystats --power-profile","clusters":2,"buckets":52,"snaps":11,
 "scan_cost_us":31383,"interval_ms":5000,"cpu_total_us":3658530,
 "cost":{"cpu_us":28732,"cpu_scan_us":27688,"cpu_sensors_us":875,"total_us":31383,"procs_us":28779,
         "wakeup_sources_us":0,"hosts_us":0,"procs":254,"skipped_idle":923,"threads":1076,"tis_reads":231},
 "dir":"/data/adb/zhor","oldest":1795389376598,
 "hist":{"events_mem":97945,"mem_bytes":4244520,"covered_from":ms,"android_from":ms,"android_to":ms,"cost_ms":209}}
```
`hist` **(0.4)** — история событий Android: событий в памяти и сколько они занимают, с какого момента есть
история (`covered_from`, вместе с диском), какой отрезок держит сейчас сам Android (`android_from…android_to`,
~45 ч) и сколько заняли разбор и запись при последнем снимке (`cost_ms`). `null` — ещё не было снимка.

Цена самого профилировщика:
- `cost.cpu_us` — процессорное время последнего шага (настоящая цена; `total_us` — настенное, оно больше,
  потому что демон работает с низким приоритетом и его вытесняют); `cpu_total_us` — CPU на шаги с запуска.
- `procs` / `skipped_idle` — сколько процессов прочитано / пропущено как простаивающие (их читают раз в 6 шагов);
  `threads`, `tis_reads` — сколько потоков разобрано (каждый процесс раз в ~30 с), `interval_ms` — текущий шаг.
- Энергию самого ядра лучше показывать из отчёта: процесс `zhord` (uid 0) в `apps[kind=kernel].processes` —
  на Xiaomi 15 Ultra при включённом экране ~2 мА, ~0.3–0.4 % расхода.

### `live`
```json
{"battery":{"draw_ma":582.0,"voltage_mv":4123.0,"charge_uah":4100000,"level":86,"temp_c":28.2,
            "status":"Discharging","plugged":false},
 "screen":{"on":true,"bright":0.42},
 "series":[{"t":ms,"dt_ms":5003,"draw_ma":..,"voltage_mv":..,"level":..,"screen":true,"plugged":false,
            "model_ma":..,"cpu_ma":..,"top":[[uid,"process name",ma],...]}],   // до 180 точек, top до 10
 "scan_cost_us":..}
```
`draw_ma` > 0 — разряд, < 0 — заряд. Ядро шагает раз в 5 с при включённом экране, раз в 20 с
бодрствования при выключенном (во сне телефона не тикает). Каждый запрос `live` включает «живой режим»
на 15 с: ядро шагает раз в 2 с. Опрашивать `live` раз в 2–3 с, пока экран «Сейчас» открыт, и перестать при уходе с него.

### `report` (и `report_text` — то же текстом для CLI)
Параметры: `range` — `since_unplug` | `screen_off` | `charge` | `1h` `3h` `6h` `12h` `24h` `48h` | `all`;
или `from`/`to` (мс, настенное время) — **любой отрезок, например кусок графика**;
`fresh: true` — перед отчётом снять свежий снимок batterystats (~1–3 с, только когда пользователь
явно обновляет); `uid` — оставить одно приложение; `max_apps`.

```text
{
 "label": "с отключения зарядки", "from": ms, "to": ms, "generated": ms,
 "recorded_from": ms,          // с какого момента в интервале есть записи ядра (процессы/потоки/ток)
 "device": {"profile": "...", "battery_full_mah": "4737", "battery_design_mah": "5410", "scan_cost_us": "..."},
 "totals": {
   "span_ms","awake_ms","suspend_ms","screen_on_ms","screen_off_ms","screen_off_suspend_ms","plugged_ms",
   "measured_mah",            // по кулоновскому счётчику, только без зарядки — ФАКТ
   "measured_avg_ma","measured_screen_on_ma","measured_screen_off_ma","measured_source",
   "model_cpu_mah","model_cpu_idle_mah","model_screen_mah","model_total_mah",   // МОДЕЛЬ
   "other_mah",               // факт − модель: радио, Wi-Fi, GPS, GPU, периферия
   "level_start","level_end","pct_per_hour","pct_per_hour_screen_off",
   "deep_sleep_pct",          // доля времени с выкл. экраном, проведённая в suspend (норма > 90)
   "suspends","suspend_fails","gpu_busy_pct"
 },
 "apps": [ {
   "uid", "label", "packages": [..], "kind": "app|system|kernel|isolated",
   "mah",                     // итог: CPU (ядро) + прочие компоненты из модели Android
   "cpu_ms","cpu_mah","cpu_mah_screen_off",
   "components": {"screen":..,"gnss":..,"mobile_radio":..,"wifi":..,"audio":..,"video":..,"camera":..,
                  "sensors":..,"wakelock":..,"bluetooth":..,"flashlight":..,"cpu":..},   // мА·ч, модель Android
   "component_ms": {...}, "states": {"fg":..,"bg":..,"fgs":..,"cached":..}, "sys_mah",
   "processes": [ {"name","uid","cpu_ms","mah","mah_screen_off","starts","explain",
                   "isolated": bool, "host_uid"?,           // (0.3) изолированный процесс; host_uid — если отнесён к хозяину
                   // имя рендера у хозяина: "WebView: sandboxed_process0 [uid 99076]"
                   "threads": [{"name","cpu_ms","mah","explain"}] } ],
   "wakelocks"|"alarms"|"jobs"|"syncs"|"sensors"|"native_wakelocks": [{"name","count","ms","explain"}],
   "net": {"mobile_rx","mobile_tx","wifi_rx","wifi_tx","mobile_bg_rx","mobile_bg_tx","wifi_bg_rx","wifi_bg_tx",
           "mobile_active_ms","mobile_active_count","mobile_wakeups","wifi_wakeups"} | null,
   "gpu_ms","crashes","anrs",
   "why": ["готовые фразы по убыванию веса"]
 } ],
 "kernel": {
   "wakeup_sources": [{"name","wakeups","activations","held_ms","prevent_suspend_ms","explain"}],
   "resume_reasons"|"wakeup_reasons"|"kernel_wakelocks"|"native_wakelocks"|"suspend_abort_reasons":
       [{"name","count","ms","explain"}]
 },
 "clusters": [{"policy","cpus":[..],"cpu_ms","mah","residency":[[khz, ms],..]}],
 "timeline": [ {"t0","t1","screen","plugged","level","meas_ma","model_ma","cpu_ma","awake_pct","top",
                "tops": [{"uid","label","ma"}],            // (0.3) топ-3 потребителя CPU отрезка, средний мА (по приложению-хозяину)
                "suspend_pct",                               // (0.3) доля отрезка в глубоком сне
                "app_ma", "app_top"} ],                      // (0.4) только при uid: CPU приложения в точке, мА, и его самый затратный процесс
 // точка = склейка соседних корзин одного состояния; до ~240 точек на интервал. Для деталей точки —
 // report c from=t0, to=t1 (полная разбивка этого отрезка).
 "sys": {"actual_mah","from","to","exact","note","global":{comp: mah},"global_ms":{..},
         "computed_mah","doze_ms","partial_wakelock_ms","mobile_radio_active_ms"} | null,
 "insights": [ {"level": "bad|warn|info|good", "title", "text",           // (0.3) выводы ядра, уже по-русски
                "uid": 10123?, "section": "sleep|apps|kernel|cpu|battery"} ],
 // uid есть — вывод про приложение (открыть его карточку); section — куда вести кнопкой «подробнее».
 // Пороги: глубокий сон ≥90 good / ≥70 warn / иначе bad (если экран выкл ≥10 мин);
 // разряд в покое <1 %/ч good / ≤3 warn / иначе bad (если ≥30 мин); пробуждений >120/ч warn, >600/ч bad.
 "notes": ["..."], "events": [{"t","kind":"screen_on|screen_off|plug|unplug|start","level"}],
 "history": {...} | отсутствует                               // (0.4) история событий Android, см. ниже
}
```

#### `history` (0.4) — история событий Android за окно

Источник — строки `9,h,…` в `dumpsys batterystats -c` (тот же вывод, что для снимков, лишнего вызова нет).
Android держит ~45 ч (буфер 4 МБ), ядро хранит дольше в `hist/` (по `keep_days`). Точность — миллисекунды.
С `uid` — только это приложение (плюс дорожки телефона для контекста), без `uid` — весь телефон.
```text
"history": {
  "from": ms, "to": ms,          // с какого момента в окне есть история (раньше — note)
  "uid": 10358?,
  "lanes": [ {"kind","label","global": bool,"count","ms",          // ms — время в окне, пересечения склеены
              "spans": [[t0, t1, событий], ...]} ],                 // соседние ближе ~1/400 окна склеены, ≤ ~400
  "items": [ {"kind","label","name","uid"?,"app"?,                  // сводка по (вид, приложение, тег)
              "count","ms","max_ms","last"} ],                     // last — начало последнего; ≤ 80 (uid) / 60
  "events": [ {"t0","t1","kind","uid"?,"app"?,"name","open"?} ],   // отдельные события: при uid — всегда,
  "events_total": n,                                               //   без uid — если окно ≤ 3 ч; ≤ 400/300 самых длинных
  "notes": ["История событий Android есть с …"]
}
```
Виды (`kind`). Приложения: `top` (на экране), `fg` (передний план), `job`, `sync`, `wakelock` — **не каждый
wakelock**, а отрезок «телефон держат wakelock'и», приписанный тому, кто взял первым (в истории Android они строго
чередуются); `longwake` (wakelock > 1 мин, отметка Android ставится через минуту после захвата), `tmpwhitelist`
(разрешён фон: высокоприоритетный пуш, будильник), `wakeupap` (входящий пакет разбудил процессор, мгновенное),
`screenwake`; только при `dumpsys batterystats enable full-history`: `wakelock_in` (каждый wakelock), `alarm`,
`proc`. Телефон (`global`): `running` (не спал), `screen`, `doze` (имя — light/full), `gps`, `mobile_radio`,
`wifi_scan`, `ble_scan`, `audio`, `video`, `camera`, `wake_reason` (причина пробуждения ядра, мгновенное).

Особенности Android 17 (Xiaomi): у пользователей ≠ 0 (второе пространство, двойные приложения) конец `tmpwhitelist`
не записывается — такие без конца дольше 10 мин ядро делает мгновенными. После аварийной перезагрузки (START без
SHUTDOWN) флаг `running` может не сбрасываться до ближайшего сброса статистики — это данные Android, не ошибка разбора.

Проверка разбора на любом телефоне: `zhord history-check [файл] [--top]` (без файла — сам вызывает dumpsys).

#### `system_work` (0.4) — за кого работает system

«Android OS (system)» (uid 1000) — это system_server, служба засыпания, композитор, SystemUI и HAL'ы.
system_server почти всё время работает **по заказу других**: приложения вызывают его службы (binder) и ставят
ему работу в очереди. Android сам ведёт выборочную статистику этого (`dumpsys binder_calls_stats`,
`dumpsys looper_stats`, каждый N-й вызов, ядро масштабирует). Ядро снимает её вместе со снимком batterystats
(~50 мс) и отдаёт разность за окно. Это та же энергия, что уже входит в «Android OS», — **не прибавлять** к
приложениям, а показывать как «из-за этого приложения система потратила ≈N мА·ч».
```text
"system_work": {
  "from": ms, "to": ms, "exact": bool, "note": "...",     // окно — между снимками со статистикой (note, если неточное)
  "binder_ms", "queue_ms",          // CPU system_server за окно: на вызовы приложений / в своих очередях, мс
  "own_ms",                         // очереди, работавшие за саму систему (без заказчика)
  "mas_per_cpu_ms",                 // цена 1 мс CPU system_server, мА·с (из записей ядра) — так считаются мА·ч
  "mah_total",                      // ≈мА·ч всей этой работы
  "callers": [ {"uid","label","packages","kind",
                "binder_ms","queue_ms","cpu_ms",   // вызовы + работа очередей от его имени
                "mah","share",                      // ≈мА·ч и доля от binder_ms+queue_ms (0..1)
                "calls",                            // число вызовов
                "top_calls": [{"call":"AccountManagerService#getUserData","cpu_ms","calls","explain"?}] } ],  // ≤ 40
  "queues": [ {"thread":"UMMS-NormalFileReclaim","cpu_ms","share","explain"?,
               "top": [{"handler","message","cpu_ms","count"}]} ],                                         // ≤ 20
  "a11y": ["com.miui.screenshot/...ScreenshotAccessibilityService"]   // включённые спецвозможности
}
```
У каждого приложения в `apps[]` (если оно нагружало систему): `system_ms`, `system_mah`, `system_calls` (≤ 8,
формат как `top_calls`) и строка в `why` («Нагружал систему: …»). У «Android OS» первая строка `why` — кто его
загружает. Выводы: «Нагружает систему: X» (доля ≥ 10 % и ≥ 1 мА·ч или ≥ 10 мА·ч) и «Спецвозможности нагружают
систему» (включена служба и события спецвозможностей ≥ 20 с CPU). Uid 1000 в `callers` — сама система
(SystemUI и др.), uid 0 — нативные демоны (netd и т. п.).

Типичная картина: сервисы Google Play опрашивают аккаунты (`AccountManagerService#getUserData`, десятки тысяч
вызовов в сутки); VPN-клиент со счётчиком скорости обновляет уведомление каждую секунду
(`NotificationManagerService#enqueueNotificationWithTag` + работа очереди уведомлений); включённая служба
спецвозможностей заставляет все приложения слать `sendAccessibilityEvent`; на Xiaomi заметны очереди UMMS
(освобождение памяти) и PowerStrategyMode.

Как читать (для подсказок в UI):
- **Факт** (`measured_*`) — кулоновский счётчик батареи, шаг ~1 мА·ч. **Модель** — оценка по
  коэффициентам производителя: CPU из времени потоков на каждой частоте, экран из яркости.
  `other_mah` — что модель не объясняет.
- `deep_sleep_pct` при выключенном экране: > 90 % — хорошо, 70–90 — так себе, < 70 — телефон не спит.
- `pct_per_hour_screen_off`: < 1 %/ч — хорошо, 1–3 — заметно, > 3 — что-то мешает спать.
- Компоненты приложений (`components`) — модель Android (batterystats) за окно между снимками;
  `cpu_mah` — модель ядра (точнее: по потокам и частотам). Окно системных данных — `sys.from…sys.to`.
- `recorded_from > from` — ядро не работало в начале интервала: процессы/потоки только с этого момента.

### `config` (0.3)
`{"cmd":"config"}` → текущие настройки; `{"cmd":"config","set":{...}}` → сохранить (в `/data/adb/zhor/config.json`, применяются сразу,
частичное обновление — можно одно поле), ответ — итоговые настройки:
```json
{"interval_on_ms":5000,"interval_off_ms":20000,"keep_days":10,"snap_every_min":60,"full_history":true,"hist_every_min":180}
```
Допустимые границы (ядро само зажимает): `interval_on_ms` 1000–60000, `interval_off_ms` 5000–600000,
`keep_days` 1–90, `snap_every_min` 10–1440, `hist_every_min` **(0.5)** 30–1440 (как часто читать полную историю
Android, если её никто не смотрит; при просмотре отчёта она и так подтягивается, если старше 5 мин). Цена: шаг сбора ~20–50 мс CPU; снимок ~0,6–0,7 с
CPU system_server (сводка `--proto` ~0,1 с + модель `--usage` ~0,35 с + мелкие дампы) и ~300 КБ на диске;
полная история (`-c --history`, ~0,4 с) — отдельно, по `hist_every_min` и при просмотре. Чаще 5 с / 20 с ставить не стоит.
`full_history` **(0.4)** — подробная история Android (`dumpsys batterystats enable full-history`): каждый wakelock
(`wakelock_in`), будильники (`alarm`) и процессы (`proc`) в `history`. Android сбрасывает опцию при перезагрузке, поэтому
ядро включает её заново перед каждым снимком, пока флаг стоит; при снятии флага — сразу `disable` (проверено: новые
`wake_lock_in` перестают появляться). Цена (замер на Xiaomi 15 Ultra): ~145 отдельных wakelock в минуту, событий
в 4–5 раз больше (~10 тыс./ч). Вывод `-c` не растёт (буфер Android фиксирован, 4 МБ), но его хватает примерно на 20 ч
вместо 45 — ядро читает историю раз в `hist_every_min` (3 ч) и при просмотре отчётов, этого хватает с запасом. В памяти ядро держит историю за 26 ч (≈ 10 МБ), старше — с диска.

### `storage` (0.3) — считать только по открытию экрана настроек, не опрашивать
```json
{"dir":"/data/adb/zhor","total_bytes":8372358,
 "groups":[{"kind":"buckets","files":2,"bytes":2156184,"records":62,"from":ms,"to":ms},
           {"kind":"snaps","files":1,"bytes":..,"records":13,...},{"kind":"events",...},
           {"kind":"log","files":1,"bytes":1908,"records":null,"from":null,"to":null},
           {"kind":"history","files":3,"bytes":1789081,"records":97945,"from":ms,"to":ms}],   // (0.4) records — событий в памяти
 "days":[{"date":"2026-11-23","bytes":..,"records":..}],
 "mem_buckets":64,"mem_snaps":14,"config":{...}}
```
`buckets` — корзины ~1 мин (процессы/потоки/ток), `snaps` — снимки batterystats, `events` — экран/зарядка,
`history` **(0.4)** — история событий Android (`hist/ГГГГ-ММ-ДД.jsonl.gz`, ~0.7 МБ в сутки).
Даты файлов — местные.

### `export` (0.3)
`{"cmd":"export","kind":"report_json|report_text|raw","range":"24h"|"from":..,"to":..,"path":"<куда>"}`
→ `{"ok":true,"path":..,"kind":..,"bytes":..,"records":{...},"from":ms,"to":ms}`;
`records` для `raw` — `{"buckets","snaps","events","hist"}`, для отчётов — `{"apps","timeline","events"}`.
Разрешённые пути: `/data/user|data|user_de/.../dev.niazlv.zhor/...`, `/data/local/tmp/`, `/sdcard/`, `/storage/emulated/`;
иначе `{"error":"путь не разрешён: ..."}`. Проверено: файл в `/data/user/0/dev.niazlv.zhor/cache/`
получает владельца `u0_a562` и метку `app_data_file:s0:c50,...` — приложение читает его обычным `File`.
Примерные размеры: `raw` за 24 ч ~1.2 МБ (gzip), `report_json` ~0.5 МБ, `report_text` ~16 КБ.
Ядро пишет файл от root и отдаёт его владельцу и SELinux-метке родительской папки, поэтому путь —
в папке приложения (`context.cacheDir`). Дальше приложение копирует файл, куда выберет пользователь
(SAF `ACTION_CREATE_DOCUMENT`) или отдаёт через «Поделиться». `raw` — все записи за интервал в
`.jsonl.gz` (строки `{"type":"bucket"|"snap"|"event"|"hist",...}`; `hist` **(0.4)** —
`{"t0","t1","kind","uid","name","open"}`).

### `prune` (0.3) — только после подтверждения пользователя
`{"cmd":"prune","before":ms}` или `{"cmd":"prune","all":true}` → `{"ok":true,"removed_files":n,"freed_bytes":..}`.
Удаляет корзины, снимки и события (до `before` или все) и с диска, и из памяти; после очистки ядро
само снимет новый снимок batterystats. Необратимо. **(0.4)** История событий тоже: всё до `before` (или всё),
и ядро запоминает границу (`hist/.floor`), чтобы не вернуть удалённое из буфера Android при следующем снимке.
