package dev.niazlv.zhor

import org.json.JSONArray
import org.json.JSONObject

// Зеркало JSON ядра (docs/api.md). Поля «(новое)» из ядра 0.3 — необязательные.

private fun JSONObject.str(k: String) = optString(k, "")
private fun JSONObject.dbl(k: String) = optDouble(k, 0.0).let { if (it.isNaN()) 0.0 else it }
private fun JSONObject.dblOrNull(k: String): Double? = if (!has(k) || isNull(k)) null else optDouble(k).takeUnless { it.isNaN() }
private fun JSONObject.lng(k: String) = optLong(k, 0L)
private fun JSONObject.strOrNull(k: String): String? = if (isNull(k) || !has(k)) null else optString(k).takeIf { it.isNotEmpty() }
private fun JSONObject.intOrNull(k: String): Int? = if (!has(k) || isNull(k)) null else optInt(k)
private inline fun <T> JSONArray?.list(f: (JSONObject) -> T): List<T> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it)?.let(f) }

/** Массив массивов: [[имя, число], …]. */
private inline fun <T> JSONArray?.list2(f: (JSONArray) -> T): List<T> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONArray(it)?.let(f) }

private fun JSONArray?.strings(): List<String> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optString(it, null) }

private fun JSONObject?.dmap(): Map<String, Double> {
    if (this == null) return emptyMap()
    val m = LinkedHashMap<String, Double>()
    keys().forEach { m[it] = optDouble(it, 0.0).let { v -> if (v.isNaN()) 0.0 else v } }
    return m
}

private fun JSONObject?.lmap(): Map<String, Long> {
    if (this == null) return emptyMap()
    val m = LinkedHashMap<String, Long>()
    keys().forEach { m[it] = optLong(it, 0) }
    return m
}

data class Thr(val name: String, val cpuMs: Long, val mah: Double, val explain: String?)
data class Proc(
    val name: String, val uid: Int, val cpuMs: Long, val mah: Double, val mahScreenOff: Double,
    val starts: Int, val threads: List<Thr>, val explain: String?,
    val hostUid: Int?, val isolated: Boolean,
)

data class Item(val name: String, val count: Long, val ms: Long, val explain: String?)
data class Net(
    val mobileRx: Long, val mobileTx: Long, val wifiRx: Long, val wifiTx: Long,
    val mobileBg: Long, val wifiBg: Long, val mobileActiveMs: Long, val mobileActiveCount: Long,
    val mobileWakeups: Long, val wifiWakeups: Long,
) {
    val total get() = mobileRx + mobileTx + wifiRx + wifiTx
}

/** (0.8) Сеть приложения в покое (экран выключен, без зарядки) — по замерам ядра. */
data class NetIdle(
    val rxPackets: Long, val rxBytes: Long, val txPackets: Long, val txBytes: Long,
    /** принято пакетов в час покоя */
    val rxPerHour: Double,
    /** доля среди всех принятых в покое пакетов, 0..1 (у VPN-клиента 0) */
    val share: Double,
    /** VPN-клиент: его пакеты — зашифрованный трафик других приложений */
    val vpn: Boolean,
) {
    companion object {
        fun parse(o: JSONObject) = NetIdle(
            o.lng("rx_packets"), o.lng("rx_bytes"), o.lng("tx_packets"), o.lng("tx_bytes"),
            o.dbl("rx_per_hour"), o.dbl("share"), o.optBoolean("vpn", false),
        )
    }
}

data class NetWaker(val uid: Int, val label: String, val packages: List<String>, val kind: String, val n: NetIdle)

/** (0.8) Кто будит по сети в покое. */
data class NetWakers(
    val idleMs: Long, val since: Long, val wifiWakeups: Long, val mobileWakeups: Long, val rxPackets: Long,
    val apps: List<NetWaker>,
) {
    val wakeupsPerHour get() = if (idleMs > 0) (wifiWakeups + mobileWakeups) * 3_600_000.0 / idleMs else 0.0

    companion object {
        fun parse(o: JSONObject) = NetWakers(
            o.lng("idle_ms"), o.lng("since"), o.lng("wifi_wakeups"), o.lng("mobile_wakeups"), o.lng("rx_packets"),
            o.optJSONArray("apps").list { a -> NetWaker(a.optInt("uid"), a.str("label"), a.optJSONArray("packages").strings(), a.str("kind"), NetIdle.parse(a)) },
        )
    }
}

/** (0.8) Часть расхода «Android OS»: процессы по смыслу. */
data class OsPart(
    val key: String, val title: String, val explain: String, val mah: Double, val mahScreenOff: Double, val share: Double,
    val top: List<Pair<String, Double>>,
)

data class App(
    val uid: Int, val label: String, val packages: List<String>, val kind: String,
    val mah: Double, val cpuMs: Long, val cpuMah: Double, val cpuMahScreenOff: Double,
    val components: Map<String, Double>, val componentMs: Map<String, Long>, val states: Map<String, Double>,
    val sysMah: Double, val processes: List<Proc>,
    val wakelocks: List<Item>, val alarms: List<Item>, val jobs: List<Item>, val syncs: List<Item>,
    val sensors: List<Item>, val nativeWakelocks: List<Item>, val net: Net?, val gpuMs: Long,
    val crashes: Long, val anrs: Long, val why: List<String>,
    /** (0.4) Сколько system_server потратил по запросам этого приложения: CPU, ≈мА·ч, топ-вызовы. Уже входит в «Android OS». */
    val systemMs: Double = 0.0, val systemMah: Double = 0.0, val systemCalls: List<SysCall> = emptyList(),
    /** (0.8) Сеть в покое; null — ядро старше или замеров в окне нет. */
    val netIdle: NetIdle? = null,
    /** (0.8) Только «Android OS»: из чего состоит расход. */
    val parts: List<OsPart> = emptyList(),
) {
    /** CPU по ядру, если есть, иначе по модели Android. */
    val cpuBest get() = if (cpuMah > 0) cpuMah else components["cpu"] ?: 0.0
    val bgMah get() = (states["bg"] ?: 0.0) + (states["fgs"] ?: 0.0) + (states["cached"] ?: 0.0)
}

