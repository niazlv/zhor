package dev.niazlv.zhor

import android.content.Context
import org.json.JSONObject
import java.io.File

/** Команда есть в контракте, но установленное ядро её ещё не знает (ядро старше 0.3). */
class Unsupported(msg: String) : Exception(msg)

/** Запрос сравнения: `rest` — два последних периода покоя; `action` — до и после момента [t]. */
data class CmpReq(val mode: String, val t: Long? = null, val uid: Int? = null) {
    val key get() = "$mode/${t ?: 0}/${uid ?: -1}"
}

/** Период запроса: пресет (`24h`, `since_unplug`…) или явный отрезок в мс. */
data class Span(val range: String? = null, val from: Long? = null, val to: Long? = null) {
    fun put(q: JSONObject): JSONObject {
        if (from != null && to != null) q.put("from", from).put("to", to) else q.put("range", range ?: "since_unplug")
        return q
    }
}

/**
 * Связь с ядром zhord. Бинарник лежит в APK как libzhord.so; при первом запуске
 * (и после обновления) копируется в /data/adb/zhor/bin и стартует демоном от root.
 */
class Core(ctx: Context) {
    private val src = ctx.applicationInfo.nativeLibraryDir + "/libzhord.so"

    companion object {
        const val DIR = "/data/adb/zhor"
        const val BIN = "$DIR/bin/zhord"
        const val BOOT = "/data/adb/service.d/zhor.sh"
    }

    /** null — всё хорошо, иначе текст ошибки. */
    fun ensureRunning(): String? {
        if (!RootShell.isRoot()) return "Нет root-доступа. Разрешите «Жору» root в KernelSU / Magisk и перезапустите приложение."
        val s = RootShell.q(src)
        // Ставим ядро из APK, только если его сборка новее установленной (никогда не откатываем);
        // `zhord start` сам перезапустит работающий демон более старой сборки.
        val r = RootShell.exec(
            "mkdir -p $DIR/bin; " +
                "nb=${'$'}($s version --build 2>/dev/null); ob=${'$'}($BIN version --build 2>/dev/null); " +
                "case ${'$'}nb in ''|*[!0-9]*) nb=0;; esac; case ${'$'}ob in ''|*[!0-9]*) ob=0;; esac; " +
                "if [ ${'$'}nb -gt ${'$'}ob ]; then cp $s $BIN.new && chmod 755 $BIN.new && mv -f $BIN.new $BIN; fi; " +
                "$BIN start 2>&1",
        )
        if (r.code != 0) return "Ядро не запустилось: ${r.out.trim()}"
        return null
    }

    /** Сырой ответ ядра (JSON или текст). */
    fun queryRaw(req: JSONObject): String {
        val r = RootShell.exec("$BIN query ${RootShell.q(req.toString())} 2>&1")
        val t = r.out.trim()
        if (t.isEmpty()) throw IllegalStateException("ядро не ответило (код ${r.code})")
        return t
    }

    fun query(req: JSONObject): JSONObject {
        val t = queryRaw(req)
        if (!t.startsWith("{")) throw IllegalStateException(t)
        val o = JSONObject(t)
        if (o.has("error")) {
            val e = o.optString("error")
            if (e.contains("неизвестная команда") || e.contains("unknown", ignoreCase = true)) throw Unsupported(e)
            throw IllegalStateException(e)
        }
        return o
    }

    fun status(): CoreStatus = CoreStatus.parse(query(JSONObject().put("cmd", "status")))

    fun report(span: Span, fresh: Boolean): Report {
        val q = span.put(JSONObject().put("cmd", "report").put("fresh", fresh))
        return Report.parse(query(q))
    }

    /** Отчёт по одному приложению: его лента (timeline[].app_ma) и история событий Android (ядро 0.4). */
    fun appReport(uid: Int, from: Long, to: Long): Report =
        Report.parse(query(JSONObject().put("cmd", "report").put("from", from).put("to", to).put("uid", uid)))

