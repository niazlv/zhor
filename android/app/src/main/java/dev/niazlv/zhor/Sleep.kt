@file:OptIn(ExperimentalMaterial3Api::class)

package dev.niazlv.zhor

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AlarmOn
import androidx.compose.material.icons.outlined.Bedtime
import androidx.compose.material.icons.outlined.Block
import androidx.compose.material.icons.outlined.DeveloperBoard
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.Memory
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.outlined.PowerSettingsNew
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@Composable
fun SleepTab(vm: Vm, navi: Navi) {
    val r = vm.report
    PullToRefreshBox(isRefreshing = vm.loading && r != null, onRefresh = { vm.refresh(fresh = true) }, modifier = Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
            item(key = "range") { RangeHeader(vm, r) }
            if (r == null) {
                if (vm.error != null) item { ErrorCard("Отчёт не загрузился", vm.error!!) { vm.refresh() } } else item { SkeletonScreen() }
                return@LazyColumn
            }
            item(key = "cov") { CoverageNote(r) }
            item(key = "hero") { SleepHero(r) }
            item(key = "susp") { Suspends(r) }
            r.sys?.let { s ->
                if (s.dozeMs + s.partialWlMs + s.radioMs > 0) item(key = "android") {
                    Section("Android при выключенном экране", sub = "за ${Fmt.span(s.from, s.to)}", icon = Icons.Outlined.PowerSettingsNew) {
                        if (s.dozeMs > 0) KV("В режиме Doze", Fmt.dur(s.dozeMs), help = "doze", sub = "чем больше, тем лучше")
                        if (s.partialWlMs > 0) KV("Wakelock приложений всего", Fmt.dur(s.partialWlMs), help = "wakelock", sub = "сколько приложения держали процессор")
                        if (s.radioMs > 0) KV("Модем в активном режиме", Fmt.dur(s.radioMs), help = "radio_active")
                    }
                }
            }
            val k = r.kernel
            if (k.wakeupSources.isNotEmpty()) item(key = "ws") { WakeupSources(k.wakeupSources) }
            val reasons = k.wakeupReasons.ifEmpty { k.resumeReasons }
            if (reasons.isNotEmpty()) item(key = "wr") {
                Section("Почему просыпался", sub = "причины выхода из сна по записям ядра", help = "wakeup_reason", icon = Icons.Outlined.NotificationsActive) {
                    ItemList(reasons, Pal.radio, key = "wr")
                }
            }
            if (k.abortReasons.isNotEmpty()) item(key = "ab") {
                Section("Почему не засыпал", sub = "кто сорвал попытку уснуть", help = "abort", icon = Icons.Outlined.Block) {
                    ItemList(k.abortReasons, Pal.cpu, showMs = false, key = "ab")
                }
            }
            if (k.kernelWakelocks.isNotEmpty()) item(key = "kw") {
                Section("Wakelock ядра", sub = "драйверы держали процессор (batterystats)", help = "kernel_wakelock", icon = Icons.Outlined.Lock) {
                    ItemList(k.kernelWakelocks, Pal.wakelock, key = "kw")
                }
            }
            if (k.nativeWakelocks.isNotEmpty()) item(key = "nw") {
                Section("Нативные wakelock", sub = "системные демоны и HAL, с PID", help = "native_wakelock", icon = Icons.Outlined.DeveloperBoard) {
                    ItemList(k.nativeWakelocks, Pal.wakelock, key = "nw")
                    Explain("Пока экран включён, часть из них (дисплей, PowerManager) держится всегда — смотрите на остальные.")
                }
            }
            if (r.clusters.isNotEmpty()) item(key = "cpu") { Clusters(r) }
        }
    }
}