data class Totals(
    val spanMs: Long, val awakeMs: Long, val suspendMs: Long, val screenOnMs: Long, val screenOffMs: Long,
    val screenOffSuspendMs: Long, val plugMs: Long, val measuredMah: Double, val measuredAvgMa: Double,
    val measuredOnMa: Double, val measuredOffMa: Double, val measuredSource: String, val modelCpuMah: Double,
    val modelIdleMah: Double, val modelScreenMah: Double, val modelTotalMah: Double, val otherMah: Double,
    val levelStart: Int, val levelEnd: Int, val pctPerHour: Double, val pctPerHourOff: Double,
    val deepSleepPct: Double, val suspends: Long, val suspendFails: Long, val gpuBusyPct: Double,
)

data class WsItem(val name: String, val wakeups: Long, val activations: Long, val heldMs: Long, val preventMs: Long, val explain: String?)
data class Kernel(
    val wakeupSources: List<WsItem>, val resumeReasons: List<Item>, val wakeupReasons: List<Item>,
    val kernelWakelocks: List<Item>, val nativeWakelocks: List<Item>, val abortReasons: List<Item>,
)

data class Cluster(val policy: String, val cpus: List<Int>, val cpuMs: Long, val mah: Double, val residency: List<Pair<Long, Long>>) {
    val maxKhz get() = residency.maxOfOrNull { it.first } ?: 0L
}

data class TopItem(val uid: Int, val label: String, val ma: Double)
data class Point(
    val t0: Long, val t1: Long, val screen: Boolean, val plugged: Boolean, val level: Int,
    val measMa: Double, val modelMa: Double, val cpuMa: Double, val awakePct: Double, val top: String,
    val tops: List<TopItem>, val suspendPct: Double?,
    /** Только в отчёте по одному приложению (ядро 0.4): его CPU в точке, мА, и самый затратный процесс. */
    val appMa: Double? = null, val appTop: String? = null,
) {
    val mah get() = measMa * (t1 - t0) / 3_600_000.0
}

// ---- история событий Android (ядро 0.4, поле report.history) ----

/** Дорожка: [t0, t1, событий] — соседние события ближе ~1/400 окна склеены. */
data class HLane(val kind: String, val label: String, val global: Boolean, val count: Long, val ms: Long, val spans: List<LongArray>)
data class HItem(
    val kind: String, val label: String, val name: String, val uid: Int?, val app: String?,
    val count: Long, val ms: Long, val maxMs: Long, val last: Long,
)
data class HEvent(val t0: Long, val t1: Long, val kind: String, val uid: Int?, val app: String?, val name: String, val open: Boolean) {
    val dur get() = t1 - t0
}
data class History(
    val from: Long, val to: Long, val uid: Int?, val lanes: List<HLane>, val items: List<HItem>,
    val events: List<HEvent>, val eventsTotal: Int, val notes: List<String>,
) {
    companion object {
        fun parse(h: JSONObject): History = History(
            h.lng("from"), h.lng("to"), h.intOrNull("uid"),
            h.optJSONArray("lanes").list { l ->
                val sp = l.optJSONArray("spans")
                HLane(
                    l.str("kind"), l.str("label"), l.optBoolean("global"), l.lng("count"), l.lng("ms"),
                    if (sp == null) emptyList() else (0 until sp.length()).mapNotNull { i ->
                        sp.optJSONArray(i)?.let { longArrayOf(it.optLong(0), it.optLong(1), it.optLong(2)) }
                    },
                )
            },
            h.optJSONArray("items").list { i ->
                HItem(
                    i.str("kind"), i.str("label"), i.str("name"), i.intOrNull("uid"), i.strOrNull("app"),
                    i.lng("count"), i.lng("ms"), i.lng("max_ms"), i.lng("last"),
                )
            },
            h.optJSONArray("events").list { e ->
                HEvent(e.lng("t0"), e.lng("t1"), e.str("kind"), e.intOrNull("uid"), e.strOrNull("app"), e.str("name"), e.optBoolean("open"))
            },
            h.optInt("events_total"), h.optJSONArray("notes").strings(),
        )
    }
}

data class SysWin(
    val actualMah: Double, val from: Long, val to: Long, val exact: Boolean, val note: String?,
    val global: Map<String, Double>, val globalMs: Map<String, Long>, val computedMah: Double,
    val dozeMs: Long, val partialWlMs: Long, val radioMs: Long,
)

data class Ev(val t: Long, val kind: String, val level: Int)
data class Insight(val level: String, val title: String, val text: String, val uid: Int?, val section: String?)

