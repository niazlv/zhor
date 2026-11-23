@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)

package dev.niazlv.zhor

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AccountTree
import androidx.compose.material.icons.outlined.Alarm
import androidx.compose.material.icons.outlined.BugReport
import androidx.compose.material.icons.outlined.DonutLarge
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.automirrored.outlined.HelpCenter
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.Memory
import androidx.compose.material.icons.outlined.NetworkCheck
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material.icons.outlined.Sensors
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material.icons.outlined.TipsAndUpdates
import androidx.compose.material.icons.outlined.WorkOutline
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@Composable
fun AppDetailScreen(vm: Vm, navi: Navi, uid: Int, seg: String?) {
    val r = vm.reportFor(seg)
    val a = r?.app(uid)
    val segInfo = seg?.let { vm.segs[it] }
    Scaffold(
        topBar = {
            DetailTopBar(
                a?.let { vm.labelOf(it) } ?: "Приложение",
                if (segInfo != null) "отрезок ${Fmt.span(segInfo.from, segInfo.to)}" else r?.label,
                onBack = { navi.back() },
            )
        },
    ) { pad ->
        when {
            r == null -> Column(Modifier.padding(pad)) { SkeletonScreen() }
            a == null -> Column(Modifier.padding(pad)) {
                EmptyState(Icons.Outlined.SearchOff, "Нет данных в этом периоде", "За выбранный период это приложение ничего не потратило (или ещё не попало в отчёт).")
            }
            else -> AppDetailContent(vm, navi, r, a, seg, Modifier.padding(pad))
        }
    }
}

