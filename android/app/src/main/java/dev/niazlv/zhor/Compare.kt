@file:OptIn(ExperimentalMaterial3Api::class)

package dev.niazlv.zhor

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.CompareArrows
import androidx.compose.material.icons.outlined.Apps
import androidx.compose.material.icons.outlined.Bedtime
import androidx.compose.material.icons.outlined.HourglassEmpty
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.PhoneAndroid
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlin.math.abs

@Composable
fun CompareScreen(vm: Vm, navi: Navi, req: CmpReq) {
    LaunchedEffect(req.key) { vm.loadCompare(req) }
    val st = vm.cmps[req.key] ?: Res.Loading
    val appName = req.uid?.let { vm.labelOfUid(it, "uid $it") }
    val title = if (req.mode == "action") "До и после" else "Два периода покоя"
    Scaffold(
        topBar = {
            DetailTopBar(title, appName ?: "сравнение покоя: экран выключен, без зарядки", onBack = { navi.back() }) {
                IconButton(onClick = { vm.loadCompare(req, force = true) }, enabled = st !is Res.Loading) { Icon(Icons.Outlined.Refresh, "пересчитать") }
            }
        },
    ) { pad ->
        when (st) {
            is Res.Ok -> {
                val c = st.v
                if (!c.ok || c.a == null || c.b == null) {
                    Column(Modifier.padding(pad)) {
                        EmptyState(Icons.Outlined.HourglassEmpty, "Пока не с чем сравнивать", c.reason ?: "Нет данных.")
                    }
                } else {
                    CompareContent(vm, navi, req, c, c.a, c.b, appName, Modifier.padding(pad))
                }
            }
            is Res.Err -> Column(Modifier.padding(pad)) { ErrorCard("Сравнение не построено", st.msg) { vm.loadCompare(req, force = true) } }
            else -> Column(Modifier.padding(pad)) { SkeletonScreen() }
        }
    }
}

/** Изменение b относительно a: подпись и оценка. Меньше 10 % — «без изменений». */
private fun delta(a: Double, b: Double, lowerBetter: Boolean, tiny: Double): Pair<String, Lvl> {
    if (a < tiny && b < tiny) return "—" to Lvl.NONE
    if (a < tiny) return "было ≈0" to if (lowerBetter) Lvl.BAD else Lvl.GOOD
    val pct = (b - a) / a * 100
    if (abs(pct) < 10) return "≈" to Lvl.NONE
    val good = if (lowerBetter) pct < 0 else pct > 0
    val sign = if (pct > 0) "+" else "−"
    return "$sign${Fmt.d0(abs(pct))} %" to if (good) Lvl.GOOD else Lvl.BAD
}