data class Report(
    val recordedFrom: Long, val generated: Long,
    val from: Long, val to: Long, val label: String, val totals: Totals, val apps: List<App>,
    val kernel: Kernel, val clusters: List<Cluster>, val timeline: List<Point>, val sys: SysWin?,
    val notes: List<String>, val events: List<Ev>, val device: Map<String, String>,
    /** null — ядро старше 0.3 и выводов не прислало. */
    val insights: List<Insight>?,
    /** null — ядро старше 0.4 или истории Android ещё нет. */
    val history: History? = null,
    /** (0.4) За кого работает system_server; null — ядро старше или снимков с разбором ещё нет. */
    val systemWork: SystemWork? = null,
    /** (0.8) Кто будит по сети в покое; null — ядро старше или замеров сети в окне нет. */
    val netWakers: NetWakers? = null,
) {
    val fullMah get() = device["battery_full_mah"]?.toDoubleOrNull() ?: 0.0
    val designMah get() = device["battery_design_mah"]?.toDoubleOrNull() ?: 0.0
    fun app(uid: Int) = apps.firstOrNull { it.uid == uid }

    companion object {
        private fun item(o: JSONObject) = Item(o.str("name"), o.lng("count"), o.lng("ms"), o.strOrNull("explain"))

        fun parse(o: JSONObject): Report {
            if (o.has("error")) throw IllegalStateException(o.str("error"))
            val t = o.optJSONObject("totals") ?: JSONObject()
            val totals = Totals(
                t.lng("span_ms"), t.lng("awake_ms"), t.lng("suspend_ms"), t.lng("screen_on_ms"), t.lng("screen_off_ms"),
                t.lng("screen_off_suspend_ms"), t.lng("plugged_ms"), t.dbl("measured_mah"), t.dbl("measured_avg_ma"),
                t.dbl("measured_screen_on_ma"), t.dbl("measured_screen_off_ma"), t.str("measured_source"),
                t.dbl("model_cpu_mah"), t.dbl("model_cpu_idle_mah"), t.dbl("model_screen_mah"), t.dbl("model_total_mah"),
                t.dbl("other_mah"), t.optInt("level_start"), t.optInt("level_end"), t.dbl("pct_per_hour"),
                t.dbl("pct_per_hour_screen_off"), t.dbl("deep_sleep_pct"), t.lng("suspends"), t.lng("suspend_fails"),
                t.dbl("gpu_busy_pct"),
            )
            val apps = o.optJSONArray("apps").list { a ->
                val n = a.optJSONObject("net")
                App(
                    a.optInt("uid"), a.str("label"), a.optJSONArray("packages").strings(),
                    a.str("kind"), a.dbl("mah"), a.lng("cpu_ms"), a.dbl("cpu_mah"), a.dbl("cpu_mah_screen_off"),
                    a.optJSONObject("components").dmap(), a.optJSONObject("component_ms").lmap(), a.optJSONObject("states").dmap(),
                    a.dbl("sys_mah"),
                    a.optJSONArray("processes").list { p ->
                        Proc(
                            p.str("name"), p.optInt("uid"), p.lng("cpu_ms"), p.dbl("mah"), p.dbl("mah_screen_off"), p.optInt("starts"),
                            p.optJSONArray("threads").list { th -> Thr(th.str("name"), th.lng("cpu_ms"), th.dbl("mah"), th.strOrNull("explain")) },
                            p.strOrNull("explain"), p.intOrNull("host_uid"), p.optBoolean("isolated", false),
                        )
                    },
                    a.optJSONArray("wakelocks").list(::item), a.optJSONArray("alarms").list(::item),
                    a.optJSONArray("jobs").list(::item), a.optJSONArray("syncs").list(::item),
                    a.optJSONArray("sensors").list(::item), a.optJSONArray("native_wakelocks").list(::item),
                    n?.let {
                        Net(
                            it.lng("mobile_rx"), it.lng("mobile_tx"), it.lng("wifi_rx"), it.lng("wifi_tx"),
                            it.lng("mobile_bg_rx") + it.lng("mobile_bg_tx"), it.lng("wifi_bg_rx") + it.lng("wifi_bg_tx"),
                            it.lng("mobile_active_ms"), it.lng("mobile_active_count"),
                            it.lng("mobile_wakeups"), it.lng("wifi_wakeups"),
                        )
                    },
                    a.lng("gpu_ms"), a.lng("crashes"), a.lng("anrs"), a.optJSONArray("why").strings(),
                    a.dbl("system_ms"), a.dbl("system_mah"), a.optJSONArray("system_calls").list(::sysCall),
                    a.optJSONObject("net_idle")?.let { NetIdle.parse(it) },
                    a.optJSONArray("parts").list { p ->
                        OsPart(
                            p.str("key"), p.str("title"), p.str("explain"), p.dbl("mah"), p.dbl("mah_screen_off"), p.dbl("share"),
                            p.optJSONArray("top").list2 { x -> x.optString(0) to x.optDouble(1, 0.0) },
                        )
                    },
                )
            }
            val k = o.optJSONObject("kernel") ?: JSONObject()
            val kernel = Kernel(
                k.optJSONArray("wakeup_sources").list { w ->
                    WsItem(w.str("name"), w.lng("wakeups"), w.lng("activations"), w.lng("held_ms"), w.lng("prevent_suspend_ms"), w.strOrNull("explain"))
                },
                k.optJSONArray("resume_reasons").list(::item), k.optJSONArray("wakeup_reasons").list(::item),
                k.optJSONArray("kernel_wakelocks").list(::item), k.optJSONArray("native_wakelocks").list(::item),
                k.optJSONArray("suspend_abort_reasons").list(::item),
            )
            val clusters = o.optJSONArray("clusters").list { c ->
                val res = c.optJSONArray("residency")
                Cluster(
                    c.str("policy"),
                    c.optJSONArray("cpus")?.let { arr -> (0 until arr.length()).map { arr.optInt(it) } } ?: emptyList(),
                    c.lng("cpu_ms"), c.dbl("mah"),
                    if (res == null) emptyList() else (0 until res.length()).mapNotNull { i ->
                        res.optJSONArray(i)?.let { it.optLong(0) to it.optLong(1) }
                    },
                )
            }
            val tl = o.optJSONArray("timeline").list { p ->
                Point(
                    p.lng("t0"), p.lng("t1"), p.optBoolean("screen"), p.optBoolean("plugged"), p.optInt("level"),
                    p.dbl("meas_ma"), p.dbl("model_ma"), p.dbl("cpu_ma"), p.dbl("awake_pct"), p.str("top"),
                    p.optJSONArray("tops").list { x -> TopItem(x.optInt("uid"), x.str("label"), x.dbl("ma")) },
                    p.dblOrNull("suspend_pct"), p.dblOrNull("app_ma"), p.strOrNull("app_top"),
                )
            }.sortedBy { it.t0 }
            val sys = o.optJSONObject("sys")?.let { s ->
                SysWin(
                    s.dbl("actual_mah"), s.lng("from"), s.lng("to"), s.optBoolean("exact"), s.strOrNull("note"),
                    s.optJSONObject("global").dmap(), s.optJSONObject("global_ms").lmap(), s.dbl("computed_mah"),
                    s.lng("doze_ms"), s.lng("partial_wakelock_ms"), s.lng("mobile_radio_active_ms"),
                )
            }
            val events = o.optJSONArray("events").list { e -> Ev(e.lng("t"), e.str("kind"), e.optInt("level")) }
            val dev = o.optJSONObject("device")?.let { d -> d.keys().asSequence().associateWith { d.optString(it) } } ?: emptyMap()
            val ins = o.optJSONArray("insights")?.list { i ->
                Insight(i.str("level"), i.str("title"), i.str("text"), i.intOrNull("uid"), i.strOrNull("section"))
            }
            return Report(
                o.lng("recorded_from"), o.lng("generated"), o.lng("from"), o.lng("to"), o.str("label"), totals, apps,
                kernel, clusters, tl, sys, o.optJSONArray("notes").strings(), events, dev, ins,
                o.optJSONObject("history")?.let { History.parse(it) },
                o.optJSONObject("system_work")?.let { SystemWork.parse(it) },
                o.optJSONObject("net_wakers")?.let { NetWakers.parse(it) },
            )
        }
    }
}