@Composable
private fun SleepHero(r: Report) {
    val t = r.totals
    val lvl = Norms.deepSleep(t.deepSleepPct, t.screenOffMs)
    Section("Глубокий сон", sub = "доля времени с выключенным экраном, когда процессор спал", help = "deep_sleep", icon = Icons.Outlined.Bedtime) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            RingGauge((t.deepSleepPct / 100).toFloat(), lvl.color()) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    AnimatedNumber(t.deepSleepPct, { "${Fmt.pctRaw(it)} %" }, MaterialTheme.typography.headlineMedium)
                    Text("спал", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                LevelPill(lvl, Norms.deepSleepWord(lvl).replaceFirstChar { it.uppercase() })
                Spacer(Modifier.height(8.dp))
                Text("Экран выключен ${Fmt.dur(t.screenOffMs)}", style = MaterialTheme.typography.bodyMedium)
                if (t.screenOffSuspendMs > 0) Text("из них во сне ${Fmt.dur(t.screenOffSuspendMs)}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("Всего спал ${Fmt.dur(t.suspendMs)}, бодрствовал ${Fmt.dur(t.awakeMs)}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Spacer(Modifier.height(12.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            val dl = Norms.drainOff(t.pctPerHourOff, t.screenOffMs)
            MetricTile("Разряд в покое", Fmt.d1(t.pctPerHourOff), Modifier.weight(1f), unit = "%/ч", lvl = dl, sub = "норма — меньше 1 %/ч")
            MetricTile(
                "Ток без экрана", Fmt.ma(t.measuredOffMa), Modifier.weight(1f), unit = "мА", lvl = Norms.offMa(t.measuredOffMa, t.screenOffMs),
                sub = "во сне 5–20 мА",
            )
        }
        when {
            t.screenOffMs < Norms.MIN_OFF_MS -> Explain("Экран выключался меньше 10 минут — оценке сна пока рано доверять. Выберите период подлиннее или «Экран выкл.».")
            lvl == Lvl.GOOD -> Explain("Телефон спит как надо: фоновые пробуждения короткие и редкие.")
            else -> Explain("Телефон много бодрствует без экрана — это 50–200 мА вместо 5–20. Ниже: «Кто будит и держит» и «Почему не засыпал».")
        }
    }
}

@Composable
private fun Suspends(r: Report) {
    val t = r.totals
    if (t.suspends + t.suspendFails == 0L) return
    val lvl = Norms.failRatio(t.suspends, t.suspendFails)
    Section("Попытки уснуть", help = "suspends", icon = Icons.Outlined.AlarmOn) {
        val all = t.suspends + t.suspendFails
        StackBar(listOf(LocalSemColorGood() to t.suspends.toDouble(), lvl.color() to t.suspendFails.toDouble()), height = 10.dp)
        Spacer(Modifier.height(6.dp))
        KV("Удачных засыпаний", Fmt.count(t.suspends))
        KV("Сорвано", "${Fmt.count(t.suspendFails)} (${Fmt.pct(100.0 * t.suspendFails / all.coerceAtLeast(1))})", lvl)
        val hours = (t.screenOffMs / 3_600_000.0).coerceAtLeast(0.05)
        if (t.screenOffMs >= Norms.MIN_OFF_MS) KV("Засыпаний в час без экрана", Fmt.d0(t.suspends / hours), sub = "каждое = пробуждение перед ним")
        if (lvl == Lvl.BAD || lvl == Lvl.WARN) Explain("Много срывов: в момент засыпания кто-то держит процессор. Виновник — в «Почему не засыпал».")
    }
}

@Composable
private fun LocalSemColorGood() = LocalSem.current.good

@Composable
private fun WakeupSources(list: List<WsItem>) {
    Section("Кто будит и держит", sub = "источники пробуждения ядра (/sys/class/wakeup)", help = "wakeup_source", icon = Icons.Outlined.NotificationsActive) {
        val max = list.maxOf { it.heldMs }.coerceAtLeast(1)
        Explain("«Будил» — сколько раз вывел из сна, «держал» — сколько был активен, «мешал» — на сколько откладывал сон.")
        Spacer(Modifier.height(6.dp))
        MoreList(list, initial = 6, key = "ws") { w ->
            Column(Modifier.padding(vertical = 5.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(w.name, Modifier.weight(1f), fontFamily = FontFamily.Monospace, fontSize = 13.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text("держал ${Fmt.dur(w.heldMs)}", style = MaterialTheme.typography.bodySmall.merge(NumStyle), fontWeight = FontWeight.SemiBold)
                }
                ShareBar(w.heldMs.toFloat() / max, Pal.radio, height = 5.dp)
                Text(
                    "будил ${Fmt.times(w.wakeups)} · активаций ${Fmt.count(w.activations)}" + if (w.preventMs > 0) " · мешал уснуть ${Fmt.dur(w.preventMs)}" else "",
                    style = MaterialTheme.typography.bodySmall,
                )
                w.explain?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
        }
    }
}

@Composable
private fun Clusters(r: Report) {
    val cl = r.clusters.sortedBy { it.maxKhz }
    val totalMs = cl.sumOf { it.cpuMs }.coerceAtLeast(1)
    val totalMah = cl.sumOf { it.mah }.coerceAtLeast(0.001)
    Section("Процессор", sub = "сколько времени ядра провели на каждой частоте", help = "clusters", icon = Icons.Outlined.Memory) {
        StackBar(cl.mapIndexed { i, c -> clusterColor(i, cl.size) to c.mah }, height = 10.dp)
        Spacer(Modifier.height(4.dp))
        cl.forEachIndexed { i, c ->
            LegendRow(
                clusterColor(i, cl.size), clusterName(i, cl.size, c), "${Fmt.mah(c.mah)} мА·ч", Fmt.pct(100 * c.mah / totalMah),
                sub = "${Fmt.dur(c.cpuMs)} CPU-времени (${Fmt.pct(100.0 * c.cpuMs / totalMs)})",
            )
        }
        cl.forEachIndexed { i, c ->
            Spacer(Modifier.height(12.dp))
            Text(clusterName(i, cl.size, c), style = MaterialTheme.typography.titleSmall)
            val freqs = c.residency.map { it.first }.sorted()
            val hiFrom = freqs.getOrNull((freqs.size * 2) / 3) ?: Long.MAX_VALUE
            val ms = c.residency.sumOf { it.second }.coerceAtLeast(1)
            val hi = c.residency.filter { it.first >= hiFrom }.sumOf { it.second }
            Text(
                "на верхней трети частот — ${Fmt.pct(100.0 * hi / ms)} времени",
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(4.dp))
            ResidencyChart(c)
        }
        Explain("Зелёные — низкие частоты, красные — высокие: миллисекунда на максимуме стоит в 5–10 раз дороже. Фону место на малых ядрах внизу шкалы.")
        if (r.totals.gpuBusyPct > 0) {
            Spacer(Modifier.height(8.dp))
            KV("GPU занят", "${Fmt.pct(r.totals.gpuBusyPct)} бодрствования", if (r.totals.gpuBusyPct > 30) Lvl.WARN else Lvl.NONE, help = "gpu")
        }
    }
}

private fun clusterName(i: Int, n: Int, c: Cluster): String {
    val name = when {
        n == 1 -> "Ядра"
        i == 0 -> "Экономные ядра"
        i == n - 1 && n >= 3 -> "Главное ядро"
        i == n - 1 -> "Быстрые ядра"
        else -> "Быстрые ядра"
    }
    val cpus = if (c.cpus.isEmpty()) c.policy else if (c.cpus.size == 1) "${c.cpus[0]}" else "${c.cpus.first()}–${c.cpus.last()}"
    return "$name $cpus · до ${Fmt.d1(c.maxKhz / 1e6)} ГГц"
}

private fun clusterColor(i: Int, n: Int) = when {
    n <= 1 -> Pal.cpu
    i == 0 -> Pal.sensors
    i == n - 1 -> Pal.cpu
    else -> Pal.screen
}

