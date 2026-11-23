package dev.niazlv.zhor

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Bolt
import androidx.compose.material.icons.outlined.Leaderboard
import androidx.compose.material.icons.automirrored.outlined.ShowChart
import androidx.compose.material.icons.outlined.Speed
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleStartEffect

@Composable
fun LiveTab(vm: Vm, navi: Navi) {
    LifecycleStartEffect(Unit) {
        vm.startLive()
        onStopOrDispose { vm.stopLive() }
    }
    val l = vm.live
    var selT by rememberSaveable { mutableStateOf<Long?>(null) }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        if (vm.fileMode) item { FileModeNote() }
        if (l == null) {
            if (vm.liveError != null) item { ErrorCard("Ядро не отвечает", vm.liveError!!) }
            else item { SkeletonScreen("Спрашиваю ядро…") }
            return@LazyColumn
        }
        item(key = "hero") { LiveHero(l, vm.fileMode) }
        val sel = selT?.let { t -> l.ticks.firstOrNull { it.t == t } }
        val tick = sel ?: l.ticks.lastOrNull()
        item(key = "chart") {
            Section("Последние ${Fmt.dur((l.ticks.lastOrNull()?.t ?: 0) - (l.ticks.firstOrNull()?.t ?: 0))}", sub = "замер батареи и модель, раз в шаг ядра", help = "live", icon = Icons.AutoMirrored.Outlined.ShowChart) {
                if (l.ticks.size < 2) {
                    Text("Нужно хотя бы два шага ядра — подождите несколько секунд.", style = MaterialTheme.typography.bodyMedium)
                } else {
                    LiveChart(l.ticks, selT, { selT = it })
                    Spacer(Modifier.height(6.dp))
                    Legend(
                        listOf(
                            MaterialTheme.colorScheme.primary to "ток батареи (замер)", Pal.cpu to "модель: CPU + экран",
                            Pal.off.copy(alpha = 0.4f) to "экран выключен",
                        ),
                    )
                    if (tick != null) {
                        Spacer(Modifier.height(10.dp))
                        TickInfo(tick, sel != null) { selT = null }
                    }
                }
            }
        }
        if (tick != null && tick.top.isNotEmpty()) item(key = "top") {
            Section(
                "Кто тратит процессор",
                sub = if (sel != null) "в точке ${Fmt.clockSec(tick.t)}" else "за последний шаг замера",
                help = "model", icon = Icons.Outlined.Leaderboard,
            ) {
                val max = tick.top.maxOf { it.ma }.coerceAtLeast(0.001)
                tick.top.forEach { p ->
                    val inReport = vm.report?.app(p.uid) != null
                    Row(
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
                            .then(if (inReport) Modifier.clickable { navi.app(p.uid) } else Modifier).padding(vertical = 5.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        UidIcon(vm, p.uid, p.name, 30.dp)
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Row {
                                Text(p.name, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium)
                                Text("${Fmt.ma(p.ma)} мА", style = MaterialTheme.typography.bodyMedium.merge(NumStyle), fontWeight = FontWeight.SemiBold)
                            }
                            Text(vm.labelOfUid(p.uid, "uid ${p.uid}"), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
                            ShareBar((p.ma / max).toFloat(), Pal.cpu, height = 4.dp)
                        }
                    }
                }
                Explain("Токи здесь — модель процессора за шаг ядра. В сумме они дают линию «модель» на графике без экрана.")
            }
        }
        item(key = "cost") {
            SelfCostCard(vm, vm.status.value(), tick?.top?.firstOrNull { it.uid == 0 && it.name == "zhord" }?.ma ?: 0.0, liveMode = !vm.fileMode)
        }
    }
}