data class LiveTop(val uid: Int, val name: String, val ma: Double)
data class LiveTick(
    val t: Long, val dtMs: Long, val drawMa: Double, val modelMa: Double, val cpuMa: Double, val voltageMv: Double,
    val screen: Boolean, val plugged: Boolean, val level: Int, val top: List<LiveTop>,
)

data class Live(
    val ticks: List<LiveTick>, val drawMa: Double, val voltageMv: Double, val chargeUah: Long, val level: Int,
    val temp: Double, val status: String, val plugged: Boolean, val screenOn: Boolean, val bright: Double?,
    val scanCostUs: Long, val at: Long,
) {
    companion object {
        fun parse(o: JSONObject): Live {
            if (o.has("error")) throw IllegalStateException(o.optString("error"))
            val b = o.optJSONObject("battery") ?: JSONObject()
            val sc = o.optJSONObject("screen")
            val ticks = o.optJSONArray("series").list { t ->
                val top = t.optJSONArray("top")
                LiveTick(
                    t.lng("t"), t.lng("dt_ms"), t.dbl("draw_ma"), t.dbl("model_ma"), t.dbl("cpu_ma"), t.dbl("voltage_mv"),
                    t.optBoolean("screen"), t.optBoolean("plugged"), t.optInt("level"),
                    if (top == null) emptyList() else (0 until top.length()).mapNotNull { i ->
                        top.optJSONArray(i)?.let { LiveTop(it.optInt(0), it.optString(1), it.optDouble(2, 0.0)) }
                    },
                )
            }.sortedBy { it.t }
            return Live(
                ticks, b.dbl("draw_ma"), b.dbl("voltage_mv"), b.lng("charge_uah"), b.optInt("level"), b.dbl("temp_c"),
                b.str("status"), b.optBoolean("plugged"), sc?.optBoolean("on") ?: (ticks.lastOrNull()?.screen ?: false),
                sc?.dblOrNull("bright"), o.lng("scan_cost_us"), System.currentTimeMillis(),
            )
        }
    }
}

data class CoreStatus(
    val version: String, val build: Long, val pid: Long, val started: Long, val profile: String, val clusters: Int,
    val buckets: Long, val snaps: Long, val scanCostUs: Long, val dir: String, val oldest: Long,
    /** Разбивка цены прохода (ядро 0.3): процессов, потоков, чтений time_in_state. */
    val costProcs: Long, val costThreads: Long, val costTis: Long,
    /** CPU-время последнего шага (настоящая цена; scanCostUs — настенное), пропущено простаивающих, шаг, CPU с запуска. */
    val costCpuUs: Long, val skippedIdle: Long, val intervalMs: Long, val cpuTotalUs: Long,
) {
    companion object {
        fun parse(o: JSONObject): CoreStatus {
            if (o.has("error")) throw IllegalStateException(o.optString("error"))
            return CoreStatus(
                o.str("version"), o.lng("build"), o.lng("pid"), o.lng("started"), o.str("profile"), o.optInt("clusters"),
                o.lng("buckets"), o.lng("snaps"), o.lng("scan_cost_us"), o.str("dir"), o.lng("oldest"),
                o.optJSONObject("cost")?.lng("procs") ?: 0, o.optJSONObject("cost")?.lng("threads") ?: 0,
                o.optJSONObject("cost")?.lng("tis_reads") ?: 0,
                o.optJSONObject("cost")?.lng("cpu_us") ?: 0, o.optJSONObject("cost")?.lng("skipped_idle") ?: 0,
                o.lng("interval_ms"), o.lng("cpu_total_us"),
            )
        }
    }
}

