@file:OptIn(ExperimentalMaterial3Api::class)

package dev.niazlv.zhor

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.BatteryChargingFull
import androidx.compose.material.icons.outlined.Battery5Bar
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.DonutLarge
import androidx.compose.material.icons.outlined.Insights
import androidx.compose.material.icons.outlined.Leaderboard
import androidx.compose.material.icons.outlined.PhoneAndroid
import androidx.compose.material.icons.outlined.Timeline
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

@Composable
fun SummaryTab(vm: Vm, navi: Navi, goTab: (Int) -> Unit) {
    val r = vm.report
    PullToRefreshBox(isRefreshing = vm.loading && r != null, onRefresh = { vm.refresh(fresh = true) }, modifier = Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
            item(key = "range") { RangeHeader(vm, r) }
            if (vm.fileMode) item(key = "file") { FileModeNote() }
            vm.error?.let { e -> item(key = "err") { ErrorCard("Отчёт не обновился", e) { vm.refresh(fresh = false) } } }
            if (r == null) {
                if (vm.error == null) item(key = "sk") { SkeletonScreen() }
                return@LazyColumn
            }
            item(key = "upd") { UpdateBanner(vm) { navi.settings() } }
            item(key = "watch") { WatchSummary(vm, navi) }
            reportOverview(r, vm, navi, seg = null, goTab = goTab)
        }
    }
}

/** Общая часть сводки и отчёта по отрезку. */
fun LazyListScope.reportOverview(r: Report, vm: Vm, navi: Navi, seg: String?, goTab: ((Int) -> Unit)?) {
    item(key = "cov") { CoverageNote(r) }
    item(key = "hero") { HeroCard(r) }
    item(key = "ins") { InsightsCard(r, vm, navi, seg, goTab) }
    item(key = "screen") { ScreenCard(r, goTab) }
    item(key = "where") { WhereCard(r) }
    item(key = "tl") {
        Section("Лента", sub = "ток по времени: замер и модель", help = "timeline", icon = Icons.Outlined.Timeline) {
            TimelineBlock(r, vm, seg, onOpenSeg = { a, b -> navi.seg(a, b) }, onOpenApp = { navi.app(it, seg) })
        }
    }
    item(key = "top") { TopAppsCard(r, vm, navi, seg, goTab) }
    if (r.fullMah > 0) item(key = "bat") { BatteryCard(r) }
    item(key = "foot") { Footer(r) }
}

