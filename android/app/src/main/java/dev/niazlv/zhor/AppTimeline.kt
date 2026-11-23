package dev.niazlv.zhor

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.automirrored.outlined.EventNote
import androidx.compose.material.icons.outlined.Timeline
import androidx.compose.material.icons.outlined.TouchApp
import androidx.compose.material.icons.outlined.ZoomIn
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Лента одного приложения и история событий Android (ядро 0.4: report с uid → timeline[].app_ma,
// history). Тот же блок истории — на экране отрезка, там по всему телефону.

// ---------------- виды событий ----------------

private val KIND_LABEL = mapOf(
    "wakelock" to "Не давал уснуть",
    "wakelock_in" to "Wakelock",
    "longwake" to "Длинный wakelock",
    "job" to "Задание",
    "sync" to "Синхронизация",
    "top" to "На экране",
    "fg" to "Передний план",
    "tmpwhitelist" to "Разрешён фон",
    "wakeupap" to "Сеть разбудила",
    "alarm" to "Будильник",
    "proc" to "Процесс работал",
    "screenwake" to "Включил экран",
    "running" to "Не спал",
    "screen" to "Экран",
    "doze" to "Doze",
    "gps" to "GPS",
    "mobile_radio" to "Радио",
    "wifi_scan" to "Поиск Wi-Fi",
    "ble_scan" to "Поиск BLE",
    "audio" to "Звук",
    "video" to "Видео",
    "camera" to "Камера",
    "wake_reason" to "Пробуждение ядра",
)

fun histKindLabel(k: String) = KIND_LABEL[k] ?: k

fun histKindColor(k: String): Color = when (k) {
    "top", "screen", "screenwake" -> Pal.screen
    "fg" -> Pal.fgs
    "job" -> Pal.cpu
    "sync", "wifi_scan" -> Pal.wifi
    "wakelock", "wakelock_in", "longwake", "ble_scan" -> Pal.bt
    "tmpwhitelist" -> Pal.sensors
    "wakeupap", "mobile_radio" -> Pal.radio
    "alarm", "video" -> Pal.video
    "proc" -> Pal.cpuIdle
    "running" -> Pal.off
    "doze" -> Pal.plug
    "gps" -> Pal.gnss
    "audio" -> Pal.audio
    "camera" -> Pal.camera
    "wake_reason" -> Pal.cpu
    else -> Pal.other
}

/** Тег Android → по-человечески. Сырое имя остаётся второй строкой. */
fun histPretty(kind: String, raw: String): String {
    var s = raw
    if (s.isEmpty()) return "—"
    if (kind == "tmpwhitelist") {
        // «e6f929f com.google.android.c2dm.intent.RECEIVE/u0/0x2051» → действие
        s = s.replace(Regex("^[0-9a-f]{5,8} "), "").replace(Regex("/u\\d+/0x[0-9a-fA-F]+$"), "")
        return when {
            s == "com.google.android.c2dm.intent.RECEIVE" -> "пуш Firebase (FCM)"
            s.contains("PING_TIMER") -> "пинг пуш-службы ($s)"
            s == "tile onclick" -> "нажатие плитки в шторке"
            else -> s
        }
    }
    fun work(x: String): String? =
        if (x.startsWith("#") && x.contains("#@androidx.work")) x.removePrefix("#").substringBefore('#') + " · WorkManager" else null
    fun comp(x: String): String {
        work(x)?.let { return it }
        val slash = x.indexOf('/')
        if (slash <= 0) return x
        val pkg = x.substring(0, slash)
        val cls = x.substring(slash + 1).substringAfterLast('.')
        return "$pkg · $cls"
    }
    return when {
        s.startsWith("*job*") -> "задание " + comp(s.substringAfter('/'))
        s.startsWith("*walarm*:") -> "будильник " + s.removePrefix("*walarm*:")
        s == "*alarm*" -> "будильник"
        s.startsWith("NotificationManagerService:post:") -> "показ уведомления (${s.removePrefix("NotificationManagerService:post:")})"
        s.startsWith("Broadcast to ") -> "приём широковещательного: " + s.removePrefix("Broadcast to ").substringBefore(' ').substringAfterLast('.')
        kind == "job" || kind == "sync" -> comp(s)
        else -> work(s) ?: s
    }
}

// ---------------- общие куски ----------------

/** Дорожка: спаны событий на общей оси времени; sel — подсветка выбранной точки. */
@Composable
private fun LaneStrip(l: HLane, from: Long, to: Long, sel: Pair<Long, Long>?) {
    val bg = MaterialTheme.colorScheme.surfaceContainerHighest
    val selC = MaterialTheme.colorScheme.primary
    val c = histKindColor(l.kind).copy(alpha = if (l.global) 0.6f else 0.95f)
    Canvas(Modifier.fillMaxWidth().height(9.dp).clip(RoundedCornerShape(3.dp))) {
        val w = size.width
        val h = size.height
        val span = (to - from).coerceAtLeast(1).toFloat()
        fun x(t: Long) = ((t - from) / span * w).coerceIn(0f, w)
        drawRect(bg)
        for (s in l.spans) {
            val x0 = x(s[0])
            val x1 = maxOf(x(s[1]), x0 + 2f)
            drawRect(c, Offset(x0, 0f), Size(x1 - x0, h))
        }
        sel?.let { (a, b) ->
            val x0 = x(a)
            val x1 = maxOf(x(b), x0 + 2f)
            drawRect(selC.copy(alpha = 0.22f), Offset(x0, 0f), Size(x1 - x0, h))
            drawLine(selC, Offset(x0, 0f), Offset(x0, h), 1.5f)
            drawLine(selC, Offset(x1, 0f), Offset(x1, h), 1.5f)
        }
    }
}

@Composable
private fun LaneRow(l: HLane, from: Long, to: Long, sel: Pair<Long, Long>?) {
    Column(Modifier.padding(vertical = 3.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(8.dp).clip(RoundedCornerShape(2.dp)).background(histKindColor(l.kind)))
            Spacer(Modifier.width(6.dp))
            Text(l.label, Modifier.weight(1f), style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                listOfNotNull("${Fmt.count(l.count)}×", if (l.ms > 0) Fmt.dur(l.ms) else null).joinToString(" · "),
                style = MaterialTheme.typography.labelMedium.merge(NumStyle), color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.height(3.dp))
        LaneStrip(l, from, to, sel)
    }
}

/** Дорожки событий: своё приложение сверху, телефон целиком — отдельной группой. */
@Composable
fun HistoryLanes(h: History, from: Long, to: Long, sel: Pair<Long, Long>? = null) {
    val own = h.lanes.filter { !it.global }
    val phone = h.lanes.filter { it.global }
    own.forEach { LaneRow(it, from, to, sel) }
    if (phone.isNotEmpty()) {
        if (own.isNotEmpty()) Text(
            "Телефон в это время", Modifier.padding(top = 10.dp, bottom = 2.dp),
            style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        phone.forEach { LaneRow(it, from, to, sel) }
    }
    TimeAxis(from, to)
}

@Composable
private fun TimeAxis(from: Long, to: Long) {
    Row(Modifier.fillMaxWidth().padding(top = 4.dp)) {
        val ls = MaterialTheme.typography.labelSmall
        val c = MaterialTheme.colorScheme.onSurfaceVariant
        Text(if (Fmt.sameDay(from, to)) Fmt.clock(from) else Fmt.time(from), style = ls, color = c)
        Spacer(Modifier.weight(1f))
        Text(Fmt.clock((from + to) / 2), style = ls, color = c)
        Spacer(Modifier.weight(1f))
        Text(Fmt.clock(to), style = ls, color = c)
    }
}

@Composable
fun HistItemRow(
    i: HItem, max: Long, showApp: Boolean, onApp: ((Int) -> Unit)?,
    appLabel: (Int, String) -> String = { _, f -> f }, winFrom: Long = 0,
) {
    val pretty = histPretty(i.kind, i.name)
    val click = if (showApp && i.uid != null && onApp != null) Modifier.clickable { onApp(i.uid) } else Modifier
    Column(click.fillMaxWidth().padding(vertical = 5.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Pill(histKindLabel(i.kind), histKindColor(i.kind))
            if (showApp && i.app != null) {
                Spacer(Modifier.width(6.dp))
                Text(if (i.uid != null) appLabel(i.uid, i.app) else i.app, Modifier.weight(1f), style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            } else {
                Spacer(Modifier.weight(1f))
            }
            Text(
                listOfNotNull("${Fmt.count(i.count)}×", if (i.ms > 0) Fmt.dur(i.ms) else null).joinToString(" · "),
                style = MaterialTheme.typography.bodyMedium.merge(NumStyle), fontWeight = FontWeight.SemiBold,
            )
        }
        Text(pretty, Modifier.padding(top = 2.dp), style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
        if (pretty != i.name && i.name.isNotEmpty()) Text(
            i.name, fontFamily = FontFamily.Monospace, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        val sub = buildList {
            if (i.maxMs > 0 && i.count > 1) add("дольше всего ${Fmt.dur(i.maxMs)}")
            if (i.last > 0) add(if (i.last < winFrom) "началось до отрезка, в ${Fmt.clockSec(i.last)}" else "последний раз ${Fmt.clockSec(i.last)}")
        }
        if (sub.isNotEmpty()) Text(sub.joinToString(" · "), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (max > 0) {
            Spacer(Modifier.height(3.dp))
            ShareBar((if (i.ms > 0) i.ms else i.count).toFloat() / max, histKindColor(i.kind), height = 4.dp)
        }
    }
}

@Composable
fun HistEventRow(e: HEvent, showApp: Boolean, appLabel: (Int, String) -> String = { _, f -> f }) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.Top) {
        Text(Fmt.clockSec(e.t0), Modifier.width(66.dp).padding(top = 1.dp), style = MaterialTheme.typography.labelMedium.merge(NumStyle))
        Box(Modifier.padding(top = 5.dp).size(8.dp).clip(CircleShape).background(histKindColor(e.kind)))
        Spacer(Modifier.width(8.dp))
        Column(Modifier.weight(1f)) {
            Text(
                listOfNotNull(histKindLabel(e.kind), if (showApp) e.app?.let { a -> e.uid?.let { appLabel(it, a) } ?: a } else null).joinToString(" · "),
                style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
            Text(histPretty(e.kind, e.name), style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        Spacer(Modifier.width(6.dp))
        Text(
            when {
                e.open -> "идёт"
                e.dur > 0 -> Fmt.dur(e.dur)
                else -> ""
            },
            style = MaterialTheme.typography.bodySmall.merge(NumStyle), fontWeight = FontWeight.SemiBold,
        )
    }
}

@Composable
private fun HistoryLists(
    h: History, showApp: Boolean, key: String, onApp: ((Int) -> Unit)?, events: List<HEvent>, eventsTitle: String,
    appLabel: (Int, String) -> String = { _, f -> f },
) {
    if (h.items.isNotEmpty()) {
        Text("Чаще и дольше всего", Modifier.padding(top = 12.dp), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
        val max = h.items.maxOf { if (it.ms > 0) it.ms else it.count }
        MoreList(h.items, initial = 6, key = "hi$key") { HistItemRow(it, max, showApp, onApp, appLabel, h.from) }
    }
    if (events.isNotEmpty()) {
        Text(eventsTitle, Modifier.padding(top = 12.dp), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
        if (h.eventsTotal > h.events.size) Text(
            "Показаны ${h.events.size} самых длинных из ${Fmt.count(h.eventsTotal.toLong())}. Чтобы увидеть все, откройте отрезок покороче.",
            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        MoreList(events, initial = 8, key = "he$key${events.size}") { HistEventRow(it, showApp, appLabel) }
    }
    h.notes.forEach { Explain(it) }
}

// ---------------- лента приложения ----------------

private fun niceSmall(v: Double): Double {
    val steps = listOf(0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 15.0, 20.0, 30.0, 50.0)
    return steps.firstOrNull { it >= v } ?: niceMax(v)
}

private fun nearest(points: List<Point>, t: Long): Int? =
    points.indices.minByOrNull { i ->
        val p = points[i]
        when {
            t < p.t0 -> p.t0 - t
            t > p.t1 -> t - p.t1
            else -> 0L
        }
    }

/** Столбики: CPU приложения в каждой точке ленты; фон — когда был включён экран. */
@Composable
private fun AppChart(points: List<Point>, from: Long, to: Long, selT: Long?, onSel: (Long?) -> Unit) {
    val grid = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)
    val labelC = MaterialTheme.colorScheme.onSurfaceVariant
    val cursorC = MaterialTheme.colorScheme.primary
    val gapC = MaterialTheme.colorScheme.surfaceContainerHighest
    val tm = rememberTextMeasurer()
    val haptic = LocalHapticFeedback.current
    val cb by rememberUpdatedState(onSel)
    val maxMa = niceSmall(((points.maxOfOrNull { it.appMa ?: 0.0 } ?: 0.0) * 1.1).coerceAtLeast(0.4))
    val grow = animatedFraction(1f)
    val selIdx = selT?.let { t -> points.indexOfFirst { it.t0 == t }.takeIf { it >= 0 } }
    val span = (to - from).coerceAtLeast(1)
    var lastHaptic by remember { mutableStateOf(-1) }
    // касание «прилипает» к самому высокому столбику рядом (столбик минуты на ленте за часы — 1–2 px);
    // протяжка пальцем — точная
    fun pick(x: Float, w: Float, slop: Float = 0f) {
        val i = if (slop > 0f) {
            val near = points.indices.filter { j ->
                val p = points[j]
                val x0 = (p.t0 - from).toFloat() / span * w
                val x1 = (p.t1 - from).toFloat() / span * w
                x1 >= x - slop && x0 <= x + slop
            }
            near.maxByOrNull { points[it].appMa ?: 0.0 }?.takeIf { (points[it].appMa ?: 0.0) > 0.0 } ?: nearest(points, from + (x / w * span).toLong())
        } else {
            nearest(points, from + (x / w * span).toLong())
        } ?: return
        if (i != lastHaptic) {
            haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
            lastHaptic = i
        }
        cb(points[i].t0)
    }
    Canvas(
        Modifier.fillMaxWidth().height(130.dp)
            .pointerInput(points, from, to) { detectTapGestures { o -> pick(o.x, size.width.toFloat(), 8.dp.toPx()) } }
            .pointerInput(points, from, to) {
                detectHorizontalDragGestures(
                    onDragStart = { o -> pick(o.x, size.width.toFloat()) },
                    onHorizontalDrag = { ch, _ -> ch.consume(); pick(ch.position.x, size.width.toFloat()) },
                )
            },
    ) {
        val w = size.width
        val h = size.height
        fun x(t: Long) = ((t - from).toFloat() / span * w).coerceIn(0f, w)
        // экран вкл. — фоном; без записи — серым
        var prev = from
        for (p in points) {
            if (p.t0 - prev > 5 * 60_000L) drawRect(gapC, Offset(x(prev), 0f), Size(x(p.t0) - x(prev), h))
            prev = maxOf(prev, p.t1)
            if (p.screen) drawRect(Pal.screen.copy(alpha = 0.10f), Offset(x(p.t0), 0f), Size(maxOf(x(p.t1) - x(p.t0), 1f), h))
        }
        val ls = TextStyle(fontSize = 10.sp, color = labelC)
        for (i in 1..2) {
            val gy = h - h * i / 2
            drawLine(grid, Offset(0f, gy), Offset(w, gy), 1f)
            val v = maxMa * i / 2
            drawText(tm, "${if (v < 10) Fmt.d1(v) else Fmt.ma(v)} мА", Offset(4f, gy + 2f), ls)
        }
        drawLine(grid, Offset(0f, h - 0.5f), Offset(w, h - 0.5f), 1.5f)
        points.forEachIndexed { i, p ->
            val v = p.appMa ?: return@forEachIndexed
            val x0 = x(p.t0)
            val x1 = maxOf(x(p.t1), x0 + 1.5f)
            val dim = selIdx != null && i != selIdx
            val bh = (v / maxMa * h).toFloat().coerceIn(0f, h) * grow
            drawRect(Pal.cpu.copy(alpha = if (dim) 0.35f else 0.9f), Offset(x0, h - bh), Size((x1 - x0 - 0.6f).coerceAtLeast(1f), bh))
        }
        if (selIdx != null) {
            val p = points[selIdx]
            val xm = (x(p.t0) + x(p.t1)) / 2
            drawLine(cursorC, Offset(xm, 0f), Offset(xm, h), 2.dp.toPx())
        }
    }
}

/** Секция карточки приложения «По времени»: свой отчёт с uid (лента + история событий). */
@Composable
fun AppTimelineSection(vm: Vm, navi: Navi, r: Report, a: App) {
    LaunchedEffect(a.uid, r.from, r.to) { vm.loadAppReport(a.uid, r.from, r.to) }
    val st = vm.appReps[vm.appKey(a.uid, r.from, r.to)] ?: Res.Loading
    when (st) {
        is Res.Ok -> {
            val ar = st.v
            val pts = ar.timeline.filter { it.appMa != null }
            // ядро старше 0.4: ни ленты приложения, ни истории — секцию не показываем
            if (pts.isEmpty() && ar.history == null) return
            AppTimelineBody(vm, navi, ar, a, pts)
        }
        is Res.Err -> Section("По времени", icon = Icons.Outlined.Timeline, help = "app_timeline") {
            Text("Лента приложения не построена: ${st.msg}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            TextButton(onClick = { vm.loadAppReport(a.uid, r.from, r.to, force = true) }) { Text("Повторить") }
        }
        else -> Section("По времени", sub = "собираю ленту и события Android…", icon = Icons.Outlined.Timeline, help = "app_timeline") {
            LinearProgressIndicator(Modifier.fillMaxWidth())
        }
    }
}

@Composable
private fun AppTimelineBody(vm: Vm, navi: Navi, ar: Report, a: App, pts: List<Point>) {
    val h = ar.history
    var selT by rememberSaveable(ar.from, ar.to, a.uid) { mutableStateOf<Long?>(null) }
    val sel = selT?.let { t -> pts.firstOrNull { it.t0 == t } }
    Section(
        "По времени", sub = "когда тратило и что делало — по истории Android",
        icon = Icons.Outlined.Timeline, help = "app_timeline",
    ) {
        if (pts.isNotEmpty()) {
            AppChart(pts, ar.from, ar.to, selT) { selT = it }
            TimeAxis(ar.from, ar.to)
            Spacer(Modifier.height(4.dp))
            Legend(listOf(Pal.cpu to "процессор приложения", Pal.screen.copy(alpha = 0.35f) to "экран включён", MaterialTheme.colorScheme.surfaceContainerHighest to "нет записи"))
        }
        AnimatedVisibility(sel == null && pts.isNotEmpty(), enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Row(Modifier.padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Outlined.TouchApp, null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(6.dp))
                Text(
                    "Коснитесь всплеска — покажу, что приложение делало в эту минуту.",
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        AnimatedVisibility(sel != null, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            val p = sel ?: return@AnimatedVisibility
            PointCard(vm, navi, a, p, h) { selT = null }
        }
        if (h != null && h.lanes.isNotEmpty()) {
            Spacer(Modifier.height(10.dp))
            HistoryLanes(h, ar.from, ar.to, sel?.let { it.t0 to it.t1 })
            if (h.lanes.any { it.kind == "wakelock" }) Explain(
                "«Не давал уснуть» — отрезки, когда телефон держали wakelock'и и это приложение взяло его первым. " +
                    "Сколько держало само приложение — в списке «Wakelock» ниже (по статистике Android).",
            )
        }
        if (h != null) {
            HistoryLists(h, showApp = false, key = "a${a.uid}${ar.from}", onApp = null, events = h.events.reversed(), eventsTitle = "События, последние сверху")
        } else {
            Explain("История событий Android ещё не собрана: она появится после ближайшего снимка batterystats (раз в час или по «Обновить»).")
        }
    }
}

@Composable
private fun PointCard(vm: Vm, navi: Navi, a: App, p: Point, h: History?, onClose: () -> Unit) {
    val inPoint = h?.events?.filter { it.t1 >= p.t0 && it.t0 <= p.t1 }?.sortedBy { it.t0 }.orEmpty()
    Column(
        Modifier.padding(top = 10.dp).fillMaxWidth().clip(RoundedCornerShape(16.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHigh).padding(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("${Fmt.clockSec(p.t0)} – ${Fmt.clockSec(p.t1)}", style = MaterialTheme.typography.titleSmall.merge(NumStyle), fontWeight = FontWeight.SemiBold)
                Text(
                    listOf(if (p.screen) "экран вкл." else "экран выкл.", if (p.plugged) "на зарядке" else null).filterNotNull().joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            IconButton(onClick = onClose) { Icon(Icons.Outlined.Close, "закрыть") }
        }
        KV("Это приложение (процессор)", "${Fmt.d2(p.appMa ?: 0.0)} мА", help = "ma")
        if (!p.plugged) KV("Весь телефон (замер)", "${Fmt.ma(p.measMa)} мА")
        p.appTop?.let { KV("Больше всего в процессе", it) }
        if (inPoint.isNotEmpty()) {
            Text("События Android в эту минуту", Modifier.padding(top = 8.dp), style = MaterialTheme.typography.labelLarge)
            MoreList(inPoint, initial = 6, key = "pc${p.t0}") { HistEventRow(it, showApp = false) }
        } else if (h != null) {
            Explain("В истории Android на этом отрезке у приложения событий нет: тратило процессор само (без заданий, синхронизаций и wakelock).")
        }
        FilledTonalButton(onClick = {
            vm.loadSegment(p.t0, p.t1)
            navi.app(a.uid, vm.segKey(p.t0, p.t1))
        }, Modifier.fillMaxWidth().padding(top = 8.dp)) {
            Icon(Icons.Outlined.ZoomIn, null, Modifier.size(18.dp))
            Spacer(Modifier.width(6.dp))
            Text("Разобрать этот отрезок подробно")
        }
    }
}

// ---------------- весь телефон: экран отрезка ----------------

/** «Что происходило» на отрезке: дорожки телефона, кто чаще всего, отдельные события. */
@Composable
fun PhoneHistorySection(vm: Vm, navi: Navi, r: Report, h: History, seg: String) {
    Section(
        "Что происходило", sub = "история событий Android на этом отрезке",
        icon = Icons.AutoMirrored.Outlined.EventNote, help = "android_history",
    ) {
        if (h.lanes.isNotEmpty()) HistoryLanes(h, r.from, r.to)
        HistoryLists(
            h, showApp = true, key = "s$seg", onApp = { uid -> if (r.app(uid) != null) navi.app(uid, seg) },
            events = h.events.reversed(), eventsTitle = "События, последние сверху",
            appLabel = { uid, fb -> vm.labelOfUid(uid, fb, seg) },
        )
        if (h.events.isEmpty() && h.items.isNotEmpty()) Explain("Отдельные события показываются для отрезков до 3 часов.")
    }
}