data class CoreConfig(
    val intervalOnMs: Long, val intervalOffMs: Long, val keepDays: Int, val snapEveryMin: Int,
    /** Подробная история Android (ядро 0.4): каждый wakelock, будильники, процессы. */
    val fullHistory: Boolean = false,
    /** (0.6) Сторож и пороги его правил; 0 — правило выключено. null — ядро сторожа не знает. */
    val watch: Boolean? = null,
    val watchSleepPct: Double = 50.0,
    val watchDrainPctH: Double = 3.0,
    val watchWakeupsH: Double = 600.0,
    val watchAppMa: Double = 10.0,
    val watchHotC: Double = 40.0,
    val watchMute: List<String> = emptyList(),
    /** (0.7) Раз в сутки проверять новую версию. null — ядро этого не знает. */
    val updateCheck: Boolean? = null,
) {
    fun json(): JSONObject {
        val o = JSONObject()
            .put("interval_on_ms", intervalOnMs).put("interval_off_ms", intervalOffMs)
            .put("keep_days", keepDays).put("snap_every_min", snapEveryMin).put("full_history", fullHistory)
        // «не напоминать» не шлём: его меняет команда mute (в том числе из уведомления), чтобы не затереть
        if (watch != null) o.put("watch", watch).put("watch_sleep_pct", watchSleepPct).put("watch_drain_pct_h", watchDrainPctH)
            .put("watch_wakeups_h", watchWakeupsH).put("watch_app_ma", watchAppMa).put("watch_hot_c", watchHotC)
        if (updateCheck != null) o.put("update_check", updateCheck)
        return o
    }

    companion object {
        val DEFAULT = CoreConfig(5000, 20000, 10, 60)
        fun parse(o: JSONObject): CoreConfig {
            val c = o.optJSONObject("config") ?: o
            return CoreConfig(
                c.optLong("interval_on_ms", DEFAULT.intervalOnMs), c.optLong("interval_off_ms", DEFAULT.intervalOffMs),
                c.optInt("keep_days", DEFAULT.keepDays), c.optInt("snap_every_min", DEFAULT.snapEveryMin),
                c.optBoolean("full_history", false),
                if (c.has("watch")) c.optBoolean("watch") else null,
                c.optDouble("watch_sleep_pct", 50.0), c.optDouble("watch_drain_pct_h", 3.0),
                c.optDouble("watch_wakeups_h", 600.0), c.optDouble("watch_app_ma", 10.0),
                c.optDouble("watch_hot_c", 40.0), c.optJSONArray("watch_mute").strings(),
                if (c.has("update_check")) c.optBoolean("update_check") else null,
            )
        }
    }
}

data class StorageGroup(val kind: String, val files: Int, val bytes: Long, val records: Long, val from: Long, val to: Long)
data class StorageDay(val date: String, val bytes: Long, val records: Long)
data class Storage(val dir: String, val totalBytes: Long, val groups: List<StorageGroup>, val days: List<StorageDay>, val approx: Boolean) {
    companion object {
        fun parse(o: JSONObject): Storage = Storage(
            o.str("dir"), o.lng("total_bytes"),
            o.optJSONArray("groups").list { g -> StorageGroup(g.str("kind"), g.optInt("files"), g.lng("bytes"), g.lng("records"), g.lng("from"), g.lng("to")) },
            o.optJSONArray("days").list { d -> StorageDay(d.str("date"), d.lng("bytes"), d.lng("records")) }.sortedBy { it.date },
            false,
        )
    }
}

data class ExportResult(
    val kind: String, val file: java.io.File, val bytes: Long, val records: Map<String, Long>,
    val from: Long, val to: Long, val viaApp: Boolean,
)

// ---- (0.4) за кого работает system: binder-вызовы и очереди system_server ----

data class SysCall(val call: String, val cpuMs: Double, val calls: Long, val explain: String?) {
    /** «AccountManagerService#getUserData» → служба и метод. */
    val service get() = call.substringBefore('#')
    val method get() = call.substringAfter('#', "")
}

private fun sysCall(o: JSONObject) = SysCall(o.str("call"), o.dbl("cpu_ms"), o.lng("calls"), o.strOrNull("explain"))

data class SysCaller(
    val uid: Int, val label: String, val packages: List<String>, val kind: String,
    val binderMs: Double, val queueMs: Double, val cpuMs: Double, val mah: Double, val share: Double, val calls: Long,
    val top: List<SysCall>,
)

data class SysQueueMsg(val handler: String, val message: String, val cpuMs: Double, val count: Long)
data class SysQueue(val thread: String, val cpuMs: Double, val share: Double, val explain: String?, val top: List<SysQueueMsg>)

