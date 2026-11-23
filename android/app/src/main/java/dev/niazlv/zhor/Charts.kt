package dev.niazlv.zhor

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.TouchApp
import androidx.compose.material.icons.outlined.ZoomIn
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private fun p95(v: List<Double>): Double {
    if (v.isEmpty()) return 0.0
    val s = v.sorted()
    return s[((s.size - 1) * 0.95).toInt()]
}

/** Круглая верхняя граница шкалы. */
fun niceMax(v: Double): Double {
    val steps = listOf(50.0, 100.0, 150.0, 200.0, 300.0, 400.0, 500.0, 600.0, 800.0, 1000.0, 1200.0, 1500.0, 2000.0, 2500.0, 3000.0, 4000.0, 5000.0, 8000.0)
    return steps.firstOrNull { it >= v } ?: (Math.ceil(v / 1000) * 1000)
}

private const val GAP_MS = 5 * 60_000L

// ---------------- лента отчёта ----------------

/** Индекс точки под временем t (внутри отрезка или ближайшей). */
private fun indexAt(points: List<Point>, t: Long): Int? {
    if (points.isEmpty()) return null
    var lo = 0
    var hi = points.size - 1
    while (lo < hi) {
        val mid = (lo + hi + 1) / 2
        if (points[mid].t0 <= t) lo = mid else hi = mid - 1
    }
    val cand = listOfNotNull(lo, (lo + 1).takeIf { it < points.size })
    return cand.minByOrNull { i ->
        val p = points[i]
        when {
            t < p.t0 -> p.t0 - t
            t > p.t1 -> t - p.t1
            else -> 0L
        }
    }
}

@Composable
fun TimelineChart(
    points: List<Point>, from: Long, to: Long, selT: Long?, onSel: (Long?) -> Unit,
    modifier: Modifier = Modifier, height: Dp = 170.dp,
) {
    val grid = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)
    val labelC = MaterialTheme.colorScheme.onSurfaceVariant
    val modelC = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.75f)
    val cursorC = MaterialTheme.colorScheme.primary
    val gapC = MaterialTheme.colorScheme.surfaceContainerHighest
    val tm = rememberTextMeasurer()
    val haptic = LocalHapticFeedback.current
    val cb by rememberUpdatedState(onSel)
    val discharge = points.filter { !it.plugged }
    val maxMa = niceMax((maxOf(p95(discharge.map { it.measMa }), p95(discharge.map { it.modelMa })) * 1.1).coerceAtLeast(40.0))
    val grow = animatedFraction(1f)
    val selIdx = selT?.let { t -> points.indexOfFirst { it.t0 == t }.takeIf { it >= 0 } }
    val span = (to - from).coerceAtLeast(1)
    var lastHaptic by remember { mutableStateOf(-1) }

    fun pick(x: Float, w: Float) {
        val t = from + (x / w * span).toLong()
        val i = indexAt(points, t) ?: return
        if (i != lastHaptic) {
            haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
            lastHaptic = i
        }
        cb(points[i].t0)
    }

    Canvas(
        modifier.fillMaxWidth().height(height)
            .pointerInput(points, from, to) { detectTapGestures { o -> pick(o.x, size.width.toFloat()) } }
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
        fun y(ma: Double) = h - (ma / maxMa * h).toFloat().coerceIn(0f, h)

        // пробелы без записи
        var prev = from
        for (p in points) {
            if (p.t0 - prev > GAP_MS) drawRect(gapC, Offset(x(prev), 0f), Size(x(p.t0) - x(prev), h))
            prev = maxOf(prev, p.t1)
        }
        if (to - prev > GAP_MS) drawRect(gapC, Offset(x(prev), 0f), Size(x(to) - x(prev), h))

        // сетка и подписи
        val ls = TextStyle(fontSize = 10.sp, color = labelC)
        for (i in 1..4) {
            val gy = h - h * i / 4
            drawLine(grid, Offset(0f, gy), Offset(w, gy), 1f)
            if (i % 2 == 0) drawText(tm, "${Fmt.ma(maxMa * i / 4)} мА", Offset(4f, gy + 2f), ls)
        }
        drawLine(grid, Offset(0f, h - 0.5f), Offset(w, h - 0.5f), 1.5f)

        // столбики замера
        points.forEachIndexed { i, p ->
            val x0 = x(p.t0)
            val x1 = maxOf(x(p.t1), x0 + 1.5f)
            val dim = selIdx != null && i != selIdx
            if (p.plugged) {
                drawRect(Pal.plug.copy(alpha = if (dim) 0.08f else 0.2f), Offset(x0, 0f), Size(x1 - x0, h))
            } else {
                val c = (if (p.screen) Pal.screen else Pal.off).copy(alpha = if (dim) 0.35f else 0.9f)
                val bh = (h - y(p.measMa)) * grow
                drawRect(c, Offset(x0, h - bh), Size((x1 - x0 - 0.6f).coerceAtLeast(1f), bh))
            }
        }

        // линия модели (рвётся на пробелах)
        val path = Path()
        var last: Point? = null
        for (p in points) {
            if (p.plugged) {
                last = null
                continue
            }
            val xm = (x(p.t0) + x(p.t1)) / 2
            val ym = h - (h - y(p.modelMa)) * grow
            if (last == null || p.t0 - last.t1 > GAP_MS) path.moveTo(xm, ym) else path.lineTo(xm, ym)
            last = p
        }
        drawPath(path, modelC, style = Stroke(width = 1.8.dp.toPx(), cap = StrokeCap.Round))

        // курсор
        if (selIdx != null) {
            val p = points[selIdx]
            val xm = (x(p.t0) + x(p.t1)) / 2
            drawLine(cursorC, Offset(xm, 0f), Offset(xm, h), 2.dp.toPx())
            if (!p.plugged) {
                drawCircle(cursorC, 5.dp.toPx(), Offset(xm, y(p.measMa)))
                drawCircle(modelC, 3.5.dp.toPx(), Offset(xm, y(p.modelMa)))
            }
        }
    }
}