@Composable
private fun CompareContent(vm: Vm, navi: Navi, req: CmpReq, c: Comparison, a: CmpWin, b: CmpWin, appName: String?, modifier: Modifier) {
    val aLabel = if (req.mode == "action") "До" else "Раньше"
    val bLabel = if (req.mode == "action") "После" else "Позже"
    LazyColumn(modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
        item(key = "verdict") { Verdict(c, a, b, appName) }
        item(key = "win") {
            Section("Отрезки", icon = Icons.AutoMirrored.Outlined.CompareArrows) {
                KV(aLabel, Fmt.span(a.from, a.to), sub = "покоя ${Fmt.dur(a.offMs)}")
                KV(bLabel, Fmt.span(b.from, b.to), sub = "покоя ${Fmt.dur(b.offMs)}")
                c.notes.forEach { Explain(it) }
                Explain("Сравнивается только покой, приведённый к часу, — поэтому отрезки разной длины сравнимы. Ночь с ночью честнее, чем ночь с днём: днём в покое больше пушей и синхронизаций.")
            }
        }
        item(key = "phone") {
            Section("Телефон в покое", icon = Icons.Outlined.PhoneAndroid) {
                Header(aLabel, bLabel)
                MetricRow("Ток", a.ma, b.ma, "мА", lowerBetter = true, tiny = 1.0) { Fmt.d0(it) }
                MetricRow("Разряд", a.pctH, b.pctH, "%/ч", lowerBetter = true, tiny = 0.05) { Fmt.d1(it) }
                MetricRow("Глубокий сон", a.deepPct, b.deepPct, "%", lowerBetter = false, tiny = 1.0) { Fmt.d0(it) }
                MetricRow("Пробуждений", a.wakeupsH, b.wakeupsH, "в час", lowerBetter = true, tiny = 1.0) { Fmt.d0(it) }
                MetricRow("Срывов засыпания", a.failsH, b.failsH, "в час", lowerBetter = true, tiny = 1.0) { Fmt.d0(it) }
                MetricRow("Процессор (модель)", a.cpuMa, b.cpuMa, "мА", lowerBetter = true, tiny = 0.5) { Fmt.d1(it) }
                if (a.tempMax > 0 && b.tempMax > 0) MetricRow("Батарея, максимум", a.tempMax, b.tempMax, "°C", lowerBetter = true, tiny = 1.0) { Fmt.d1(it) }
            }
        }
        val pa = a.app
        val pb = b.app
        if (pa != null && pb != null) item(key = "app") {
            Section(appName ?: "Приложение", sub = "процессор и запуски — в покое, остальное — Android за весь отрезок", icon = Icons.Outlined.Apps) {
                Header(aLabel, bLabel)
                MetricRow("Процессор в покое", pa.cpuMa, pb.cpuMa, "мА", lowerBetter = true, tiny = 0.1) { Fmt.d1(it) }
                MetricRow("Запусков процессов", pa.startsH, pb.startsH, "в час", lowerBetter = true, tiny = 0.1) { Fmt.d1(it) }
                if (pa.alarmsH != null && pb.alarmsH != null) MetricRow("Будильников", pa.alarmsH, pb.alarmsH, "в час", lowerBetter = true, tiny = 0.1) { Fmt.d1(it) }
                if (pa.wakelockMinH != null && pb.wakelockMinH != null) MetricRow("Wakelock", pa.wakelockMinH, pb.wakelockMinH, "мин в час", lowerBetter = true, tiny = 0.05) { Fmt.d1(it) }
                if (pa.jobsH != null && pb.jobsH != null) MetricRow("Фоновых заданий", pa.jobsH, pb.jobsH, "в час", lowerBetter = true, tiny = 0.1) { Fmt.d1(it) }
                if (pa.mahH != null && pb.mahH != null) MetricRow("Всего по модели Android", pa.mahH, pb.mahH, "мА·ч в час", lowerBetter = true, tiny = 0.05) { Fmt.d1(it) }
            }
        }
        if (c.apps.isNotEmpty()) item(key = "apps") {
            Section("Кто тратил процессор в покое", sub = "средний ток, мА", icon = Icons.Outlined.Bedtime) {
                Header(aLabel, bLabel)
                c.apps.forEach { r ->
                    val (d, lvl) = delta(r.aMa, r.bMa, lowerBetter = true, tiny = 0.3)
                    Row(
                        Modifier.fillMaxWidth().clickable(enabled = r.kind == "app") { navi.compare(req.copy(uid = r.uid)) }.padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        UidIcon(vm, r.uid, r.label, 24.dp)
                        Spacer(Modifier.width(8.dp))
                        Text(vm.labelOfUid(r.uid, r.label), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Num(Fmt.d1(r.aMa))
                        Num(Fmt.d1(r.bMa), strong = true)
                        DeltaText(d, lvl)
                    }
                }
                if (req.uid == null) Explain("Нажмите на приложение — сравнение покажет и его будильники, wakelock и задания.")
            }
        }
    }
}

@Composable
private fun Verdict(c: Comparison, a: CmpWin, b: CmpWin, appName: String?) {
    val app = a.app != null && b.app != null && appName != null
    val (va, vb) = if (app) a.app!!.cpuMa to b.app!!.cpuMa else a.ma to b.ma
    val (d, lvl) = delta(va, vb, lowerBetter = true, tiny = if (app) 0.1 else 1.0)
    val title = when {
        !c.enough -> "Пока мало данных"
        lvl == Lvl.GOOD -> "Стало лучше"
        lvl == Lvl.BAD -> "Стало хуже"
        else -> "Почти без изменений"
    }
    val shown = if (c.enough) lvl else Lvl.INFO
    Section(title, sub = if (app) "$appName: процессор в покое" else "средний ток в покое", icon = Icons.AutoMirrored.Outlined.CompareArrows, iconTint = if (shown == Lvl.NONE) null else shown.color()) {
        Row(verticalAlignment = Alignment.Bottom) {
            Text("${if (app) Fmt.d1(va) else Fmt.d0(va)} → ${if (app) Fmt.d1(vb) else Fmt.d0(vb)}", style = MaterialTheme.typography.displaySmall.merge(NumStyle))
            Text(" мА", Modifier.padding(bottom = 6.dp), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            if (shown != Lvl.NONE && d != "—" && d != "≈") LevelPill(shown, d)
        }
        if (!c.enough) {
            Explain("Нужно хотя бы ${Fmt.dur(c.minOffMs)} покоя в каждом отрезке. Загляните утром — после ночи будет видно.")
        } else if (!app) {
            Explain("Разряд в покое: ${Fmt.d1(a.pctH)} → ${Fmt.d1(b.pctH)} %/ч, глубокий сон: ${Fmt.d0(a.deepPct)} → ${Fmt.d0(b.deepPct)} %.")
        }
    }
}

/** Ширина столбца чисел. */
private val COL = 64.dp

@Composable
private fun Header(a: String, b: String) {
    Row(Modifier.fillMaxWidth().padding(bottom = 2.dp)) {
        Spacer(Modifier.weight(1f))
        Text(a, Modifier.width(COL), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.End, maxLines = 1)
        Text(b, Modifier.width(COL), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.End, maxLines = 1)
        Spacer(Modifier.width(64.dp))
    }
}

@Composable
private fun Num(s: String, strong: Boolean = false) {
    Text(
        s, Modifier.width(COL), style = MaterialTheme.typography.bodyMedium.merge(NumStyle), maxLines = 1,
        fontWeight = if (strong) FontWeight.SemiBold else FontWeight.Normal, textAlign = TextAlign.End,
    )
}

@Composable
private fun DeltaText(d: String, lvl: Lvl) {
    Text(
        d, Modifier.width(64.dp), style = MaterialTheme.typography.labelMedium.merge(NumStyle), textAlign = TextAlign.End,
        color = if (lvl == Lvl.NONE) MaterialTheme.colorScheme.onSurfaceVariant else lvl.color(), fontWeight = FontWeight.SemiBold,
    )
}

@Composable
private fun MetricRow(name: String, a: Double, b: Double, unit: String, lowerBetter: Boolean, tiny: Double, fmt: (Double) -> String) {
    val (d, lvl) = delta(a, b, lowerBetter, tiny)
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(name, style = MaterialTheme.typography.bodyMedium)
            Text(unit, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Num(fmt(a))
        Num(fmt(b), strong = true)
        DeltaText(d, lvl)
    }
}