data class SystemWork(
    val from: Long, val to: Long, val exact: Boolean, val note: String?,
    val binderMs: Double, val queueMs: Double, val ownMs: Double, val masPerCpuMs: Double, val mahTotal: Double,
    val callers: List<SysCaller>, val queues: List<SysQueue>, val a11y: List<String>,
    /** (0.8) весь system_server за окно отчёта, мА·ч (0 — ядро старше) */
    val serverMah: Double = 0.0,
) {
    val totalMs get() = binderMs + queueMs
    /** ≈мА·ч работы «за саму систему» (очереди без заказчика). */
    val ownMah get() = ownMs * masPerCpuMs / 3600.0

    companion object {
        fun parse(o: JSONObject) = SystemWork(
            o.lng("from"), o.lng("to"), o.optBoolean("exact"), o.strOrNull("note"),
            o.dbl("binder_ms"), o.dbl("queue_ms"), o.dbl("own_ms"), o.dbl("mas_per_cpu_ms"), o.dbl("mah_total"),
            o.optJSONArray("callers").list { c ->
                SysCaller(
                    c.optInt("uid"), c.str("label"), c.optJSONArray("packages").strings(), c.str("kind"),
                    c.dbl("binder_ms"), c.dbl("queue_ms"), c.dbl("cpu_ms"), c.dbl("mah"), c.dbl("share"), c.lng("calls"),
                    c.optJSONArray("top_calls").list(::sysCall),
                )
            },
            o.optJSONArray("queues").list { q ->
                SysQueue(
                    q.str("thread"), q.dbl("cpu_ms"), q.dbl("share"), q.strOrNull("explain"),
                    q.optJSONArray("top").list { m -> SysQueueMsg(m.str("handler"), m.str("message"), m.dbl("cpu_ms"), m.lng("count")) },
                )
            },
            o.optJSONArray("a11y").strings(),
            o.dbl("server_mah"),
        )
    }
}

// ---- (0.6) сторож, действия, сравнение «до/после» ----

/** Тревога сторожа: что заметил ядро в текущем покое. */
data class WatchAlert(
    val id: String, val t: Long, val rule: String, val key: String, val level: String,
    val title: String, val text: String, val uid: Int?, val pkg: String?,
    val from: Long, val to: Long, val sent: Boolean,
) {
    companion object {
        fun parse(o: JSONObject) = WatchAlert(
            o.str("id"), o.lng("t"), o.str("rule"), o.str("key"), o.str("level"), o.str("title"), o.str("text"),
            o.intOrNull("uid"), o.strOrNull("pkg"), o.lng("from"), o.lng("to"), o.optBoolean("sent"),
        )
    }
}

data class Alerts(val list: List<WatchAlert>, val watch: Boolean, val mute: List<String>, val lastEval: Long) {
    companion object {
        fun parse(o: JSONObject) = Alerts(
            o.optJSONArray("alerts").list { WatchAlert.parse(it) }, o.optBoolean("watch", true),
            o.optJSONArray("mute").strings(), o.lng("last_eval"),
        )
    }
}

/** Может ли приложение работать в фоне (appop RUN_ANY_IN_BACKGROUND) и его корзина ожидания. */
data class AppState(val pkg: String, val bg: String?, val restricted: Boolean, val bucket: Int?, val bucketName: String?) {
    companion object {
        fun parse(o: JSONObject) = AppState(o.str("pkg"), o.strOrNull("bg"), o.optBoolean("restricted"), o.intOrNull("bucket"), o.strOrNull("bucket_name"))
    }
}

data class ActionRec(
    val t: Long, val pkg: String, val uid: Int?, val action: String, val ok: Boolean, val prev: String?,
    /** (0.8) "" — вручную, mode — экономный режим, clean — авто-уборка */
    val by: String = "", val out: String = "",
) {
    companion object {
        fun parse(o: JSONObject) = ActionRec(
            o.lng("t"), o.str("pkg"), o.intOrNull("uid"), o.str("action"), o.optBoolean("ok"), o.strOrNull("prev"),
            o.str("by"), o.str("out"),
        )
    }
}

/** Ограничение фона, которое поставил Жор и которое до сих пор действует; [prev] — режим до Жора. */
data class Restr(val pkg: String, val uid: Int?, val since: Long, val prev: String, val by: String = "") {
    companion object {
        fun parse(o: JSONObject) = Restr(o.str("pkg"), o.intOrNull("uid"), o.lng("since"), o.str("prev"), o.str("by"))
    }
}

// ---- (0.8) группы и профили ----

/** Группа: копии приложений по uid и пакеты по имени (во всех пространствах, в том числе системные). */
data class MGroup(
    val id: String, val name: String, val members: List<Int>, val packages: List<String> = emptyList(),
    /** компоненты «пакет/класс» — отключаются отдельно, пакет целиком не трогается */
    val components: List<String> = emptyList(),
) {
    val size get() = members.size + packages.size + components.size
}

data class MWhen(
    val manual: Boolean = false, val always: Boolean = false, val saver: Boolean = false, val belowPct: Int = 0,
    val abroad: Boolean = false, val night: Boolean = false, val nightFrom: Int = 23 * 60, val nightTo: Int = 7 * 60,
    /** когда ни одно из этих Bluetooth-устройств не подключено (дольше 10 мин) */
    val btAway: List<String> = emptyList(),
) {
    fun json(): JSONObject = JSONObject().put("manual", manual).put("always", always).put("saver", saver).put("below_pct", belowPct)
        .put("abroad", abroad).put("night", night).put("night_from", nightFrom).put("night_to", nightTo).put("bt_away", JSONArray(btAway))

    companion object {
        fun parse(o: JSONObject) = MWhen(
            o.optBoolean("manual"), o.optBoolean("always"), o.optBoolean("saver"), o.optInt("below_pct"),
            o.optBoolean("abroad"), o.optBoolean("night"), o.optInt("night_from", 23 * 60), o.optInt("night_to", 7 * 60),
            o.optJSONArray("bt_away").strings(),
        )
    }
}

