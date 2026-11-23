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

    private var liveJob: Job? = null

    init {
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

    override fun onCleared() {
        stopLive()
    }
}
