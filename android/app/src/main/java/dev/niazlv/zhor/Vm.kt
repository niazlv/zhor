package dev.niazlv.zhor

import android.app.Application
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.drawable.Drawable
import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.core.graphics.drawable.toBitmap
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.util.concurrent.ConcurrentHashMap

/** Состояние разовой загрузки. */
sealed interface Res<out T> {
    data object Idle : Res<Nothing>
    data object Loading : Res<Nothing>
    data class Ok<T>(val v: T) : Res<T>
    data class Err(val msg: String, val unsupported: Boolean = false) : Res<Nothing>
}

fun <T> Res<T>.value(): T? = (this as? Res.Ok<T>)?.v

/** Обновление приложения: от проверки до установки. */
sealed interface Upd {
    data object Idle : Upd
    data object Checking : Upd
    data object UpToDate : Upd
    data class Available(val r: Updater.Rel) : Upd
    data class Downloading(val r: Updater.Rel, val done: Long, val total: Long) : Upd
    data class Verifying(val r: Updater.Rel) : Upd
    data class Installing(val r: Updater.Rel) : Upd
    data class Failed(val msg: String, val r: Updater.Rel?) : Upd
}

/** Отчёт по отрезку графика. */
data class Seg(val from: Long, val to: Long, val res: Res<Report>)

class Vm(app: Application) : AndroidViewModel(app) {
    val core = Core(app)
    private val pm: PackageManager = app.packageManager
    private val prefs = app.getSharedPreferences("zhor", Context.MODE_PRIVATE)

    var ready by mutableStateOf(false)
    var starting by mutableStateOf(true)
    var error by mutableStateOf<String?>(null)
    var loading by mutableStateOf(false)
    var range by mutableStateOf(prefs.getString("range", "since_unplug") ?: "since_unplug")
    var report by mutableStateOf<Report?>(null)
    var updatedAt by mutableStateOf(0L)
    var live by mutableStateOf<Live?>(null)
    var liveError by mutableStateOf<String?>(null)
    var status by mutableStateOf<Res<CoreStatus>>(Res.Idle)
    var autostart by mutableStateOf(false)
    var onboarded by mutableStateOf(prefs.getBoolean("onboarded", false))

    /** Нет root, но есть отчёт в файле (Android/data/dev.niazlv.zhor/files/report.json) — показываем его. */
    var fileMode by mutableStateOf(false)
    private val files = app.getExternalFilesDir(null)

    val segs = mutableStateMapOf<String, Seg>()

    /** Отчёты по одному приложению для карточки (лента и история событий), ключ — appKey. */
    val appReps = mutableStateMapOf<String, Res<Report>>()
    var config by mutableStateOf<Res<CoreConfig>>(Res.Idle)
    var configSaving by mutableStateOf(false)
    var storage by mutableStateOf<Res<Storage>>(Res.Idle)
    var export by mutableStateOf<Res<ExportResult>>(Res.Idle)
    var prune by mutableStateOf<Res<String>>(Res.Idle)

    /** Сообщения для снекбара. */
    var toast by mutableStateOf<String?>(null)

    // ---- (0.6) сторож, действия, сравнение ----
    /** Тревоги сторожа; null внутри Ok не бывает, Err(unsupported) — ядро старше 0.6. */
    var alerts by mutableStateOf<Res<Alerts>>(Res.Idle)
    var notifAllowed by mutableStateOf(Notify.allowed(app))

    /** Нажали на уведомление: (from, to, uid) — ZhorApp откроет отчёт за период. */
    var pendingAlert by mutableStateOf<Triple<Long, Long, Int?>?>(null)
    val appStates = mutableStateMapOf<String, Res<AppState>>()
    val actionLog = mutableStateMapOf<Int, List<ActionRec>>()
    val cmps = mutableStateMapOf<String, Res<Comparison>>()
    var acting by mutableStateOf(false)