/** Профиль: когда включается и что делать с группами. rules: id группы или "rest" → none | watch | bg | hard. */
data class MProfile(
    val id: String, val name: String, val on: Boolean, val `when`: MWhen,
    val rules: Map<String, String>, val apps: Map<Int, String>,
    val heavyMa: Double, val heavyRxH: Int, val idleH: Int,
) {
    fun json(): JSONObject = JSONObject().put("id", id).put("name", name).put("on", on).put("when", `when`.json())
        .put("rules", JSONObject().apply { rules.forEach { (k, v) -> put(k, v) } })
        .put("apps", JSONObject().apply { apps.forEach { (k, v) -> put(k.toString(), v) } })
        .put("heavy_ma", heavyMa).put("heavy_rx_h", heavyRxH).put("idle_h", idleH)

    companion object {
        fun parse(o: JSONObject): MProfile {
            val r = o.optJSONObject("rules")
            val a = o.optJSONObject("apps")
            return MProfile(
                o.str("id"), o.str("name"), o.optBoolean("on", true), MWhen.parse(o.optJSONObject("when") ?: JSONObject()),
                r?.keys()?.asSequence()?.associateWith { r.optString(it) } ?: emptyMap(),
                a?.keys()?.asSequence()?.mapNotNull { k -> k.toIntOrNull()?.let { it to a.optString(k) } }?.toMap() ?: emptyMap(),
                o.optDouble("heavy_ma", 5.0), o.optInt("heavy_rx_h", 2000), o.optInt("idle_h", 3),
            )
        }
    }
}

/** Настройки групп и профилей (modes.json ядра). */
data class ModesCfg(val groups: List<MGroup>, val profiles: List<MProfile>, val homeCountry: String, val presets: List<String> = emptyList()) {
    fun groupsOf(uid: Int, pkgs: List<String> = emptyList()) = groups.filter { uid in it.members || pkgs.any { p -> p in it.packages } }
    fun group(id: String) = groups.firstOrNull { it.id == id }
    fun profile(id: String) = profiles.firstOrNull { it.id == id }

    fun withMember(uid: Int, gid: String, on: Boolean) = copy(
        groups = groups.map { g -> if (g.id != gid) g else g.copy(members = (g.members - uid) + if (on) listOf(uid) else emptyList()) },
    )
    fun withGroup(g: MGroup) = copy(groups = if (groups.any { it.id == g.id }) groups.map { if (it.id == g.id) g else it } else groups + g)
    fun withoutPackage(gid: String, pkg: String) = copy(groups = groups.map { g -> if (g.id != gid) g else g.copy(packages = g.packages - pkg, components = g.components - pkg) })
    fun withoutGroup(id: String) = copy(groups = groups.filter { it.id != id }, profiles = profiles.map { it.copy(rules = it.rules - id) })
    fun withProfile(p: MProfile) = copy(profiles = if (profiles.any { it.id == p.id }) profiles.map { if (it.id == p.id) p else it } else profiles + p)
    fun withoutProfile(id: String) = copy(profiles = profiles.filter { it.id != id })

    fun json(): JSONObject = JSONObject()
        .put("groups", JSONArray().apply {
            groups.forEach { g ->
                put(JSONObject().put("id", g.id).put("name", g.name).put("members", JSONArray(g.members)).put("packages", JSONArray(g.packages)).put("components", JSONArray(g.components)))
            }
        })
        .put("presets", JSONArray(presets))
        .put("profiles", JSONArray().apply { profiles.forEach { put(it.json()) } })
        .put("home_country", homeCountry)

    companion object {
        fun parse(o: JSONObject) = ModesCfg(
            o.optJSONArray("groups").list { g ->
                val m = g.optJSONArray("members")
                MGroup(
                    g.str("id"), g.str("name"), if (m == null) emptyList() else (0 until m.length()).map { m.optInt(it) },
                    g.optJSONArray("packages").strings(), g.optJSONArray("components").strings(),
                )
            },
            o.optJSONArray("profiles").list { MProfile.parse(it) },
            o.str("home_country"),
            o.optJSONArray("presets").strings(),
        )
    }
}

data class ActiveP(val id: String, val name: String, val why: List<String>, val since: Long)
data class BtDev(val name: String, val connected: Boolean)
data class ModesState(
    val active: List<ActiveP>, val saver: Boolean, val country: String, val home: String, val abroad: Boolean,
    /** сопряжённые Bluetooth-устройства по последнему чтению ядра */
    val bt: List<BtDev> = emptyList(),
)

/** Правило, которое сейчас действует на приложение: самое строгое из активных профилей. pkg — системный пакет с общим uid. */
data class Eff(val uid: Int, val level: String, val profile: String, val tripped: Boolean, val pkg: String? = null)