@Composable
private fun HeroCard(r: Report) {
    val t = r.totals
    Section("Израсходовано", sub = r.label.replaceFirstChar { it.uppercase() }, help = "mah", icon = Icons.Outlined.Battery5Bar) {
        if (t.spanMs == 0L) {
            Text("За этот период ядро ещё ничего не записало.", style = MaterialTheme.typography.bodyMedium)
            return@Section
        }
        Row(verticalAlignment = Alignment.Bottom) {
            AnimatedNumber(t.measuredMah, { Fmt.mah(it) }, MaterialTheme.typography.displayMedium)
            Text(" мА·ч", Modifier.padding(bottom = 10.dp), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            if (t.levelStart > 0) {
                Column(horizontalAlignment = Alignment.End) {
                    val d = t.levelStart - t.levelEnd
                    Text(if (d > 0) "−$d %" else if (d < 0) "+${-d} %" else "0 %", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                    Text("${t.levelStart} → ${t.levelEnd} %", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        Text(
            "записано ${Fmt.dur(t.spanMs)} · источник: ${t.measuredSource.ifBlank { "счётчик батареи" }}",
            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile("Средний ток", Fmt.ma(t.measuredAvgMa), Modifier.weight(1f), unit = "мА", sub = "скорость расхода")
            // уровень заряда меняется шагом 1 %: на коротком отрезке %/ч считаем по току
            val coarse = t.spanMs < 30 * 60_000 || kotlin.math.abs(t.levelStart - t.levelEnd) < 2
            if (coarse && r.fullMah > 0) {
                MetricTile("Разряд", Fmt.d1(t.measuredAvgMa / r.fullMah * 100), Modifier.weight(1f), unit = "%/ч", sub = "по току: уровень меняется шагом 1 %")
            } else {
                MetricTile("Разряд", Fmt.d1(t.pctPerHour), Modifier.weight(1f), unit = "%/ч", sub = "по уровню заряда")
            }
        }
        val full = r.fullMah
        if (full > 0 && t.measuredAvgMa > 1 && t.plugMs < t.spanMs / 2) {
            Explain("При таком среднем токе полного заряда хватило бы примерно на ${Fmt.dur((full / t.measuredAvgMa * 3_600_000).toLong())}.")
        }
        if (t.plugMs > 0) Explain("${Fmt.dur(t.plugMs)} телефон был на зарядке — это время в расход не входит.")
    }
}

/** В выводах ядра приложение названо пакетом — подставляем человеческое имя. */
fun humanize(i: Insight, r: Report, vm: Vm): Insight {
    return i.copy(title = vm.nice(i.title, r), text = vm.nice(i.text, r))
}

@Composable
private fun InsightsCard(r: Report, vm: Vm, navi: Navi, seg: String?, goTab: ((Int) -> Unit)?) {
    val list = Insights.of(r)
    if (list.isEmpty()) return
    Section("Главное", sub = if (r.insights != null) "выводы ядра по этому периоду" else "что бросается в глаза", icon = Icons.Outlined.Insights) {
        list.take(6).map { i -> humanize(i, r, vm) }.forEach { i ->
            val action: (() -> Unit)? = when {
                i.uid != null && r.app(i.uid) != null -> ({ navi.app(i.uid, seg) })
                // спецвозможности: объяснение и кнопка — в карточке «Android OS»
                i.uid == null && i.title.startsWith("Спецвозможности") && r.app(1000) != null -> ({ navi.app(1000, seg) })
                goTab != null && i.section in setOf("sleep", "kernel", "cpu") -> ({ goTab(2) })
                goTab != null && i.section == "apps" -> ({ goTab(1) })
                else -> null
            }
            InsightCard(i, action)
        }
    }
}

@Composable
private fun ScreenCard(r: Report, goTab: ((Int) -> Unit)?) {
    val t = r.totals
    if (t.spanMs == 0L) return
    Section("Экран и сон", sub = "главное — расход с выключенным экраном", help = "pct_per_hour", icon = Icons.Outlined.PhoneAndroid) {
        StackBar(listOf(Pal.screen to t.screenOnMs.toDouble(), Pal.off to t.screenOffMs.toDouble(), Pal.plug to t.plugMs.toDouble()), height = 12.dp)
        Spacer(Modifier.height(10.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile(
                "Экран вкл.", Fmt.dur(t.screenOnMs), Modifier.weight(1f),
                sub = if (t.screenOnMs > 0) "≈ ${Fmt.ma(t.measuredOnMa)} мА" else "не включался",
            )
            val off = Norms.drainOff(t.pctPerHourOff, t.screenOffMs)
            MetricTile(
                "Экран выкл.", Fmt.dur(t.screenOffMs), Modifier.weight(1f), lvl = off,
                sub = if (t.screenOffMs > 0) "≈ ${Fmt.ma(t.measuredOffMa)} мА · ${Fmt.d1(t.pctPerHourOff)} %/ч" else "не выключался без зарядки",
            )
        }
        Spacer(Modifier.height(8.dp))
        val sl = Norms.deepSleep(t.deepSleepPct, t.screenOffMs)
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp))
                .then(if (goTab != null) Modifier.clickable { goTab(2) } else Modifier).padding(vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text("Глубокий сон при выкл. экране", style = MaterialTheme.typography.bodyMedium)
                Text(
                    if (t.screenOffMs < Norms.MIN_OFF_MS) "экран выключался ненадолго — судить рано" else "норма — больше 90 %",
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            LevelPill(sl, "${Fmt.pctRaw(t.deepSleepPct)} % · ${Norms.deepSleepWord(sl)}")
            if (goTab != null) Icon(Icons.Outlined.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        val off = Norms.drainOff(t.pctPerHourOff, t.screenOffMs)
        if (off == Lvl.BAD || off == Lvl.WARN) {
            Explain("С выключенным экраном уходило ${Fmt.d1(t.pctPerHourOff)} %/ч — норма меньше 1 %/ч. Кто мешает спать — на вкладке «Сон».")
        } else if (off == Lvl.GOOD) {
            Explain("В покое телефон тратит мало — фон в порядке.")
        }
    }
}

@Composable
private fun WhereCard(r: Report) {
    val t = r.totals
    if (t.spanMs == 0L) return
    Section("Куда ушло", sub = "факт по батарее против модели по процессору и экрану", help = "model", icon = Icons.Outlined.DonutLarge) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile("Факт", Fmt.mah(t.measuredMah), Modifier.weight(1f), unit = "мА·ч", sub = "счётчик батареи")
            MetricTile("Модель", Fmt.mah(t.modelTotalMah), Modifier.weight(1f), unit = "мА·ч", sub = "процессор + экран")
        }
        Spacer(Modifier.height(12.dp))
        val parts = listOf(
            Triple(Pal.cpu, "Процессор (работа)", t.modelCpuMah),
            Triple(Pal.cpuIdle, "Процессор в простое", t.modelIdleMah),
            Triple(Pal.screen, "Экран", t.modelScreenMah),
            Triple(Pal.other, "Прочее: радио, GPS, GPU…", t.otherMah),
        )
        val sum = parts.sumOf { it.third }.coerceAtLeast(0.001)
        StackBar(parts.map { it.first to it.third }, height = 14.dp)
        Spacer(Modifier.height(6.dp))
        val helps = listOf("model", "cpu_idle", "model", "other")
        parts.forEachIndexed { i, (c, n, v) ->
            LegendRow(c, n, "${Fmt.mah(v)} мА·ч", share = Fmt.pct(100 * v / sum), help = if (i == 1 || i == 3) helps[i] else null)
        }
        Norms.modelVsFact(t.modelTotalMah, t.measuredMah)?.let { Explain(it) }
        r.sys?.let { s ->
            val g = s.global.filter { it.value >= 0.05 }.toList().sortedByDescending { it.second }
            if (g.isNotEmpty()) {
                Spacer(Modifier.height(14.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text("Модель Android по компонентам", style = MaterialTheme.typography.titleSmall)
                        Text(
                            "за ${Fmt.span(s.from, s.to)}" + if (s.actualMah > 0) " · по данным Android ушло ${Fmt.mah(s.actualMah)} мА·ч" else "",
                            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    HelpButton("android_model")
                }
                Spacer(Modifier.height(6.dp))
                val gs = g.sumOf { it.second }.coerceAtLeast(0.001)
                StackBar(g.map { Pal.comp(it.first) to it.second })
                Spacer(Modifier.height(4.dp))
                MoreList(g, initial = 5, key = "glob") { (k, v) ->
                    val ms = s.globalMs[k]?.takeIf { it > 0 }?.let { "активно ${Fmt.dur(it)}" }
                    LegendRow(Pal.comp(k), Fmt.compName(k).replaceFirstChar { it.uppercase() }, "${Fmt.mah(v)} мА·ч", Fmt.pct(100 * v / gs), sub = ms)
                }
                s.note?.let { Explain(it) }
                val radio = s.global["mobile_radio"] ?: 0.0
                if (radio > 0.2 * gs) Explain("Мобильная связь — заметная часть расхода. При слабом сигнале модем работает на большой мощности; помогает Wi-Fi.")
            }
        }
    }
}

@Composable
private fun TopAppsCard(r: Report, vm: Vm, navi: Navi, seg: String?, goTab: ((Int) -> Unit)?) {
    if (r.apps.isEmpty()) return
    val total = r.totals.measuredMah.takeIf { it > 0 } ?: r.apps.sumOf { it.mah }
    Section("Кто жрёт", sub = "доля — от израсходованного по счётчику", help = "app_total", icon = Icons.Outlined.Leaderboard) {
        val max = r.apps.first().mah.coerceAtLeast(0.001)
        val list = r.apps
        if (goTab != null) {
            list.take(6).forEachIndexed { i, a -> AppRow(vm, a, a.mah, max, total, a.why.firstOrNull()?.let { vm.nice(it, r) }, i) { navi.app(a.uid, seg) } }
            TextButton(onClick = { goTab(1) }) {
                Text("Все приложения (${list.size})")
                Icon(Icons.Outlined.ChevronRight, null)
            }
        } else {
            MoreList(list.withIndex().toList(), initial = 8, key = "segapps") { (i, a) ->
                AppRow(vm, a, a.mah, max, total, a.why.firstOrNull()?.let { vm.nice(it, r) }, i) { navi.app(a.uid, seg) }
            }
        }
    }
}

/** Цветные части расхода приложения. */
fun appParts(a: App): List<Pair<Color, Double>> {
    val l = mutableListOf<Pair<Color, Double>>()
    l += Pal.cpu to a.cpuBest
    a.components.filter { it.key != "cpu" }.forEach { (k, v) -> l += Pal.comp(k) to v }
    return l
}

/** Строка приложения: иконка, имя, значение, полоса состава, пояснение. */
@Composable
fun AppRow(
    vm: Vm, a: App, value: Double, max: Double, total: Double, sub: String?, index: Int,
    valueText: String = "${Fmt.mah(value)} мА·ч", parts: List<Pair<Color, Double>>? = null, onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).clickable(onClick = onClick).padding(vertical = 8.dp, horizontal = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AppIcon(vm, a)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(vm.labelOf(a), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
                if (total > 0 && value > 0) {
                    Text(
                        Fmt.pct(100 * value / total), Modifier.padding(start = 6.dp, end = 8.dp),
                        style = MaterialTheme.typography.labelMedium.merge(NumStyle), color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text(valueText, style = MaterialTheme.typography.bodyLarge.merge(NumStyle), fontWeight = FontWeight.SemiBold)
            }
            Spacer(Modifier.height(5.dp))
            val f = animatedFraction((value / max).toFloat().coerceIn(0.02f, 1f), (index * 35).coerceAtMost(400))
            StackBar(parts ?: appParts(a), Modifier.fillMaxWidth(f), height = 6.dp)
            Spacer(Modifier.height(2.dp))
            if (sub != null) {
                Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun BatteryCard(r: Report) {
    val full = r.fullMah
    val design = r.designMah
    Section("Батарея", sub = "сколько держит сейчас", help = "battery_health", icon = Icons.Outlined.BatteryChargingFull) {
        if (design > 0) {
            val lvl = Norms.health(full, design)
            val pct = 100 * full / design
            KV("Ёмкость сейчас", "${Fmt.d0(full)} из ${Fmt.d0(design)} мА·ч", lvl)
            Spacer(Modifier.height(4.dp))
            ShareBar((full / design).toFloat(), lvl.color(), height = 8.dp)
            Explain(
                "Батарея держит ${Fmt.pctRaw(pct)} % паспортной ёмкости — " + when (lvl) {
                    Lvl.GOOD -> "как новая."
                    Lvl.WARN -> "обычный износ за год-два."
                    else -> "заметно сдала."
                },
            )
        } else {
            KV("Ёмкость", "${Fmt.d0(full)} мА·ч")
        }
        Explain("1 % заряда ≈ ${Fmt.d0(full / 100)} мА·ч: так можно переводить мА·ч приложений в проценты.")
    }
}

@Composable
private fun Footer(r: Report) {
    Column(Modifier.padding(horizontal = 20.dp, vertical = 10.dp)) {
        r.notes.forEach { Explain(it) }
        Row(verticalAlignment = Alignment.CenterVertically) {
            val cost = (r.device["scan_cost_us"]?.toLongOrNull() ?: 0) / 1000
            Text(
                "Отчёт собран ${Fmt.human(r.generated.takeIf { it > 0 } ?: r.to)} · профиль мощности: ${r.device["profile"] ?: "?"} · проход сканера $cost мс",
                Modifier.weight(1f), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            HelpButton("scan_cost")
        }
    }
}