    fun live(): Live = Live.parse(query(JSONObject().put("cmd", "live")))

    fun config(): CoreConfig = CoreConfig.parse(query(JSONObject().put("cmd", "config")))

    fun setConfig(c: CoreConfig): CoreConfig =
        CoreConfig.parse(query(JSONObject().put("cmd", "config").put("set", c.json())))

    /** Место на диске. Ядро 0.3 считает само; со старым ядром — грубая оценка одним проходом stat/wc. */
    fun storage(): Storage = try {
        Storage.parse(query(JSONObject().put("cmd", "storage")))
    } catch (_: Unsupported) {
        storageFallback()
    }

    private fun storageFallback(): Storage {
        val r = RootShell.exec(
            "cd $DIR 2>/dev/null && for f in buckets/*.jsonl snaps.jsonl events.jsonl zhord.log; do " +
                "[ -f \"${'$'}f\" ] && echo \"${'$'}f ${'$'}(stat -c %s \"${'$'}f\") ${'$'}(wc -l < \"${'$'}f\")\"; done",
        )
        data class F(val path: String, val bytes: Long, val lines: Long)
        val files = r.out.lines().mapNotNull { l ->
            val p = l.trim().split(' ').filter { it.isNotEmpty() }
            if (p.size < 3) null else F(p[0], p[1].toLongOrNull() ?: 0, p[2].toLongOrNull() ?: 0)
        }
        fun group(kind: String, sel: (F) -> Boolean): StorageGroup? {
            val l = files.filter(sel)
            if (l.isEmpty()) return null
            return StorageGroup(kind, l.size, l.sumOf { it.bytes }, l.sumOf { it.lines }, 0, 0)
        }
        val groups = listOfNotNull(
            group("buckets") { it.path.startsWith("buckets/") },
            group("snaps") { it.path == "snaps.jsonl" },
            group("events") { it.path == "events.jsonl" },
            group("log") { it.path == "zhord.log" },
        )
        val days = files.filter { it.path.startsWith("buckets/") }
            .map { StorageDay(it.path.removePrefix("buckets/").removeSuffix(".jsonl"), it.bytes, it.lines) }
            .sortedBy { it.date }
        return Storage(DIR, groups.sumOf { it.bytes }, groups, days, approx = true)
    }

    /**
     * Выгрузка в [out] (папка cache/export приложения). Ядро 0.3 пишет файл само и отдаёт его владельцу папки;
     * со старым ядром отчёты (JSON и текст) пишет приложение, сырые данные — только через ядро.
     */
    fun export(kind: String, span: Span, out: File): ExportResult {
        out.parentFile?.mkdirs()
        out.delete()
        try {
            val q = span.put(JSONObject().put("cmd", "export").put("kind", kind).put("path", out.absolutePath))
            val o = query(q)
            val rec = o.optJSONObject("records")
            val records = LinkedHashMap<String, Long>()
            rec?.keys()?.forEach { records[it] = rec.optLong(it) }
            val path = o.optString("path", out.absolutePath)
            val f = File(path)
            if (!f.canRead()) throw IllegalStateException("Ядро записало файл, но приложение не может его прочитать: $path")
            return ExportResult(kind, f, o.optLong("bytes", f.length()).takeIf { it > 0 } ?: f.length(), records, o.optLong("from"), o.optLong("to"), false)
        } catch (e: Unsupported) {
            val text = when (kind) {
                "report_json" -> queryRaw(span.put(JSONObject().put("cmd", "report").put("fresh", false)))
                "report_text" -> queryRaw(span.put(JSONObject().put("cmd", "report_text")))
                else -> throw Unsupported("Сырые данные выгружает ядро 0.3 и новее. Сейчас на телефоне более старое ядро.")
            }
            if (text.startsWith("{\"error\"")) throw IllegalStateException(JSONObject(text).optString("error"))
            out.writeText(text)
            var from = 0L
            var to = 0L
            var apps = 0L
            if (kind == "report_json") runCatching {
                val o = JSONObject(text)
                from = o.optLong("from"); to = o.optLong("to"); apps = o.optJSONArray("apps")?.length()?.toLong() ?: 0
            }
            val rec = if (apps > 0) mapOf("apps" to apps) else emptyMap()
            return ExportResult(kind, out, out.length(), rec, from, to, true)
        }
    }