/** Ответ ядра на modes / modes_set / app_group / app_rule. */
data class ModesInfo(
    val cfg: ModesCfg, val state: ModesState, val effective: List<Eff>, val protected: Set<String>,
    /** что сейчас ограничено профилями: фон, отключённые пакеты, закрытая сеть */
    val claims: List<Restr>,
    val disabled: List<Restr>,
    val nonet: List<Restr>,
    val components: List<Restr>,
    /** что делали профили, новые первыми */
    val log: List<ActionRec>,
    /** пакеты участников групп и приложений с правилами */
    val packages: Map<Int, List<String>>,
    /** все приложения телефона — только если просили (installed) */
    val installed: Map<Int, List<String>>?,
    /** пакеты групп по имени → uid, где они установлены */
    val present: Map<String, List<Int>> = emptyMap(),
) {
    fun isProtected(pkgs: List<String>) = pkgs.any { it in protected || it.startsWith("com.android.providers.") }
    fun eff(uid: Int) = effective.firstOrNull { it.uid == uid }
    fun pkgsOf(uid: Int) = packages[uid] ?: installed?.get(uid).orEmpty()

    companion object {
        private fun pmap(o: JSONObject?): Map<Int, List<String>> =
            o?.keys()?.asSequence()?.mapNotNull { k -> k.toIntOrNull()?.let { it to o.optJSONArray(k).strings() } }?.toMap() ?: emptyMap()

        fun parse(o: JSONObject): ModesInfo {
            val st = o.optJSONObject("state") ?: JSONObject()
            return ModesInfo(
                ModesCfg.parse(o.optJSONObject("modes") ?: JSONObject()),
                ModesState(
                    st.optJSONArray("active").list { a -> ActiveP(a.str("id"), a.str("name"), a.optJSONArray("why").strings(), a.lng("since")) },
                    st.optBoolean("saver"), st.str("country"), st.str("home"), st.optBoolean("abroad"),
                    st.optJSONArray("bt").list { b -> BtDev(b.str("name"), b.optBoolean("connected")) },
                ),
                o.optJSONArray("effective").list { e -> Eff(e.optInt("uid"), e.str("level"), e.str("profile"), e.optBoolean("tripped"), e.strOrNull("pkg")) },
                o.optJSONArray("protected").strings().toSet(),
                o.optJSONArray("claims").list { Restr.parse(it) },
                o.optJSONArray("disabled").list { Restr.parse(it) },
                o.optJSONArray("nonet").list { Restr.parse(it) },
                o.optJSONArray("components").list { Restr.parse(it) },
                o.optJSONArray("log").list { ActionRec.parse(it) },
                pmap(o.optJSONObject("packages")),
                o.optJSONObject("installed")?.let { pmap(it) },
                o.optJSONObject("present")?.let { p -> p.keys().asSequence().associateWith { k -> p.optJSONArray(k)?.let { a -> (0 until a.length()).map { a.optInt(it) } } ?: emptyList() } } ?: emptyMap(),
            )
        }
    }
}

/** Приложение в отрезке сравнения: CPU в покое — ядро; будильники, wakelock, задания — Android за весь отрезок. */
data class CmpApp(
    val cpuMa: Double, val cpuMah: Double, val startsH: Double,
    val alarmsH: Double?, val wakelockMinH: Double?, val jobsH: Double?, val mahH: Double?,
)

data class CmpWin(
    val from: Long, val to: Long, val offMs: Long, val deepPct: Double, val pctH: Double, val ma: Double, val mah: Double,
    val wakeupsH: Double, val suspendsH: Double, val failsH: Double, val cpuMa: Double, val tempMax: Double, val app: CmpApp?,
) {
    companion object {
        fun parse(o: JSONObject) = CmpWin(
            o.lng("from"), o.lng("to"), o.lng("off_ms"), o.dbl("deep_sleep_pct"), o.dbl("pct_h"), o.dbl("ma"), o.dbl("mah"),
            o.dbl("wakeups_h"), o.dbl("suspends_h"), o.dbl("fails_h"), o.dbl("cpu_ma"), o.dbl("temp_max"),
            o.optJSONObject("app")?.let { a ->
                CmpApp(a.dbl("cpu_ma"), a.dbl("cpu_mah"), a.dbl("starts_h"), a.dblOrNull("alarms_h"), a.dblOrNull("wakelock_min_h"), a.dblOrNull("jobs_h"), a.dblOrNull("mah_h"))
            },
        )
    }
}

data class CmpRow(val uid: Int, val label: String, val kind: String, val aMa: Double, val bMa: Double)

data class Comparison(
    val ok: Boolean, val reason: String?, val mode: String, val uid: Int?, val enough: Boolean, val minOffMs: Long,
    val a: CmpWin?, val b: CmpWin?, val apps: List<CmpRow>, val periods: List<Pair<Long, Long>>, val notes: List<String>,
) {
    companion object {
        fun parse(o: JSONObject): Comparison {
            val per = o.optJSONArray("periods")
            val periods = if (per == null) emptyList() else (0 until per.length()).mapNotNull { i ->
                per.optJSONArray(i)?.let { it.optLong(0) to it.optLong(1) }
            }
            return Comparison(
                o.optBoolean("ok"), o.strOrNull("reason"), o.str("mode"), o.intOrNull("uid"), o.optBoolean("enough"), o.lng("min_off_ms"),
                o.optJSONObject("a")?.let { CmpWin.parse(it) }, o.optJSONObject("b")?.let { CmpWin.parse(it) },
                o.optJSONArray("apps").list { CmpRow(it.optInt("uid"), it.str("label"), it.str("kind"), it.dbl("a_ma"), it.dbl("b_ma")) },
                periods, o.optJSONArray("notes").strings(),
            )
        }
    }
}