    // ---- обновления ----
    var upd by mutableStateOf<Upd>(Updater.known(app)?.let { Upd.Available(it) } ?: Upd.Idle)
    var updChecked by mutableStateOf(Updater.lastChecked(app))
    /** Нажали на уведомление об обновлении — открыть настройки. */
    var pendingSettings by mutableStateOf(false)
    /** Что ограничил Жор и что до сих пор ограничено (null — ещё не загружено или ядро старое). */
    var restricted by mutableStateOf<List<Restr>?>(null)

    /** Окно «нужны уведомления» в этом запуске уже было (закрыли «назад» — до следующего запуска). */
    var notifPromptShown by mutableStateOf(false)

    /** Окно «нужны уведомления» — не чаще раза в 3 дня после «Не сейчас». */
    fun notifPromptDue(): Boolean = System.currentTimeMillis() - prefs.getLong("notif_prompt_later", 0) > 3 * 86_400_000L

    fun notifPromptLater() {
        prefs.edit().putLong("notif_prompt_later", System.currentTimeMillis()).apply()
    }

    /** Запрос разрешения уже был: после двух отказов Android его больше не показывает. */
    var notifAsked: Boolean
        get() = prefs.getBoolean("notif_asked", false)
        set(v) = prefs.edit().putBoolean("notif_asked", v).apply()

    private var liveJob: Job? = null

    init {
        Notify.ensureChannel(app)
        start()
    }

    fun start() {
        starting = true
        error = null
        viewModelScope.launch {
            val err = withContext(Dispatchers.IO) { core.ensureRunning() }
            val f = File(files, "report.json")
            if (err != null && f.exists()) {
                try {
                    report = withContext(Dispatchers.IO) { Report.parse(JSONObject(f.readText())) }
                    updatedAt = f.lastModified()
                    File(files, "live.json").takeIf { it.exists() }?.let { live = Live.parse(JSONObject(it.readText())) }
                    fileMode = true
                    ready = true
                    error = null
                } catch (e: Exception) {
                    error = "$err\n\nФайл отчёта не прочитан: ${e.message}"
                }
                starting = false
                return@launch
            }
            error = err
            ready = err == null
            starting = false
            if (ready) {
                autostart = withContext(Dispatchers.IO) { core.autostart() }
                refresh(fresh = true)
                startUpdates()
            }
        }
    }

    fun setOnboarded() {
        onboarded = true
        prefs.edit().putBoolean("onboarded", true).apply()
    }

    fun refresh(fresh: Boolean = false) {
        if (!ready || loading || fileMode) return
        loading = true
        viewModelScope.launch {
            try {
                val r = withContext(Dispatchers.IO) { core.report(Span(range), fresh) }
                report = r
                updatedAt = System.currentTimeMillis()
                error = null
                loadAlerts()
            } catch (e: Exception) {
                error = e.message ?: e.toString()
            } finally {
                loading = false
            }
        }
    }

    fun selectRange(r: String) {
        if (range == r) return
        range = r
        prefs.edit().putString("range", r).apply()
        report = null
        refresh(fresh = false)
    }

    /** Отчёт для экрана: основной или по отрезку. */
    fun reportFor(seg: String?): Report? = if (seg == null) report else segs[seg]?.res?.value()

    fun segKey(from: Long, to: Long) = "$from-$to"

    fun appKey(uid: Int, from: Long, to: Long) = "$uid@$from-$to"