/** Лента + инспектор точки + «Открыть этот отрезок». */
@Composable
fun TimelineBlock(r: Report, vm: Vm, seg: String?, onOpenSeg: (Long, Long) -> Unit, onOpenApp: (Int) -> Unit) {
    val points = r.timeline
    if (points.isEmpty()) {
        EmptyState(Icons.Outlined.TouchApp, "Ленты пока нет", "Ядро ещё не записало ни одного отрезка в этом периоде. Обычно первая точка появляется через минуту после запуска.")
        return
    }
    var selT by rememberSaveable(r.from, r.to) { mutableStateOf<Long?>(null) }
    val sel = selT?.let { t -> points.indexOfFirst { it.t0 == t }.takeIf { it >= 0 } }
    TimelineChart(points, r.from, r.to, selT, { selT = it })
    Row(Modifier.fillMaxWidth().padding(top = 4.dp)) {
        val ls = MaterialTheme.typography.labelSmall
        val c = MaterialTheme.colorScheme.onSurfaceVariant
        Text(if (Fmt.sameDay(r.from, r.to)) Fmt.clock(r.from) else Fmt.time(r.from), style = ls, color = c)
        Spacer(Modifier.weight(1f))
        Text(Fmt.clock((r.from + r.to) / 2), style = ls, color = c)
        Spacer(Modifier.weight(1f))
        Text(Fmt.clock(r.to), style = ls, color = c)
    }
    Spacer(Modifier.height(6.dp))
    Legend(
        listOf(
            Pal.screen to "замер, экран вкл.", Pal.off to "замер, экран выкл.",
            Pal.plug.copy(alpha = 0.5f) to "зарядка", MaterialTheme.colorScheme.onSurface.copy(alpha = 0.75f) to "модель",
            MaterialTheme.colorScheme.surfaceContainerHighest to "нет записи",
        ),
    )
    AnimatedVisibility(sel == null, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
        Row(Modifier.padding(top = 10.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.TouchApp, null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
            Spacer(Modifier.width(6.dp))
            Text(
                "Коснитесь графика или проведите по нему пальцем — покажу ток, модель и кто тратил в этой точке.",
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
    AnimatedVisibility(sel != null, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
        val i = sel ?: return@AnimatedVisibility
        PointInspector(r, points, i, vm, seg, onClose = { selT = null }, onOpenSeg = onOpenSeg, onOpenApp = onOpenApp)
    }
}

private fun same(a: Point, b: Point) = a.screen == b.screen && a.plugged == b.plugged

/** Непрерывный участок того же состояния (экран/зарядка) вокруг точки. */
private fun runOf(points: List<Point>, i: Int): IntRange {
    var a = i
    while (a > 0 && same(points[a - 1], points[i]) && points[a].t0 - points[a - 1].t1 < GAP_MS) a--
    var b = i
    while (b < points.size - 1 && same(points[b + 1], points[i]) && points[b + 1].t0 - points[b].t1 < GAP_MS) b++
    return a..b
}

private fun stateName(p: Point) = when {
    p.plugged -> "на зарядке"
    p.screen -> "экран вкл."
    else -> "экран выкл."
}

@Composable
private fun PointInspector(
    r: Report, points: List<Point>, i: Int, vm: Vm, seg: String?, onClose: () -> Unit,
    onOpenSeg: (Long, Long) -> Unit, onOpenApp: (Int) -> Unit,
) {
    val p = points[i]
    val run = runOf(points, i)
    Column(
        Modifier.padding(top = 12.dp).fillMaxWidth().clip(RoundedCornerShape(18.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHighest).padding(14.dp).animateContentSize(),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("${Fmt.clockSec(p.t0)} – ${Fmt.clockSec(p.t1)}", style = MaterialTheme.typography.titleSmall.merge(NumStyle), fontWeight = FontWeight.SemiBold)
                Text(
                    "${Fmt.date(p.t0)} · ${Fmt.dur(p.t1 - p.t0)} · заряд ${p.level} %",
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Pill(stateName(p), if (p.plugged) Pal.plug else if (p.screen) Pal.screen else Pal.off)
            IconButton(onClick = onClose) { Icon(Icons.Outlined.Close, "закрыть") }
        }
        Spacer(Modifier.height(8.dp))
        if (p.plugged) {
            Text(
                "Телефон заряжался: счётчик показывает приток заряда, а не расход. Модель и потребители ниже — что работало во время зарядки.",
                style = MaterialTheme.typography.bodySmall,
            )
            Spacer(Modifier.height(8.dp))
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            val tb = MaterialTheme.colorScheme.surfaceContainerLow
            MetricTile("Замер", if (p.plugged) "—" else Fmt.ma(p.measMa), Modifier.weight(1f), unit = "мА", sub = "факт по батарее", bg = tb)
            MetricTile("Модель", Fmt.ma(p.modelMa), Modifier.weight(1f), unit = "мА", sub = "из них CPU\u00A0${Fmt.ma(p.cpuMa)}", bg = tb)
            val asleep = p.suspendPct ?: (100 - p.awakePct).coerceIn(0.0, 100.0)
            MetricTile(
                "Спал", Fmt.pctRaw(asleep), Modifier.weight(1f), unit = "%",
                sub = "не спал ${Fmt.pctRaw(p.awakePct)}\u00A0%", bg = MaterialTheme.colorScheme.surfaceContainerLow,
                lvl = if (p.screen || p.plugged) Lvl.NONE else Norms.deepSleep(asleep, Norms.MIN_OFF_MS),
            )
        }
        Spacer(Modifier.height(8.dp))
        val notes = buildList {
            if (!p.plugged) {
                add("Отрезок стоил ≈ ${Fmt.mah(p.mah)} мА·ч.")
                val diff = p.measMa - p.modelMa
                if (diff > 30 && p.measMa > p.modelMa * 1.3) add("Замер выше модели на ${Fmt.ma(diff)} мА — это то, что модель не видит: радио, GPS, Wi-Fi, GPU, периферия.")
                else if (p.modelMa > p.measMa * 1.3 && p.modelMa - p.measMa > 30) add("Модель выше замера: коэффициенты производителя завышены, но кто тратил — показано верно.")
            }
            if (!p.screen && !p.plugged && p.awakePct > 50) add("Экран выключен, но процессор бодрствовал ${Fmt.pctRaw(p.awakePct)} % времени — кто-то не давал уснуть.")
        }
        notes.forEach { Explain(it) }
        val tops = p.tops
        if (tops.isNotEmpty()) {
            Spacer(Modifier.height(10.dp))
            Text("Кто тратил в этой точке", style = MaterialTheme.typography.labelLarge)
            val max = tops.maxOf { it.ma }.takeIf { it > 0 } ?: 1.0
            tops.forEach { t ->
                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable { onOpenApp(t.uid) }.padding(vertical = 5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    UidIcon(vm, t.uid, t.label, 26.dp)
                    Spacer(Modifier.width(8.dp))
                    Column(Modifier.weight(1f)) {
                        Row {
                            Text(vm.labelOfUid(t.uid, t.label, seg), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium)
                            Text("${Fmt.ma(t.ma)} мА", style = MaterialTheme.typography.bodyMedium.merge(NumStyle), fontWeight = FontWeight.SemiBold)
                        }
                        ShareBar((t.ma / max).toFloat(), Pal.cpu, height = 4.dp)
                    }
                }
            }
        } else if (p.top.isNotBlank()) {
            Spacer(Modifier.height(8.dp))
            Text("Больше всех процессор тратил: ${p.top}", style = MaterialTheme.typography.bodyMedium)
            Text("Подробный топ точки появится с ядром 0.3; полную разбивку даст «Открыть этот отрезок».", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.height(12.dp))
        FilledTonalButton(onClick = { onOpenSeg(p.t0, p.t1) }, Modifier.fillMaxWidth()) {
            Icon(Icons.Outlined.ZoomIn, null, Modifier.size(18.dp))
            Spacer(Modifier.width(6.dp))
            Text("Открыть этот отрезок")
        }
        if (run.last > run.first) {
            val a = points[run.first]
            val b = points[run.last]
            OutlinedButton(onClick = { onOpenSeg(a.t0, b.t1) }, Modifier.fillMaxWidth()) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Весь период «${stateName(p)}»")
                    Text("${Fmt.span(a.t0, b.t1)} · ${Fmt.dur(b.t1 - a.t0)}", style = MaterialTheme.typography.labelSmall)
                }
            }
        }
    }
}

// ---------------- живой график ----------------

@Composable
fun LiveChart(ticks: List<LiveTick>, selT: Long?, onSel: (Long?) -> Unit, modifier: Modifier = Modifier) {
    if (ticks.size < 2) return
    val drawC = MaterialTheme.colorScheme.primary
    val modelC = Pal.cpu
    val grid = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)
    val labelC = MaterialTheme.colorScheme.onSurfaceVariant
    val offC = Pal.off.copy(alpha = 0.12f)
    val tm = rememberTextMeasurer()
    val haptic = LocalHapticFeedback.current
    val cb by rememberUpdatedState(onSel)
    val tk by rememberUpdatedState(ticks)
    val maxMa = niceMax(ticks.maxOf { maxOf(it.drawMa, it.modelMa) } * 1.1)
    val t0 = ticks.first().t
    val t1 = ticks.last().t
    val span = (t1 - t0).coerceAtLeast(1).toFloat()
    var lastHaptic by remember { mutableStateOf(-1L) }

    fun pick(x: Float, w: Float) {
        val l = tk
        if (l.isEmpty()) return
        val a = l.first().t
        val s = (l.last().t - a).coerceAtLeast(1)
        val t = a + (x / w * s).toLong()
        val best = l.minBy { kotlin.math.abs(it.t - t) }
        if (best.t != lastHaptic) {
            haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
            lastHaptic = best.t
        }
        cb(best.t)
    }

    Canvas(
        modifier.fillMaxWidth().height(150.dp)
            .pointerInput(Unit) { detectTapGestures { o -> pick(o.x, size.width.toFloat()) } }
            .pointerInput(Unit) {
                detectHorizontalDragGestures(
                    onDragStart = { o -> pick(o.x, size.width.toFloat()) },
                    onHorizontalDrag = { ch, _ -> ch.consume(); pick(ch.position.x, size.width.toFloat()) },
                )
            },
    ) {
        val w = size.width
        val h = size.height
        fun x(t: Long) = (t - t0) / span * w
        fun y(v: Double) = h - (v.coerceAtLeast(0.0) / maxMa * h).toFloat().coerceIn(0f, h)
        // экран выключен — фон
        for (i in 1 until ticks.size) if (!ticks[i].screen) drawRect(offC, Offset(x(ticks[i - 1].t), 0f), Size(x(ticks[i].t) - x(ticks[i - 1].t), h))
        val ls = TextStyle(fontSize = 10.sp, color = labelC)
        for (i in 1..4) {
            val gy = h - h * i / 4
            drawLine(grid, Offset(0f, gy), Offset(w, gy), 1f)
            if (i % 2 == 0) drawText(tm, "${Fmt.ma(maxMa * i / 4)} мА", Offset(4f, gy + 2f), ls)
        }
        val line = Path()
        val area = Path()
        ticks.forEachIndexed { i, t ->
            val px = x(t.t)
            val py = y(t.drawMa)
            if (i == 0) {
                line.moveTo(px, py); area.moveTo(px, h); area.lineTo(px, py)
            } else {
                line.lineTo(px, py); area.lineTo(px, py)
            }
        }
        area.lineTo(x(t1), h)
        area.close()
        drawPath(area, Brush.verticalGradient(listOf(drawC.copy(alpha = 0.28f), drawC.copy(alpha = 0.02f))))
        drawPath(line, drawC, style = Stroke(width = 2.5.dp.toPx(), cap = StrokeCap.Round))
        val mp = Path()
        ticks.forEachIndexed { i, t -> if (i == 0) mp.moveTo(x(t.t), y(t.modelMa)) else mp.lineTo(x(t.t), y(t.modelMa)) }
        drawPath(mp, modelC, style = Stroke(width = 1.8.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(10f, 8f))))
        val s = selT?.let { st -> ticks.firstOrNull { it.t == st } } ?: ticks.last()
        val sx = x(s.t)
        if (selT != null) drawLine(drawC.copy(alpha = 0.6f), Offset(sx, 0f), Offset(sx, h), 1.5.dp.toPx())
        drawCircle(drawC, 5.dp.toPx(), Offset(sx, y(s.drawMa)))
        drawCircle(modelC, 3.5.dp.toPx(), Offset(sx, y(s.modelMa)))
    }
}

// ---------------- прочие графики ----------------

/** Кольцо-шкала 0..1. */
@Composable
fun RingGauge(fraction: Float, color: Color, modifier: Modifier = Modifier, size: Dp = 128.dp, stroke: Dp = 12.dp, center: @Composable () -> Unit) {
    val f = animatedFraction(fraction)
    val track = MaterialTheme.colorScheme.surfaceContainerHighest
    Box(modifier.size(size), contentAlignment = Alignment.Center) {
        Canvas(Modifier.size(size)) {
            val sw = stroke.toPx()
            val inset = sw / 2
            val arc = Size(this.size.width - sw, this.size.height - sw)
            drawArc(track, 135f, 270f, false, Offset(inset, inset), arc, style = Stroke(sw, cap = StrokeCap.Round))
            if (f > 0.002f) drawArc(color, 135f, 270f * f, false, Offset(inset, inset), arc, style = Stroke(sw, cap = StrokeCap.Round))
        }
        center()
    }
}

/** Время на частотах кластера: чем выше частота, тем «горячее» цвет. */
@Composable
fun ResidencyChart(c: Cluster) {
    val rows = c.residency.filter { it.second > 0 }
    if (rows.isEmpty()) return
    val total = rows.sumOf { it.second }.coerceAtLeast(1)
    val max = rows.maxOf { it.second }.coerceAtLeast(1)
    val all = c.residency.map { it.first }.sorted()
    val cool = Color(0xFF55B04A)
    MoreList(rows, initial = 24, key = "res" + c.policy) { (khz, ms) ->
        val pos = all.indexOf(khz).toFloat() / (all.size - 1).coerceAtLeast(1)
        Row(Modifier.padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("${Fmt.d1(khz / 1e6)} ГГц", Modifier.width(68.dp), style = MaterialTheme.typography.labelMedium.merge(NumStyle))
            Box(Modifier.weight(1f)) { ShareBar(ms.toFloat() / max, lerp(cool, Pal.cpu, pos), height = 8.dp) }
            Text(
                "${Fmt.pctRaw(100.0 * ms / total)} %", Modifier.width(52.dp).padding(start = 6.dp),
                style = MaterialTheme.typography.labelMedium.merge(NumStyle), color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Столбики по дням (хранилище). */
@Composable
fun DayBars(days: List<StorageDay>, modifier: Modifier = Modifier) {
    if (days.isEmpty()) return
    val max = days.maxOf { it.bytes }.coerceAtLeast(1)
    val c = MaterialTheme.colorScheme.primary
    Row(modifier.fillMaxWidth().height(110.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.Bottom) {
        days.takeLast(14).forEachIndexed { i, d ->
            val f = animatedFraction((d.bytes.toFloat() / max).coerceAtLeast(0.03f), i * 40)
            Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(if (days.size <= 7) Fmt.bytes(d.bytes) else "", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
                Box(Modifier.fillMaxWidth().height(70.dp), contentAlignment = Alignment.BottomCenter) {
                    Box(Modifier.fillMaxWidth().height((70 * f).dp).clip(RoundedCornerShape(topStart = 6.dp, topEnd = 6.dp)).background(c.copy(alpha = 0.75f)))
                }
                Text(d.date.takeLast(5).let { "${it.takeLast(2)}.${it.take(2)}" }, style = MaterialTheme.typography.labelSmall, maxLines = 1)
            }
        }
    }
}