    // ---- (0.6) сторож, действия, сравнение ----

    fun alerts(limit: Int = 100): Alerts = Alerts.parse(query(JSONObject().put("cmd", "alerts").put("limit", limit)))

    /** Проверочная тревога: ядро шлёт её приложению тем же путём, что и настоящие. → (доставлено, вывод cmd) */
    fun alertTest(): Pair<Boolean, String> {
        val o = query(JSONObject().put("cmd", "alert_test"))
        return o.optBoolean("ok") to o.optString("out")
    }

    fun mute(key: String, on: Boolean): CoreConfig = CoreConfig.parse(query(JSONObject().put("cmd", "mute").put("key", key).put("on", on)))

    fun appState(pkg: String, uid: Int): AppState = AppState.parse(query(JSONObject().put("cmd", "app_state").put("pkg", pkg).put("uid", uid)))

    /** restrict | unrestrict | stop. → состояние после действия. */
    fun act(pkg: String, uid: Int, action: String): Pair<Boolean, AppState?> {
        val o = query(JSONObject().put("cmd", "act").put("pkg", pkg).put("uid", uid).put("action", action))
        return o.optBoolean("ok") to o.optJSONObject("state")?.let { AppState.parse(it) }
    }

    /** Что ограничил Жор и что до сих пор ограничено. */
    fun restricted(): List<Restr> {
        val arr = query(JSONObject().put("cmd", "restricted")).optJSONArray("restricted") ?: return emptyList()
        return (0 until arr.length()).mapNotNull { arr.optJSONObject(it)?.let { o -> Restr.parse(o) } }
    }

    /** Вернуть всё, что ограничил Жор. → (возвращено, не вышло) */
    fun revertAll(): Pair<Int, Int> {
        val o = query(JSONObject().put("cmd", "revert_all"))
        return (o.optJSONArray("reverted")?.length() ?: 0) to (o.optJSONArray("failed")?.length() ?: 0)
    }

    fun actions(uid: Int? = null): List<ActionRec> {
        val q = JSONObject().put("cmd", "actions")
        if (uid != null) q.put("uid", uid)
        val arr = query(q).optJSONArray("actions") ?: return emptyList()
        return (0 until arr.length()).mapNotNull { arr.optJSONObject(it)?.let { o -> ActionRec.parse(o) } }
    }

    fun compare(req: CmpReq): Comparison {
        val q = JSONObject().put("cmd", "compare").put("mode", req.mode)
        if (req.t != null) q.put("t", req.t)
        if (req.uid != null) q.put("uid", req.uid)
        return Comparison.parse(query(q))
    }

    fun prune(before: Long?, all: Boolean): JSONObject {
        val q = JSONObject().put("cmd", "prune")
        if (all) q.put("all", true) else q.put("before", before ?: 0L)
        return query(q)
    }

    fun stop() {
        RootShell.exec("$BIN stop")
    }

    fun autostart(): Boolean = RootShell.exec("test -f $BOOT").code == 0

    fun setAutostart(on: Boolean) {
        if (on) {
            val script = """#!/system/bin/sh
# Жор: автозапуск ядра профилировщика энергии
until [ "${'$'}(getprop sys.boot_completed)" = "1" ]; do sleep 5; done
$BIN start
"""
            RootShell.exec("mkdir -p /data/adb/service.d && printf '%s' ${RootShell.q(script)} > $BOOT && chmod 755 $BOOT")
        } else {
            RootShell.exec("rm -f $BOOT")
        }
    }
}