    /** Отчёт с uid за то же окно, что и экран: лента приложения + история событий Android. */
    fun loadAppReport(uid: Int, from: Long, to: Long, force: Boolean = false) {
        val k = appKey(uid, from, to)
        val cur = appReps[k]
        if (!force && cur != null && cur !is Res.Err) return
        if (fileMode) {
            appReps[k] = Res.Err("В режиме «из файла» лента приложения не строится.", unsupported = true)
            return
        }
        if (appReps.size > 24) appReps.clear()
        appReps[k] = Res.Loading
        viewModelScope.launch {
            appReps[k] = try {
                Res.Ok(withContext(Dispatchers.IO) { core.appReport(uid, from, to) })
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
        }
    }

    fun loadSegment(from: Long, to: Long, force: Boolean = false) {
        val k = segKey(from, to)
        val cur = segs[k]
        if (!force && cur != null && cur.res !is Res.Err) return
        if (fileMode) {
            segs[k] = Seg(from, to, Res.Err("В режиме «из файла» отрезки не строятся: нужен root и ядро."))
            return
        }
        segs[k] = Seg(from, to, Res.Loading)
        viewModelScope.launch {
            val res = try {
                Res.Ok(withContext(Dispatchers.IO) { core.report(Span(from = from, to = to), false) })
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
            segs[k] = Seg(from, to, res)
        }
    }

    fun startLive() {
        if (liveJob?.isActive == true || fileMode || !ready) return
        liveJob = viewModelScope.launch {
            var n = 0
            while (isActive) {
                try {
                    live = withContext(Dispatchers.IO) { core.live() }
                    liveError = null
                } catch (e: Exception) {
                    liveError = e.message
                }
                // status — раз в ~9 с: пока открыт «Сейчас», ядро в живом режиме, и шаг/цена меняются
                if (n % 3 == 0) runCatching { withContext(Dispatchers.IO) { core.status() } }.onSuccess { status = Res.Ok(it) }
                n++
                delay(3000)
            }
        }
    }

    fun stopLive() {
        liveJob?.cancel()
        liveJob = null
    }

    fun loadStatus() {
        if (fileMode) {
            status = Res.Err("Нет root — ядро недоступно, показан отчёт из файла.")
            return
        }
        if (status !is Res.Ok) status = Res.Loading
        viewModelScope.launch {
            status = try {
                Res.Ok(withContext(Dispatchers.IO) { core.status() })
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
        }
    }

    fun toggleAutostart(on: Boolean) {
        autostart = on
        viewModelScope.launch {
            withContext(Dispatchers.IO) { core.setAutostart(on) }
            autostart = withContext(Dispatchers.IO) { core.autostart() }
            toast = if (autostart) "Ядро будет запускаться при загрузке" else "Автозапуск выключен"
        }
    }

    fun restartCore() {
        status = Res.Loading
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                core.stop()
                Thread.sleep(1500)
            }
            val err = withContext(Dispatchers.IO) { core.ensureRunning() }
            error = err
            ready = err == null
            toast = if (err == null) "Ядро перезапущено" else err
            loadStatus()
        }
    }

    // ---- настройки сбора ----

    fun loadConfig() {
        if (fileMode) {
            config = Res.Err("Нужен root.")
            return
        }
        config = Res.Loading
        viewModelScope.launch {
            config = try {
                Res.Ok(withContext(Dispatchers.IO) { core.config() })
            } catch (e: Unsupported) {
                Res.Err("Настройки сбора появятся с ядром 0.3. Сейчас действуют встроенные: шаг 5 с при включённом экране, 20 с при выключенном, хранение 10 дней, снимок раз в час.", unsupported = true)
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
        }
    }

    fun saveConfig(c: CoreConfig) {
        val prev = config
        config = Res.Ok(c)
        configSaving = true
        viewModelScope.launch {
            try {
                config = Res.Ok(withContext(Dispatchers.IO) { core.setConfig(c) })
                toast = "Сохранено, ядро применило сразу"
            } catch (e: Exception) {
                config = prev
                toast = "Не сохранилось: ${e.message}"
            } finally {
                configSaving = false
            }
        }
    }

    // ---- данные ----

    fun loadStorage() {
        if (fileMode) {
            storage = Res.Err("Нужен root.")
            return
        }
        storage = Res.Loading
        viewModelScope.launch {
            storage = try {
                Res.Ok(withContext(Dispatchers.IO) { core.storage() })
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
        }
    }

    private fun exportDir() = File(getApplication<Application>().cacheDir, "export")

    fun exportName(kind: String, span: Span): String {
        val what = span.range ?: "${Fmt.stamp(span.from ?: 0)}"
        val ext = when (kind) {
            "report_text" -> "txt"
            "raw" -> "jsonl.gz"
            else -> "json"
        }
        val prefix = if (kind == "raw") "zhor-raw" else "zhor-report"
        return "$prefix-$what-${Fmt.stamp(System.currentTimeMillis())}.$ext"
    }

    fun runExport(kind: String, span: Span) {
        if (fileMode) {
            export = Res.Err("Нужен root.")
            return
        }
        export = Res.Loading
        viewModelScope.launch {
            export = try {
                val dir = exportDir()
                withContext(Dispatchers.IO) {
                    dir.listFiles()?.forEach { it.delete() } // старые выгрузки не копим
                    core.export(kind, span, File(dir, exportName(kind, span)))
                }.let { Res.Ok(it) }
            } catch (e: Unsupported) {
                Res.Err(e.message ?: "", unsupported = true)
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
        }
    }

    fun clearExport() {
        export = Res.Idle
    }

    /** Копия выгрузки в выбранный пользователем документ (SAF). */
    fun saveExportTo(uri: Uri) {
        val r = export.value() ?: return
        viewModelScope.launch {
            toast = try {
                withContext(Dispatchers.IO) {
                    val cr = getApplication<Application>().contentResolver
                    cr.openOutputStream(uri, "wt")!!.use { o -> r.file.inputStream().use { it.copyTo(o) } }
                }
                "Сохранено: ${r.file.name} (${Fmt.bytes(r.bytes)})"
            } catch (e: Exception) {
                "Не удалось сохранить: ${e.message}"
            }
        }
    }

    fun runPrune(before: Long?, all: Boolean) {
        prune = Res.Loading
        viewModelScope.launch {
            prune = try {
                val o = withContext(Dispatchers.IO) { core.prune(before, all) }
                Res.Ok("Удалено файлов: ${o.optInt("removed_files")}, освобождено ${Fmt.bytes(o.optLong("freed_bytes"))}")
            } catch (e: Unsupported) {
                Res.Err("Очистку выполняет ядро 0.3 и новее. Сейчас на телефоне более старое ядро; старые записи оно само удаляет через 10 дней.", unsupported = true)
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
            if (prune is Res.Ok) {
                toast = (prune as Res.Ok<String>).v
                loadStorage()
            }
        }
    }

    // ---- метки и иконки приложений ----
    private val labels = ConcurrentHashMap<String, String>()
    private val icons = ConcurrentHashMap<String, ImageBitmap>()
    private val noIcon = ConcurrentHashMap.newKeySet<String>()

    private fun pkgLabel(pkg: String): String = labels.getOrPut(pkg) {
        try {
            pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString()
        } catch (_: Exception) {
            pkg
        }
    }

    fun labelOf(a: App): String {
        if (a.kind != "app" && a.kind != "isolated") return a.label
        val pkg = a.packages.firstOrNull() ?: return a.label
        val base = pkgLabel(pkg)
        // приложение не установлено (удалено после отчёта, отчёт с другого телефона) — подпись из отчёта
        if (base == pkg && !a.label.startsWith(pkg)) return a.label
        val user = a.uid / 100000
        val extra = if (a.packages.size > 1) " +${a.packages.size - 1}" else ""
        return base + extra + if (user > 0) " (польз. $user)" else ""
    }

    fun iconOf(a: App): ImageBitmap? {
        val pkg = when {
            a.kind == "app" || a.kind == "isolated" -> a.packages.firstOrNull()
            a.uid % 100000 == 1000 -> "android"
            else -> null
        } ?: return null
        return iconOfPkg(pkg)
    }

    private val niceCache = ConcurrentHashMap<String, String>()

    /**
     * Тексты ядра называют приложения пакетами («com.google.android.gms +1 (польз. 10)»).
     * Подставляем человеческие имена (длинные метки — первыми, чтобы не задеть их начало короткими).
     */
    fun nice(text: String, r: Report?): String {
        if (r == null || !text.contains('.')) return text
        niceCache[text]?.let { return it }
        var out = text
        r.apps.asSequence().filter { (it.kind == "app" || it.kind == "isolated") && it.label.contains('.') }
            .sortedByDescending { it.label.length }
            .forEach { a -> if (out.contains(a.label)) out = out.replace(a.label, labelOf(a)) }
        if (niceCache.size > 3000) niceCache.clear()
        niceCache[text] = out
        return out
    }

    /** Метка и иконка по имени пакета (службы спецвозможностей и т.п.). */
    fun labelOfPackage(pkg: String): String = pkgLabel(pkg)
    fun iconOfPackage(pkg: String): ImageBitmap? = iconOfPkg(pkg)

    private fun iconOfPkg(pkg: String): ImageBitmap? {
        if (noIcon.contains(pkg)) return null
        icons[pkg]?.let { return it }
        return try {
            val d: Drawable = pm.getApplicationIcon(pkg)
            val b = d.toBitmap(96, 96).asImageBitmap()
            icons[pkg] = b
            b
        } catch (_: Exception) {
            noIcon.add(pkg)
            null
        }
    }

    /** Метка для uid из ленты или «живого» ряда. */
    fun labelOfUid(uid: Int, fallback: String, seg: String? = null): String {
        (reportFor(seg) ?: report)?.app(uid)?.let { return labelOf(it) }
        report?.app(uid)?.let { return labelOf(it) }
        if (uid % 100000 >= 10000) {
            pm.getPackagesForUid(uid)?.firstOrNull()?.let { return pkgLabel(it) }
        }
        return fallback
    }

    fun iconOfUid(uid: Int, seg: String? = null): ImageBitmap? {
        (reportFor(seg) ?: report)?.app(uid)?.let { return iconOf(it) }
        if (uid % 100000 >= 10000) pm.getPackagesForUid(uid)?.firstOrNull()?.let { return iconOfPkg(it) }
        if (uid % 100000 == 1000) return iconOfPkg("android")
        return null
    }

    // ---- обновления ----

    /** После запуска: сказать, что обновились; раз в сутки проверить новую версию, если проверка не выключена. */
    private fun startUpdates() {
        val app = getApplication<Application>()
        val p = prefs.getString("upd_pending", null)
        if (p != null) {
            prefs.edit().remove("upd_pending").apply()
            if (Updater.installedVersion(app) == p) toast = "Жор обновлён до $p"
        }
        // при запуске ничего не качается: скачанный раньше APK уже не нужен (при неудаче следующая попытка скачает заново)
        Updater.cleanup(app)
        viewModelScope.launch {
            val c = runCatching { withContext(Dispatchers.IO) { core.config() } }.getOrNull()
            if (c != null && config !is Res.Ok) config = Res.Ok(c)
            if (c?.updateCheck != false && Updater.due(app)) checkUpdate(quiet = true)
        }
    }

    fun checkUpdate(quiet: Boolean = false) {
        if (upd is Upd.Checking || upd is Upd.Downloading || upd is Upd.Verifying || upd is Upd.Installing) return
        val app = getApplication<Application>()
        val before = upd
        upd = Upd.Checking
        viewModelScope.launch {
            upd = try {
                val r = withContext(Dispatchers.IO) { Updater.check(app) }
                if (r != null) Upd.Available(r) else Upd.UpToDate
            } catch (e: Exception) {
                if (quiet) before else Upd.Failed("Не удалось проверить: ${e.message ?: e.javaClass.simpleName}", null)
            }
            updChecked = Updater.lastChecked(app)
        }
    }

    /** Скачать, проверить и поставить — только по кнопке владельца. */
    fun startUpdate(r: Updater.Rel) {
        val app = getApplication<Application>()
        upd = Upd.Downloading(r, 0, r.apkSize)
        viewModelScope.launch {
            try {
                var last = 0L
                val f = withContext(Dispatchers.IO) {
                    Updater.download(app, r) { done, total ->
                        if (done - last > 128 * 1024 || done == total) {
                            last = done
                            upd = Upd.Downloading(r, done, total)
                        }
                    }
                }
                upd = Upd.Verifying(r)
                withContext(Dispatchers.IO) { Updater.verify(app, f, r) }
                upd = Upd.Installing(r)
                prefs.edit().putString("upd_pending", r.version).commit()
                // при успехе система закроет приложение прямо здесь, ядро откроет новую версию
                val (ok, out) = try {
                    withContext(Dispatchers.IO) { core.installApk(f.absolutePath) }
                } catch (e: Unsupported) {
                    throw e
                } catch (e: Exception) {
                    throw IllegalStateException("Ядро не поставило обновление: ${e.message ?: e.javaClass.simpleName}")
                }
                if (!ok) {
                    prefs.edit().remove("upd_pending").apply()
                    upd = Upd.Failed("Android не поставил обновление: ${out.ifBlank { "нет ответа" }}", r)
                }
            } catch (e: Unsupported) {
                upd = Upd.Failed("Ставить обновления умеет ядро 0.7 и новее. Скачайте APK со страницы выпуска.", r)
            } catch (e: Exception) {
                upd = Upd.Failed(e.message ?: e.javaClass.simpleName, r)
            }
        }
    }

    fun setUpdateCheck(on: Boolean) {
        val c = config.value() ?: return
        saveConfig(c.copy(updateCheck = on))
    }

    // ---- (0.6) сторож ----

    fun openAlert(from: Long, to: Long, uid: Int?) {
        pendingAlert = Triple(from, to, uid)
    }

    fun refreshNotifState() {
        notifAllowed = Notify.allowed(getApplication())
    }

    fun loadAlerts() {
        if (fileMode) {
            // демо без root: тревоги из alerts.json рядом с report.json (ответ команды alerts)
            alerts = File(files, "alerts.json").takeIf { it.exists() }
                ?.let { f -> runCatching { Res.Ok(Alerts.parse(JSONObject(f.readText()))) }.getOrNull() }
                ?: Res.Err("Сторож работает в ядре — нужен root.")
            return
        }
        if (!ready) return
        if (alerts !is Res.Ok) alerts = Res.Loading
        viewModelScope.launch {
            alerts = try {
                Res.Ok(withContext(Dispatchers.IO) { core.alerts() })
            } catch (e: Unsupported) {
                Res.Err("Сторож появится с ядром 0.6. Сейчас на телефоне более старое ядро — перезапустите приложение, оно обновит ядро.", unsupported = true)
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
        }
    }

    fun alertTest() {
        viewModelScope.launch {
            toast = try {
                val (ok, out) = withContext(Dispatchers.IO) { core.alertTest() }
                refreshNotifState()
                when {
                    !ok -> "Ядро не достучалось до приложения: ${out.ifBlank { "нет ответа" }}"
                    !notifAllowed -> "Ядро достучалось, но уведомления Жору запрещены"
                    else -> "Отправлено — уведомление должно появиться"
                }
            } catch (e: Exception) {
                "Не вышло: ${e.message}"
            }
            loadAlerts()
        }
    }

    fun mute(key: String, on: Boolean) {
        viewModelScope.launch {
            try {
                val c = withContext(Dispatchers.IO) { core.mute(key, on) }
                config = Res.Ok(c)
                toast = if (on) "Больше не напомню об этом" else "Снова буду напоминать"
            } catch (e: Exception) {
                toast = "Не вышло: ${e.message}"
            }
            loadAlerts()
        }
    }

    // ---- действия с приложением ----

    fun loadAppState(pkg: String, uid: Int) {
        if (fileMode || !ready) return
        val k = "$uid/$pkg"
        if (appStates[k] !is Res.Ok) appStates[k] = Res.Loading
        viewModelScope.launch {
            appStates[k] = try {
                Res.Ok(withContext(Dispatchers.IO) { core.appState(pkg, uid) })
            } catch (e: Unsupported) {
                Res.Err("Действия появятся с ядром 0.6.", unsupported = true)
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
        }
        loadActions(uid)
    }

    fun act(pkg: String, uid: Int, action: String, label: String) {
        if (acting) return
        acting = true
        viewModelScope.launch {
            try {
                val (ok, st) = withContext(Dispatchers.IO) { core.act(pkg, uid, action) }
                if (st != null) appStates["$uid/$pkg"] = Res.Ok(st)
                toast = when {
                    !ok -> "Не вышло: Android не принял изменение"
                    action == "restrict" -> "$label: работа в фоне ограничена. Сравнение «до/после» — когда наберётся полтора часа покоя"
                    action == "unrestrict" -> "$label: работа в фоне снова разрешена"
                    else -> "$label остановлено"
                }
            } catch (e: Exception) {
                toast = "Не вышло: ${e.message}"
            } finally {
                acting = false
            }
            loadActions(uid)
            loadActions(null)
            loadRestricted()
        }
    }

    fun loadRestricted() {
        if (fileMode || !ready) return
        viewModelScope.launch {
            runCatching { withContext(Dispatchers.IO) { core.restricted() } }.onSuccess { restricted = it }
        }
    }

    fun revertAll() {
        if (acting) return
        acting = true
        viewModelScope.launch {
            try {
                val (ok, bad) = withContext(Dispatchers.IO) { core.revertAll() }
                toast = when {
                    bad > 0 -> "Вернул $ok, не вышло: $bad — снимите ограничение в настройках батареи приложения"
                    ok == 0 -> "Возвращать нечего"
                    else -> "Вернул всё: $ok ${Fmt.plural(ok.toLong(), "приложению", "приложениям", "приложениям")} снова разрешена работа в фоне"
                }
            } catch (e: Exception) {
                toast = "Не вышло: ${e.message}"
            } finally {
                acting = false
            }
            appStates.clear()
            loadRestricted()
            loadActions(null)
        }
    }

    /** Журнал действий: uid или все (ключ −1). */
    fun loadActions(uid: Int?) {
        if (fileMode || !ready) return
        viewModelScope.launch {
            runCatching { withContext(Dispatchers.IO) { core.actions(uid) } }.onSuccess { actionLog[uid ?: -1] = it }
        }
    }

    // ---- сравнение «до/после» ----

    fun loadCompare(req: CmpReq, force: Boolean = false) {
        val k = req.key
        if (!force && cmps[k] is Res.Ok) return
        if (fileMode) {
            // демо без root: сравнение из compare.json рядом с report.json (ответ команды compare)
            cmps[k] = File(files, "compare.json").takeIf { it.exists() }
                ?.let { f -> runCatching { Res.Ok(Comparison.parse(JSONObject(f.readText()))) }.getOrNull() }
                ?: Res.Err("Сравнение строит ядро — нужен root.")
            return
        }
        cmps[k] = Res.Loading
        viewModelScope.launch {
            cmps[k] = try {
                Res.Ok(withContext(Dispatchers.IO) { core.compare(req) })
            } catch (e: Unsupported) {
                Res.Err("Сравнение появится с ядром 0.6.", unsupported = true)
            } catch (e: Exception) {
                Res.Err(e.message ?: e.toString())
            }
        }
    }

    override fun onCleared() {
        stopLive()
    }
}
