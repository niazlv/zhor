@file:OptIn(ExperimentalMaterial3Api::class)

package dev.niazlv.zhor

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.automirrored.outlined.MenuBook
import androidx.compose.material.icons.outlined.Memory
import androidx.compose.material.icons.outlined.PowerSettingsNew
import androidx.compose.material.icons.outlined.RestartAlt
import androidx.compose.material.icons.outlined.School
import androidx.compose.material.icons.outlined.Storage
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

@Composable
fun SettingsScreen(vm: Vm, navi: Navi) {
    LaunchedEffect(Unit) {
        vm.loadStatus()
        vm.loadConfig()
    }
    Scaffold(topBar = { DetailTopBar("Настройки", "ядро, сбор данных, выгрузка", onBack = { navi.back() }) }) { pad ->
        LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
            item(key = "core") { CoreCard(vm) }
            item(key = "cost") { SelfCostCard(vm, vm.status.value(), null) }
            item(key = "auto") {
                Section("Автозапуск", icon = Icons.Outlined.PowerSettingsNew, help = "coverage") {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text("Запускать ядро при загрузке телефона", style = MaterialTheme.typography.bodyLarge)
                            Text(
                                "Без автозапуска запись начнётся только когда откроете Жор, и после перезагрузки будет пробел в данных. " +
                                    "Скрипт: ${Core.BOOT} (KernelSU / Magisk).",
                                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        Spacer(Modifier.width(8.dp))
                        Switch(checked = vm.autostart, onCheckedChange = { vm.toggleAutostart(it) }, enabled = vm.ready && !vm.fileMode)
                    }
                }
            }
            item(key = "cfg") { ConfigCard(vm) }
            item(key = "nav") {
                Section("Ещё") {
                    NavRow(Icons.Outlined.Storage, "Данные и выгрузка", "место на диске, экспорт отчёта, очистка") { navi.data() }
                    NavRow(Icons.AutoMirrored.Outlined.MenuBook, "Как читать", "термины, нормы, что делать") { navi.guide() }
                    NavRow(Icons.Outlined.School, "Знакомство", "показать вступление ещё раз") { navi.onboarding() }
                }
            }
            item(key = "about") {
                Section("О приложении", icon = Icons.Outlined.Info) {
                    val ctx = LocalContext.current
                    val ver = remember { runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull() ?: "?" }
                    KV("Интерфейс", "Жор $ver")
                    vm.status.value()?.let { KV("Ядро", it.version) }
                    Explain("Жор не отправляет данные в сеть: всё хранится на телефоне в ${Core.DIR} и выгружается только по вашей кнопке.")
                }
            }
        }
    }
}

@Composable
private fun CoreCard(vm: Vm) {
    Section("Ядро zhord", sub = "root-демон, который собирает данные", icon = Icons.Outlined.Memory) {
        AnimatedContent(vm.status, transitionSpec = { fadeIn(tween(200)) togetherWith fadeOut(tween(120)) }, label = "st", contentKey = { it::class }) { st ->
            Column {
                when (st) {
                    is Res.Ok -> {
                        val s = st.v
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.size(10.dp).clip(CircleShape).background(Lvl.GOOD.color()))
                            Spacer(Modifier.width(8.dp))
                            Text("Работает ${Fmt.dur(System.currentTimeMillis() - s.started)}", style = MaterialTheme.typography.titleSmall)
                        }
                        Spacer(Modifier.height(6.dp))
                        KV("Версия", s.version + if (s.build > 0) " · сборка ${Fmt.time(s.build * 1000)}" else "")
                        KV("Записи с", if (s.oldest > 0) Fmt.human(s.oldest) else "—", help = "coverage")
                        if (s.intervalMs > 0) KV("Шаг сейчас", "раз в ${Fmt.d0(s.intervalMs / 1000.0)} с", sub = "при выключенном экране — реже и только пока телефон не спит")
                        KV("Профиль мощности", "", sub = s.profile)
                        KV("Кластеров процессора", s.clusters.toString(), help = "clusters")
                        KV("В памяти", "${s.buckets} отрезков · ${s.snaps} снимков", help = "snapshots")
                        KV("PID", s.pid.toString())
                    }
                    is Res.Err -> {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.size(10.dp).clip(CircleShape).background(Lvl.BAD.color()))
                            Spacer(Modifier.width(8.dp))
                            Text("Не отвечает", style = MaterialTheme.typography.titleSmall)
                        }
                        Text(st.msg, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                    }
                    else -> {
                        val b = shimmerBrush()
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) { repeat(4) { SkeletonLine(if (it % 2 == 0) 0.8f else 0.6f, 12.dp, b) } }
                    }
                }
            }
        }
        Spacer(Modifier.height(10.dp))
        OutlinedButton(onClick = { vm.restartCore() }, enabled = vm.ready && !vm.fileMode && vm.status !is Res.Loading) {
            Icon(Icons.Outlined.RestartAlt, null, Modifier.size(18.dp))
            Spacer(Modifier.width(6.dp))
            Text("Перезапустить ядро")
        }
        Explain("Перезапуск нужен, только если ядро зависло. Записи не теряются.")
    }
}