@Composable
private fun AppDetailContent(vm: Vm, navi: Navi, r: Report, a: App, seg: String?, modifier: Modifier) {
    val total = r.totals.measuredMah.takeIf { it > 0 } ?: r.apps.sumOf { it.mah }
    val rank = r.apps.indexOfFirst { it.uid == a.uid } + 1
    LazyColumn(modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
        item(key = "hero") { Hero(vm, a, total, rank, r.apps.size) }
        val ins = Insights.forApp(r, a.uid)
        if (ins.isNotEmpty()) item(key = "ins") {
            Section("Выводы ядра") { ins.forEach { InsightCard(humanize(it, r, vm), null) } }
        }
        // «Android OS»: за кого работает system и включённые спецвозможности — сразу под главным
        val sw = r.systemWork
        if (a.uid == 1000 && sw != null) {
            item(key = "syswork") { SystemWorkCard(r, sw, vm, navi, seg) }
            if (sw.a11y.isNotEmpty()) item(key = "a11y") { A11yCard(sw, vm) }
        }
        item(key = "comp") { Composition(a, r) }
        if (a.states.values.any { it > 0.005 }) item(key = "states") { States(a) }
        if (a.uid != 1000 && (a.systemMah > 0 || a.systemMs > 0)) item(key = "sysload") { SystemLoadCard(a, r, vm, navi, seg) }
        if (a.why.isNotEmpty()) item(key = "why") {
            Section("Почему столько", sub = "разбор ядра по убыванию веса", icon = Icons.AutoMirrored.Outlined.HelpCenter) {
                a.why.forEachIndexed { i, w ->
                    Row(Modifier.padding(vertical = 4.dp)) {
                        Text("${i + 1}", Modifier.width(22.dp), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
                        Text(vm.nice(w, r), style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
        }
        item(key = "time") { AppTimelineSection(vm, navi, r, a) }
        item(key = "todo") { Advice(a, r) }
        if (a.processes.isNotEmpty()) {
            item(key = "ph") {
                Row(Modifier.padding(start = 20.dp, end = 12.dp, top = 14.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.AccountTree, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(8.dp))
                    Column(Modifier.weight(1f)) {
                        Text("Процессы и потоки", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                        Text("нажмите на процесс, чтобы раскрыть все потоки", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    HelpButton("processes")
                }
            }
            val maxP = a.processes.maxOf { it.mah }.coerceAtLeast(0.001)
            items(a.processes, key = { "p" + it.name + it.uid }) { p -> ProcCard(p, maxP, a.uid) }
        }
        val lists = listOf(
            Triple("Wakelock — не давал уснуть", "wakelock", a.wakelocks),
            Triple("Будильники с пробуждением", "alarm", a.alarms),
            Triple("Фоновые задания", "job", a.jobs),
            Triple("Синхронизация", "sync", a.syncs),
            Triple("Датчики", "sensor", a.sensors),
            Triple("Нативные wakelock", "native_wakelock", a.nativeWakelocks),
        )
        val icons = listOf(Icons.Outlined.Lock, Icons.Outlined.Alarm, Icons.Outlined.WorkOutline, Icons.Outlined.Sync, Icons.Outlined.Sensors, Icons.Outlined.Lock)
        lists.forEachIndexed { i, (title, help, l) ->
            if (l.isNotEmpty()) item(key = "l$help") {
                val sub = when (help) {
                    "wakelock" -> "всего ${Fmt.dur(l.sumOf { it.ms })}, ${Fmt.times(l.sumOf { it.count })}"
                    "alarm" -> "разбудил телефон ${Fmt.times(l.sumOf { it.count })}"
                    else -> "${l.size} шт."
                }
                Section(title, sub = sub, help = help, icon = icons[i]) { ItemList(l, Pal.wakelock, showMs = help != "alarm", key = "al$help$seg") }
            }
        }
        a.net?.let { n -> if (n.total > 0 || n.mobileWakeups + n.wifiWakeups > 0) item(key = "net") { NetCard(n) } }
        if (a.gpuMs > 0 || a.crashes + a.anrs > 0 || a.sysMah > 0) item(key = "misc") {
            Section("Прочее", icon = Icons.Outlined.BugReport) {
                if (a.gpuMs > 0) KV("GPU занят", Fmt.dur(a.gpuMs), help = "gpu")
                if (a.crashes + a.anrs > 0) KV("Падения / зависания (ANR)", "${a.crashes} / ${a.anrs}", Lvl.WARN, help = "crashes")
                if (a.sysMah > 0) KV("Итог по модели Android", "${Fmt.mah(a.sysMah)} мА·ч", help = "android_model")
                if (a.sysMah > 0 && a.cpuMah > 0) {
                    Explain("Ядро Жора посчитало процессор как ${Fmt.mah(a.cpuMah)} мА·ч, Android — ${Fmt.mah(a.components["cpu"] ?: 0.0)} мА·ч. Расхождение нормально: ядро видит потоки и частоты, Android — только суммарное время.")
                }
            }
        }
        item(key = "ids") {
            Section("Пакеты и uid", sub = "uid ${a.uid} · ${Fmt.kindName(a.kind)}") {
                if (a.packages.isEmpty()) Text("Без пакетов: системный uid.", style = MaterialTheme.typography.bodySmall)
                MoreList(a.packages, initial = 4, key = "pk${a.uid}") {
                    Text(it, Modifier.padding(vertical = 2.dp), fontFamily = FontFamily.Monospace, fontSize = 13.sp)
                }
                if (a.uid / 100000 > 0) Explain("Приложение во втором профиле пользователя (${a.uid / 100000}) — например, «Второе пространство» или рабочий профиль.")
            }
        }
    }
}

@Composable
private fun Hero(vm: Vm, a: App, total: Double, rank: Int, count: Int) {
    val ctx = LocalContext.current
    Section(Fmt.kindName(a.kind).replaceFirstChar { it.uppercase() }, sub = if (rank > 0) "№$rank из $count по расходу" else null, help = "app_total") {
        Row(verticalAlignment = Alignment.CenterVertically) {
            AppIcon(vm, a, 56.dp)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.Bottom) {
                    AnimatedNumber(a.mah, { Fmt.mah(it) }, MaterialTheme.typography.displaySmall)
                    Text(" мА·ч", Modifier.padding(bottom = 6.dp))
                }
                if (total > 0) Text(
                    "${Fmt.pct(100 * a.mah / total)} всего расхода за период",
                    style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Spacer(Modifier.height(10.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile("Процессор", Fmt.mah(a.cpuBest), Modifier.weight(1f), unit = "мА·ч", sub = "${Fmt.dur(a.cpuMs)} CPU-времени")
            val offShare = if (a.cpuMah > 0) a.cpuMahScreenOff / a.cpuMah else 0.0
            val lvl = when {
                a.cpuMahScreenOff < 0.3 -> Lvl.GOOD
                offShare > 0.5 -> Lvl.WARN
                else -> Lvl.NONE
            }
            MetricTile(
                "При выкл. экране", Fmt.mah(a.cpuMahScreenOff), Modifier.weight(1f), unit = "мА·ч", lvl = lvl,
                sub = if (a.cpuMah > 0) "${Fmt.pctRaw(offShare * 100)} % его процессора" else null,
            )
        }
        val pkg = a.packages.firstOrNull()
        if (a.kind == "app" && pkg != null) {
            Spacer(Modifier.height(10.dp))
            FilledTonalButton(onClick = {
                runCatching {
                    ctx.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$pkg")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                }
            }, Modifier.fillMaxWidth()) {
                Icon(Icons.AutoMirrored.Outlined.OpenInNew, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("Настройки приложения: батарея и фон")
            }
        }
    }
}

@Composable
private fun Composition(a: App, r: Report) {
    Section("Из чего состоит", sub = "процессор — ядро Жора, остальное — модель Android", help = "app_total", icon = Icons.Outlined.DonutLarge) {
        val parts = buildList {
            add(Triple("cpu", a.cpuBest, a.cpuMs))
            a.components.filter { it.key != "cpu" && it.value >= 0.005 }.toList().sortedByDescending { it.second }
                .forEach { (k, v) -> add(Triple(k, v, a.componentMs[k] ?: 0L)) }
        }
        val sum = parts.sumOf { it.second }.coerceAtLeast(0.001)
        StackBar(parts.map { Pal.comp(it.first) to it.second }, height = 14.dp)
        Spacer(Modifier.height(6.dp))
        parts.forEach { (k, v, ms) ->
            LegendRow(
                Pal.comp(k), Fmt.compName(k).replaceFirstChar { it.uppercase() } + if (k == "cpu" && a.cpuMah > 0) " (ядро)" else "",
                "${Fmt.mah(v)} мА·ч", Fmt.pct(100 * v / sum), sub = if (ms > 0) "активно ${Fmt.dur(ms)}" else null,
            )
        }
        val screen = a.components["screen"] ?: 0.0
        if (screen > 0.5 * sum) Explain("Больше половины — экран, пока приложение было на первом плане. Это расход на использование, а не фон.")
        r.sys?.let { s -> if (a.components.isNotEmpty()) Explain("Компоненты Android — за ${Fmt.span(s.from, s.to)}.") }
    }
}

@Composable
private fun States(a: App) {
    val order = listOf("fg", "fgs", "bg", "cached")
    val st = order.mapNotNull { k -> a.states[k]?.takeIf { it > 0.005 }?.let { k to it } } +
        a.states.filter { it.key !in order && it.value > 0.005 }.toList()
    val sum = st.sumOf { it.second }.coerceAtLeast(0.001)
    Section("Когда тратил", sub = "по состояниям приложения (Android)", help = "states", icon = Icons.Outlined.Schedule) {
        StackBar(st.map { Pal.state(it.first) to it.second }, height = 12.dp)
        Spacer(Modifier.height(6.dp))
        st.forEach { (k, v) -> LegendRow(Pal.state(k), Fmt.stateName(k).replaceFirstChar { it.uppercase() }, "${Fmt.mah(v)} мА·ч", Fmt.pct(100 * v / sum)) }
        val bg = (a.states["bg"] ?: 0.0) + (a.states["cached"] ?: 0.0)
        if (a.kind != "app") Explain("Системные службы работают в фоне всегда — это нормально. Важно не «когда», а по чьему заказу: см. «За кого работает system».")
        else if (bg > 0.5 * sum && bg > 0.5) Explain("Большая часть — в фоне, без вас. Если вы этим приложением в период не пользовались — его стоит ограничить.")
        val fgs = a.states["fgs"] ?: 0.0
        if (fgs > 0.3 * sum) Explain("Заметная часть — фоновая служба с уведомлением: музыка, навигация, загрузки или постоянное подключение.")
    }
}

@Composable
private fun Advice(a: App, r: Report) {
    val tips = buildList {
        val wlMs = a.wakelocks.sumOf { it.ms }
        val alarms = a.alarms.sumOf { it.count }
        val hours = (r.to - r.from).coerceAtLeast(1) / 3_600_000.0
        val sys = a.kind != "app"
        if (sys) {
            add("Это системная часть — её нельзя ограничить. Её расход обычно следствие чужой работы: binder-потоки отвечают на запросы приложений, сетевые — на трафик.")
            if (a.uid == 0) add("Здесь же сам Жор (процесс zhord) — его цену видно в процессах ниже.")
        } else {
            val screen = a.components["screen"] ?: 0.0
            if (a.cpuMahScreenOff > 1 || (a.bgMah > 1 && a.bgMah > 0.5 * a.mah) || wlMs > 60_000 || alarms / hours > 10) {
                add("Ограничьте работу в фоне: «Настройки приложения» → Батарея → «Ограничить» (на Xiaomi — «Контроль активности»).")
            }
            if (alarms / hours > 10) add("Будит телефон будильниками ${Fmt.times(alarms)} (~${Fmt.d0(alarms / hours)} в час). Отключите в нём лишние уведомления, обновления виджетов или синхронизацию.")
            if (wlMs > 60_000) add("Держал телефон без сна ${Fmt.dur(wlMs)}. Если это не плеер и не навигация — это лишнее.")
            val bgNet = (a.net?.mobileBg ?: 0) + (a.net?.wifiBg ?: 0)
            if (bgNet > 20L shl 20) add("В фоне передал ${Fmt.bytes(bgNet)} — проверьте автозагрузку и резервное копирование.")
            if (a.crashes > 0) add("Падало ${Fmt.times(a.crashes)} — перезапуски по кругу тратят заряд. Обновите приложение или очистите его данные.")
            if (screen > 0.5 * a.mah) add("Почти всё — экран при использовании. Снизить можно яркостью и тёмной темой.")
        }
        if (isEmpty()) add("Ничего подозрительного: расход соответствует тому, как приложение использовалось.")
    }
    Section("Что с этим делать", icon = Icons.Outlined.TipsAndUpdates) {
        tips.forEach { t ->
            Row(Modifier.padding(vertical = 3.dp)) {
                Text("•", Modifier.width(16.dp), color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
                Text(t, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

@Composable
private fun ProcCard(p: Proc, max: Double, appUid: Int) {
    var open by rememberSaveable(p.name + p.uid) { mutableStateOf(false) }
    val rot by animateFloatAsState(if (open) 180f else 0f, label = "chev")
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp).clip(RoundedCornerShape(18.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerLow).clickable { open = !open }
            .padding(14.dp).animateContentSize(),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(p.name, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.padding(top = 2.dp)) {
                    if (p.isolated) Pill("песочница / WebView", Pal.wifi)
                    if (p.uid != appUid) Pill("uid ${p.uid}", Pal.other)
                    if (p.starts > 0) Pill("запусков ${p.starts}", if (p.starts > 20) Pal.cpu else Pal.other)
                }
            }
            Text("${Fmt.mah(p.mah)} мА·ч", style = MaterialTheme.typography.bodyLarge.merge(NumStyle), fontWeight = FontWeight.SemiBold)
            Icon(Icons.Outlined.ExpandMore, null, Modifier.rotate(rot), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.height(6.dp))
        ShareBar((p.mah / max).toFloat(), Pal.cpu)
        Text(
            "CPU ${Fmt.dur(p.cpuMs)}" + (if (p.mahScreenOff > 0.001) " · при выкл. экране ${Fmt.mah(p.mahScreenOff)} мА·ч" else ""),
            Modifier.padding(top = 4.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        p.explain?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
        if (p.isolated) Text("Изолированный процесс (рендер WebView или песочница) отнесён к этому приложению.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (p.starts > 20) Text("Часто перезапускается: его будят пуши или будильники, или он падает.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (p.threads.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            val maxT = p.threads.maxOf { it.mah }.coerceAtLeast(0.001)
            p.threads.take(3).forEach { ThreadRow(it, maxT, open) }
            AnimatedVisibility(open && p.threads.size > 3, enter = expandVertically() + fadeIn(), exit = shrinkVertically() + fadeOut()) {
                Column { p.threads.drop(3).forEach { ThreadRow(it, maxT, true) } }
            }
            if (!open && p.threads.size > 3) {
                Text("ещё ${p.threads.size - 3} потоков и пояснения", Modifier.padding(top = 4.dp), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
            }
        }
    }
}

@Composable
private fun ThreadRow(t: Thr, max: Double, explain: Boolean) {
    Column(Modifier.padding(vertical = 3.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(Fmt.threadName(t.name), Modifier.weight(1f), fontFamily = FontFamily.Monospace, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text("${Fmt.mah(t.mah)} · ${Fmt.dur(t.cpuMs)}", style = MaterialTheme.typography.bodySmall.merge(NumStyle))
        }
        ShareBar((t.mah / max).toFloat(), Pal.cpu.copy(alpha = 0.75f), height = 4.dp)
        if (explain) t.explain?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
    }
}

@Composable
private fun NetCard(n: Net) {
    Section("Сеть", sub = "трафик и пробуждения от пакетов", help = "network", icon = Icons.Outlined.NetworkCheck) {
        val parts = listOf(
            Pal.radio to (n.mobileRx + n.mobileTx).toDouble(),
            Pal.wifi to (n.wifiRx + n.wifiTx).toDouble(),
        )
        if (n.total > 0) {
            StackBar(parts, height = 10.dp)
            Spacer(Modifier.height(6.dp))
        }
        KV("Мобильная сеть", "↓ ${Fmt.bytes(n.mobileRx)}  ↑ ${Fmt.bytes(n.mobileTx)}")
        KV("Wi-Fi", "↓ ${Fmt.bytes(n.wifiRx)}  ↑ ${Fmt.bytes(n.wifiTx)}")
        KV("Из них в фоне", "моб. ${Fmt.bytes(n.mobileBg)} · Wi-Fi ${Fmt.bytes(n.wifiBg)}")
        if (n.mobileActiveMs > 0) KV("Модем активен ради него", "${Fmt.dur(n.mobileActiveMs)}, ${Fmt.times(n.mobileActiveCount)}", help = "radio_active")
        val wk = n.mobileWakeups + n.wifiWakeups
        if (wk > 0) KV("Пакеты будили телефон", Fmt.times(wk), if (wk > 100) Lvl.WARN else Lvl.NONE)
        if (wk > 100) Explain("Частые входящие пакеты при выключенном экране — push-соединение или keepalive. Мессенджеры и VPN — частая причина плохого сна.")
    }
}