@Composable
private fun LiveHero(l: Live, file: Boolean) {
    val charging = l.drawMa < 0 || l.plugged
    Section(if (charging) "Заряжается" else "Расход сейчас", sub = if (file) "снимок из файла live.json, ${Fmt.human(l.ticks.lastOrNull()?.t ?: 0)}" else "ток батареи, обновляется каждые 3 с", help = "ma", icon = Icons.Outlined.Bolt) {
        Row(verticalAlignment = Alignment.Bottom) {
            AnimatedNumber(kotlin.math.abs(l.drawMa), { Fmt.ma(it) }, MaterialTheme.typography.displayLarge)
            Text(" мА", Modifier.padding(bottom = 12.dp), style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.weight(1f))
            Column(horizontalAlignment = Alignment.End, modifier = Modifier.padding(bottom = 8.dp)) {
                Text("${Fmt.d2(kotlin.math.abs(l.drawMa) * l.voltageMv / 1e6)} Вт", style = MaterialTheme.typography.titleMedium.merge(NumStyle))
                Text(if (l.screenOn) "экран вкл." + (l.bright?.let { " · ярк. ${Fmt.pctRaw(it * 100)} %" } ?: "") else "экран выкл.", style = MaterialTheme.typography.bodySmall)
            }
        }
        val verdict = when {
            charging -> null
            !l.screenOn && l.drawMa < 25 -> "спит — так и должно быть" to Lvl.GOOD
            !l.screenOn -> "экран выключен, но телефон бодрствует" to Lvl.WARN
            l.drawMa < 500 -> "обычный расход с включённым экраном" to Lvl.GOOD
            l.drawMa < 1200 -> "высокий: тяжёлое приложение, связь или камера" to Lvl.WARN
            else -> "очень высокий: игра, камера, навигация" to Lvl.BAD
        }
        AnimatedContent(verdict, transitionSpec = { fadeIn(tween(200)) togetherWith fadeOut(tween(150)) }, label = "v") { v ->
            if (v != null) LevelPill(v.second, v.first)
        }
        Spacer(Modifier.height(12.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile("Заряд", "${l.level}", Modifier.weight(1f), unit = "%", sub = if (l.chargeUah > 0) "${Fmt.d0(l.chargeUah / 1000.0)} мА·ч" else null)
            MetricTile("Нагрев", Fmt.d1(l.temp), Modifier.weight(1f), unit = "°C", lvl = Norms.temp(l.temp), sub = "норма < 38°")
            MetricTile("Напряжение", Fmt.d2(l.voltageMv / 1000), Modifier.weight(1f), unit = "В")
        }
        if (!charging && l.drawMa > 1 && l.chargeUah > 0) {
            Explain("При таком токе оставшегося заряда хватило бы на ${Fmt.dur((l.chargeUah / 1000.0 / l.drawMa * 3_600_000).toLong())}. Ток скачет — для оценок смотрите средний на «Сводке».")
        }
    }
}

@Composable
private fun TickInfo(t: LiveTick, selected: Boolean, onReset: () -> Unit) {
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).padding(vertical = 2.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                (if (selected) "Точка " else "Последний шаг ") + Fmt.clockSec(t.t),
                Modifier.weight(1f), style = MaterialTheme.typography.titleSmall.merge(NumStyle),
            )
            Pill(if (t.plugged) "зарядка" else if (t.screen) "экран вкл." else "экран выкл.", if (t.plugged) Pal.plug else if (t.screen) Pal.screen else Pal.off)
            if (selected) TextButton(onClick = onReset) { Text("к последней") }
        }
        Spacer(Modifier.height(6.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile("Замер", Fmt.ma(t.drawMa), Modifier.weight(1f), unit = "мА")
            MetricTile("Модель", Fmt.ma(t.modelMa), Modifier.weight(1f), unit = "мА", sub = "CPU ${Fmt.ma(t.cpuMa)}")
            MetricTile("Заряд", "${t.level}", Modifier.weight(1f), unit = "%", sub = if (t.dtMs > 0) "шаг ${Fmt.d1(t.dtMs / 1000.0)} с" else null)
        }
        val diff = t.drawMa - t.modelMa
        if (!t.plugged && diff > 80) Explain("Замер выше модели на ${Fmt.ma(diff)} мА: работает то, что модель не видит, — радио, GPS, GPU, камера.")
        if (!selected) Text(
            "Коснитесь графика или проведите по нему — покажу любую точку.",
            Modifier.padding(top = 6.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