private data class Opt(val label: String, val value: Long)

@Composable
private fun ConfigCard(vm: Vm) {
    val st = vm.config
    val cfg = st.value() ?: CoreConfig.DEFAULT
    val editable = st is Res.Ok && !vm.configSaving
    Section("Сбор данных", sub = "как часто ядро шагает и сколько хранит", icon = Icons.Outlined.Tune, help = "scan_cost") {
        if (st is Res.Err) {
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(Lvl.INFO.bg()).padding(12.dp),
            ) {
                Icon(Icons.Outlined.Info, null, Modifier.size(18.dp), tint = Lvl.INFO.color())
                Spacer(Modifier.width(8.dp))
                Text(st.msg, style = MaterialTheme.typography.bodySmall)
            }
            Spacer(Modifier.height(10.dp))
        }
        val a by animateFloatAsState(if (editable) 1f else 0.55f, label = "a")
        Column(Modifier.alpha(a)) {
            Choice(
                "Шаг при включённом экране", "Чаще — подробнее лента и «Сейчас», но сам Жор тратит больше.",
                listOf(Opt("2 с", 2000), Opt("5 с", 5000), Opt("10 с", 10000), Opt("30 с", 30000)), cfg.intervalOnMs, editable,
            ) { vm.saveConfig(cfg.copy(intervalOnMs = it)) }
            Choice(
                "Шаг при выключенном экране", "Считается только время бодрствования: спящий телефон ядро не будит.",
                listOf(Opt("10 с", 10000), Opt("20 с", 20000), Opt("1 мин", 60000), Opt("2 мин", 120000)), cfg.intervalOffMs, editable,
            ) { vm.saveConfig(cfg.copy(intervalOffMs = it)) }
            Choice(
                "Хранить записи", "Старше — удаляются сами. День записей — несколько мегабайт.",
                listOf(Opt("3 д", 3), Opt("7 д", 7), Opt("10 д", 10), Opt("14 д", 14), Opt("30 д", 30)), cfg.keepDays.toLong(), editable,
            ) { vm.saveConfig(cfg.copy(keepDays = it.toInt())) }
            Choice(
                "Снимок статистики Android", "Снимок batterystats стоит 1–3 с процессора; реже — дешевле, но грубее «почему» у приложений.",
                listOf(Opt("15 мин", 15), Opt("30 мин", 30), Opt("1 ч", 60), Opt("2 ч", 120)), cfg.snapEveryMin.toLong(), editable,
            ) { vm.saveConfig(cfg.copy(snapEveryMin = it.toInt())) }
            Row(Modifier.padding(top = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Подробная история Android", style = MaterialTheme.typography.bodyLarge)
                    Text(
                        "Android будет записывать каждый wakelock, будильники и запуски процессов — в ленте приложения " +
                            "видно, кто именно будил. Android сбрасывает это при перезагрузке, ядро включает снова. " +
                            "Дороже: событий в 4–5 раз больше (~10 тыс. в час), буфер Android хватает часов на 20 вместо 45, " +
                            "Жору — до ~10 МБ памяти и около 0,5 с работы раз в час на разбор и запись.",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.width(8.dp))
                Switch(checked = cfg.fullHistory, onCheckedChange = { vm.saveConfig(cfg.copy(fullHistory = it)) }, enabled = editable)
            }
        }
    }
}

@Composable
private fun Choice(title: String, explain: String, opts: List<Opt>, cur: Long, enabled: Boolean, onPick: (Long) -> Unit) {
    Column(Modifier.padding(vertical = 6.dp)) {
        Text(title, style = MaterialTheme.typography.bodyLarge)
        Text(explain, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.height(6.dp))
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            opts.forEachIndexed { i, o ->
                SegmentedButton(
                    selected = o.value == cur, onClick = { if (o.value != cur) onPick(o.value) }, enabled = enabled,
                    shape = SegmentedButtonDefaults.itemShape(i, opts.size), icon = {},
                ) { Text(o.label, maxLines = 1) }
            }
        }
    }
}

@Composable
fun NavRow(icon: ImageVector, title: String, sub: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).clickable(onClick = onClick).padding(vertical = 10.dp, horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = MaterialTheme.colorScheme.primary)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Icon(Icons.Outlined.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
