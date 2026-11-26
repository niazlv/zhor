@file:OptIn(ExperimentalMaterial3Api::class)

package dev.niazlv.zhor

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.CompareArrows
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.ErrorOutline
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.outlined.NotificationsOff
import androidx.compose.material.icons.outlined.Rule
import androidx.compose.material.icons.outlined.Bedtime
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material.icons.outlined.Block
import androidx.compose.material.icons.outlined.SettingsBackupRestore
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.app.ActivityCompat
import androidx.lifecycle.compose.LifecycleResumeEffect

/** Человеческое имя правила сторожа. */
fun ruleName(rule: String) = when (rule) {
    "no_sleep" -> "Телефон не спит"
    "idle_drain" -> "Батарея тает в покое"
    "wakeups" -> "Частые пробуждения"
    "app_bg" -> "Работает в фоне"
    "hot" -> "Греется с выключенным экраном"
    "usb_flap" -> "USB-порт мигает"
    "test" -> "Проверка"
    else -> rule
}

/** «app_bg:10123» → «Работает в фоне: Телеграм». */
private fun muteName(vm: Vm, key: String): String {
    val rule = key.substringBefore(':')
    val who = key.substringAfter(':', "")
    if (who.isEmpty()) return ruleName(rule)
    val name = if (rule == "app_bg") who.toIntOrNull()?.let { vm.labelOfUid(it, "uid $it") } ?: who else who
    return "${ruleName(rule)}: $name"
}

/** Текст тревоги с человеческим именем приложения вместо пакета. */
fun alertText(vm: Vm, a: WatchAlert, s: String): String {
    val p = a.pkg ?: return s
    return if (s.contains(p)) Notify.replacePkg(s, p, vm.labelOfPackage(p)) else s
}

@Composable
fun WatchScreen(vm: Vm, navi: Navi) {
    LaunchedEffect(Unit) {
        vm.loadAlerts()
        vm.loadConfig()
        vm.loadActions(null)
        vm.loadRestricted()
    }
    // вернулись из настроек уведомлений — перечитать разрешение
    LifecycleResumeEffect(Unit) {
        vm.refreshNotifState()
        onPauseOrDispose { }
    }
    Scaffold(topBar = { DetailTopBar("Сторож", "тревоги в момент события и «до/после»", onBack = { navi.back() }) }) { pad ->
        LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
            item(key = "head") { WatchHead(vm) }
            item(key = "rules") { RulesCard(vm) }
            item(key = "alerts") { AlertsCard(vm, navi) }
            val mute = vm.config.value()?.watchMute ?: vm.alerts.value()?.mute.orEmpty()
            if (mute.isNotEmpty()) item(key = "mute") { MuteCard(vm, mute) }
            item(key = "cmp") { BeforeAfterCard(vm, navi) }
            item(key = "changes") { SystemChangesCard(vm) }
        }
    }
}

private tailrec fun Context.activity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.activity()
    else -> null
}

/**
 * Включить уведомления: системный запрос разрешения, а если Android его больше не показывает (после двух
 * отказов) или разрешение есть, но уведомления выключены в настройках, — сразу системные настройки уведомлений.
 */
@Composable
fun rememberNotifRequest(vm: Vm): () -> Unit {
    val ctx = LocalContext.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { vm.refreshNotifState() }
    return {
        val act = ctx.activity()
        when {
            Notify.permitted(ctx) -> Notify.openSettings(ctx)
            Build.VERSION.SDK_INT >= 33 && (!vm.notifAsked || (act != null && ActivityCompat.shouldShowRequestPermissionRationale(act, Manifest.permission.POST_NOTIFICATIONS))) -> {
                vm.notifAsked = true
                launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
            else -> Notify.openSettings(ctx)
        }
    }
}

@Composable
private fun WatchHead(vm: Vm) {
    val cfg = vm.config.value()
    val askNotif = rememberNotifRequest(vm)
    val unsupported = (vm.alerts as? Res.Err)?.unsupported == true || (cfg != null && cfg.watch == null)
    Section("Сторож", sub = "ядро само скажет, когда смотреть", icon = Icons.Outlined.NotificationsActive) {
        if (vm.fileMode) {
            Note(Lvl.INFO, "Сторож работает в ядре — нужен root.")
            return@Section
        }
        if (unsupported) {
            Note(Lvl.INFO, "Сторож появится с ядром 0.6. Перезапустите приложение — оно поставит новое ядро.")
            return@Section
        }
        val on = cfg?.watch ?: vm.alerts.value()?.watch ?: true
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Следить и присылать уведомления", style = MaterialTheme.typography.bodyLarge)
                Text(
                    "Ядро проверяет правила раз в 5 минут на своём обычном шаге — только пока телефон и так не спит. " +
                        "Само оно телефон не будит и почти ничего не стоит.",
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.width(8.dp))
            Switch(
                checked = on, enabled = cfg != null && !vm.configSaving,
                onCheckedChange = {
                    vm.saveConfig(cfg!!.copy(watch = it))
                    if (it && !vm.notifAllowed) askNotif()
                },
            )
        }
        if (on && !vm.notifAllowed) {
            Spacer(Modifier.height(10.dp))
            Note(Lvl.WARN, "Уведомления Жору запрещены: тревоги будут копиться только в списке ниже.")
            FilledTonalButton(onClick = askNotif, Modifier.padding(top = 6.dp)) {
                Icon(Icons.Outlined.NotificationsActive, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("Разрешить уведомления")
            }
        }
        if (on) {
            OutlinedButton(onClick = { vm.alertTest() }, Modifier.padding(top = 8.dp)) { Text("Прислать проверочное") }
            Explain("Проверочное идёт тем же путём, что и настоящие: от ядра через рассылку в приложение. Если не пришло — проверьте, что в настройках уведомлений Жора включён канал «Сторож».")
        }
    }
}

@Composable
private fun RulesCard(vm: Vm) {
    val st = vm.config
    val cfg = st.value() ?: return
    if (cfg.watch == null) return
    val editable = !vm.configSaving && cfg.watch
    fun r(v: Double) = Math.round(v)
    Section("Правила", sub = "о чём и с какого порога тревожить", icon = Icons.Outlined.Rule) {
        Choice(
            "Телефон не спит", "Экран выключен 30+ мин, а в глубоком сне меньше этой доли времени. Норма — больше 90 %.",
            listOf(Opt("выкл", 0), Opt("< 30 %", 30), Opt("< 50 %", 50), Opt("< 70 %", 70)), r(cfg.watchSleepPct), editable,
        ) { vm.saveConfig(cfg.copy(watchSleepPct = it.toDouble())) }
        Choice(
            "Батарея тает в покое", "Экран выключен час и больше, а разряд выше порога. Норма — меньше 1 %/ч.",
            listOf(Opt("выкл", 0), Opt("> 2 %/ч", 2), Opt("> 3 %/ч", 3), Opt("> 5 %/ч", 5)), r(cfg.watchDrainPctH), editable,
        ) { vm.saveConfig(cfg.copy(watchDrainPctH = it.toDouble())) }
        Choice(
            "Частые пробуждения", "Один источник будит процессор чаще, чем столько раз в час (за 30+ мин покоя).",
            listOf(Opt("выкл", 0), Opt("> 300", 300), Opt("> 600", 600), Opt("> 1500", 1500)), r(cfg.watchWakeupsH), editable,
        ) { vm.saveConfig(cfg.copy(watchWakeupsH = it.toDouble())) }
        Choice(
            "Приложение работает в фоне", "За час и больше покоя приложение в среднем тратит на процессор больше порога.",
            listOf(Opt("выкл", 0), Opt("> 5 мА", 5), Opt("> 10 мА", 10), Opt("> 25 мА", 25)), r(cfg.watchAppMa), editable,
        ) { vm.saveConfig(cfg.copy(watchAppMa = it.toDouble())) }
        Choice(
            "Греется в покое", "Батарея горячее порога все последние 10 минут, хотя экран выключен.",
            listOf(Opt("выкл", 0), Opt("38 °C", 38), Opt("40 °C", 40), Opt("43 °C", 43)), r(cfg.watchHotC), editable,
        ) { vm.saveConfig(cfg.copy(watchHotC = it.toDouble())) }
        Explain("Покой — экран выключен и телефон не на зарядке. Каждое правило срабатывает не больше раза за период покоя.")
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun AlertsCard(vm: Vm, navi: Navi) {
    val st = vm.alerts
    val mute = vm.config.value()?.watchMute ?: st.value()?.mute.orEmpty()
    Section("Тревоги", sub = "последние две недели", icon = Icons.Outlined.WarningAmber) {
        when (st) {
            is Res.Ok -> {
                val list = st.v.list
                if (list.isEmpty()) {
                    Explain("Пока тихо: ни одно правило не сработало.")
                    if (st.v.lastEval > 0) Explain("Последняя проверка — ${Fmt.human(st.v.lastEval)}.")
                }
                MoreList(list, initial = 5, key = "alerts") { a ->
                    AlertRow(vm, navi, a, muted = a.key in mute || a.rule in mute)
                }
            }
            is Res.Err -> Note(Lvl.INFO, st.msg)
            else -> {
                val b = shimmerBrush()
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) { repeat(3) { SkeletonLine(0.9f, 14.dp, b) } }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun AlertRow(vm: Vm, navi: Navi, a: WatchAlert, muted: Boolean) {
    val lvl = lvlOf(a.level)
    val icon = when (lvl) {
        Lvl.BAD -> Icons.Outlined.ErrorOutline
        Lvl.WARN -> Icons.Outlined.WarningAmber
        Lvl.GOOD -> Icons.Outlined.CheckCircle
        else -> Icons.Outlined.Info
    }
    Column(Modifier.fillMaxWidth().padding(vertical = 4.dp).clip(RoundedCornerShape(16.dp)).background(lvl.bg()).padding(horizontal = 12.dp, vertical = 10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, Modifier.size(20.dp), tint = lvl.color())
            Spacer(Modifier.width(8.dp))
            Text(alertText(vm, a, a.title), Modifier.weight(1f), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
            Text(Fmt.human(a.t), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Text(alertText(vm, a, a.text), Modifier.padding(top = 4.dp), style = MaterialTheme.typography.bodySmall)
        if (!a.sent && a.rule != "test") {
            Text("Уведомление не дошло до приложения", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.error)
        }
        if (a.rule != "test") FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            TextButton(onClick = { navi.seg(a.from, a.to) }) { Text("Отчёт за период") }
            if (a.uid != null && a.uid > 0) TextButton(onClick = {
                vm.loadSegment(a.from, a.to)
                navi.app(a.uid, vm.segKey(a.from, a.to))
            }) { Text("Приложение") }
            TextButton(onClick = { vm.mute(a.key, !muted) }) { Text(if (muted) "Напоминать снова" else "Не напоминать") }
        }
    }
}

@Composable
private fun MuteCard(vm: Vm, mute: List<String>) {
    Section("Не напоминать", sub = "приглушённые тревоги", icon = Icons.Outlined.NotificationsOff) {
        mute.forEach { k ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(muteName(vm, k), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                TextButton(onClick = { vm.mute(k, false) }) { Text("Вернуть") }
            }
        }
    }
}

@Composable
private fun BeforeAfterCard(vm: Vm, navi: Navi) {
    val log = vm.actionLog[-1].orEmpty()
    Section("До и после", sub = "помогло ли", icon = Icons.AutoMirrored.Outlined.CompareArrows) {
        NavRow(Icons.Outlined.Bedtime, "Два последних периода покоя", "например, эта ночь против прошлой") { navi.compare(CmpReq("rest")) }
        if (log.isEmpty()) {
            Explain("Ограничьте приложению работу в фоне в его карточке — здесь появится сравнение покоя до и после.")
        } else {
            Spacer(Modifier.height(6.dp))
            Text("Действия", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            log.take(10).forEach { a -> ActionRow(vm, navi, a) }
        }
    }
}

fun actionName(a: String) = when (a) {
    "restrict" -> "фон ограничен"
    "unrestrict" -> "фон разрешён"
    "stop" -> "остановлено"
    else -> a
}

@Composable
fun ActionRow(vm: Vm, navi: Navi, a: ActionRec, showApp: Boolean = true) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        if (showApp && a.uid != null) {
            UidIcon(vm, a.uid, a.pkg, 28.dp)
            Spacer(Modifier.width(10.dp))
        }
        Column(Modifier.weight(1f)) {
            if (showApp) Text(vm.labelOfPackage(a.pkg), style = MaterialTheme.typography.bodyMedium)
            Text(
                "${actionName(a.action)} · ${Fmt.human(a.t)}" + if (!a.ok) " · не вышло" else "",
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (a.ok && a.action != "unrestrict") TextButton(onClick = { navi.compare(CmpReq("action", a.t, a.uid)) }) { Text("До/после") }
    }
}

/** Полоска-пояснение с цветом уровня. */
@Composable
fun Note(lvl: Lvl, text: String) {
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(lvl.bg()).padding(12.dp)) {
        Icon(if (lvl == Lvl.WARN || lvl == Lvl.BAD) Icons.Outlined.WarningAmber else Icons.Outlined.Info, null, Modifier.size(18.dp), tint = lvl.color())
        Spacer(Modifier.width(8.dp))
        Text(text, style = MaterialTheme.typography.bodySmall)
    }
}

private fun bucketRu(n: String?) = when (n) {
    "exempted" -> "без ограничений (системное)"
    "active" -> "активное"
    "working_set" -> "рабочий набор"
    "frequent" -> "частое"
    "rare" -> "редкое"
    "restricted" -> "ограниченное"
    "never" -> "не запускалось"
    else -> null
}

/** Карточка приложения: можно ли ему работать в фоне, ограничить / остановить, «до/после». */
@Composable
fun BackgroundCard(vm: Vm, navi: Navi, a: App) {
    val pkg = a.packages.first()
    LaunchedEffect(a.uid, pkg) { vm.loadAppState(pkg, a.uid) }
    val st = vm.appStates["${a.uid}/$pkg"]
    val label = vm.labelOf(a)
    var confirm by rememberSaveable { mutableStateOf<String?>(null) }
    Section("Работа в фоне", sub = "ограничить и проверить, помогло ли", icon = Icons.Outlined.Block) {
        when (st) {
            is Res.Ok -> {
                val s = st.v
                KV(
                    "Сейчас", if (s.restricted) "ограничена" else if (s.bg == null) "неизвестно" else "разрешена",
                    lvl = if (s.restricted) Lvl.INFO else Lvl.NONE,
                    sub = bucketRu(s.bucketName)?.let { "корзина ожидания Android: $it" },
                )
                Row(Modifier.padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (s.restricted) {
                        OutlinedButton(onClick = { vm.act(pkg, a.uid, "unrestrict", label) }, enabled = !vm.acting) { Text("Разрешить фон") }
                    } else {
                        FilledTonalButton(onClick = { confirm = "restrict" }, enabled = !vm.acting) { Text("Ограничить фон") }
                    }
                    OutlinedButton(onClick = { confirm = "stop" }, enabled = !vm.acting) { Text("Остановить") }
                }
            }
            is Res.Err -> Note(Lvl.INFO, st.msg)
            else -> SkeletonLine(0.7f, 14.dp, shimmerBrush())
        }
        val log = vm.actionLog[a.uid].orEmpty()
        if (log.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            Text("Что делали", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            log.take(3).forEach { ActionRow(vm, navi, it, showApp = false) }
        }
        Explain("«Ограничить фон» — то же, что «Ограничено» в настройках батареи приложения: фоновые задания, будильники и сеть ждут, пока приложение не откроют. После ограничения Жор сравнит покой до и после.")
    }
    when (confirm) {
        "restrict" -> AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text("Ограничить работу в фоне?") },
            text = { Text("«$label» не сможет работать, пока его нет на экране: фоновые задания, будильники и сеть в фоне отложатся, обычные уведомления могут опаздывать. Вернуть можно здесь же.") },
            confirmButton = { TextButton(onClick = { confirm = null; vm.act(pkg, a.uid, "restrict", label) }) { Text("Ограничить") } },
            dismissButton = { TextButton(onClick = { confirm = null }) { Text("Отмена") } },
        )
        "stop" -> AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text("Остановить «$label»?") },
            text = { Text("Android завершит все процессы приложения. Пока вы его не откроете, оно не запускается само и не получает уведомления.") },
            confirmButton = { TextButton(onClick = { confirm = null; vm.act(pkg, a.uid, "stop", label) }) { Text("Остановить") } },
            dismissButton = { TextButton(onClick = { confirm = null }) { Text("Отмена") } },
        )
    }
}

/** На сводке: что сторож заметил за сутки. Ничего — карточки нет. */
@Composable
fun WatchSummary(vm: Vm, navi: Navi) {
    val list = vm.alerts.value()?.list.orEmpty()
    val day = System.currentTimeMillis() - 86_400_000
    val recent = list.filter { it.t >= day && it.rule != "test" }
    val last = recent.firstOrNull() ?: return
    Section(
        "Сторож заметил", sub = if (recent.size > 1) "${recent.size} ${Fmt.plural(recent.size.toLong(), "тревога", "тревоги", "тревог")} за сутки" else "за сутки",
        icon = Icons.Outlined.NotificationsActive, iconTint = lvlOf(last.level).color(),
    ) {
        InsightCard(Insight(last.level, alertText(vm, last, last.title), "${Fmt.human(last.t)} · ${alertText(vm, last, last.text)}", last.uid, null)) { navi.watch() }
    }
}

/** Что Жор поменял в системе и как это вернуть — кнопкой, без приложения и при его удалении. */
@Composable
fun SystemChangesCard(vm: Vm) {
    val list = vm.restricted
    var confirm by rememberSaveable { mutableStateOf(false) }
    Section("Что Жор меняет в системе", sub = "и как это вернуть", icon = Icons.Outlined.SettingsBackupRestore) {
        when {
            vm.fileMode -> Explain("Нужен root.")
            list == null -> SkeletonLine(0.6f, 14.dp, shimmerBrush())
            list.isEmpty() -> KV("Ограничено Жором", "ничего")
            else -> {
                KV("Ограничено Жором", "${list.size} ${Fmt.plural(list.size.toLong(), "приложение", "приложения", "приложений")}", lvl = Lvl.INFO)
                list.forEach { r ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
                        if (r.uid != null) {
                            UidIcon(vm, r.uid, r.pkg, 26.dp)
                            Spacer(Modifier.width(10.dp))
                        }
                        Text(vm.labelOfPackage(r.pkg) + if ((r.uid ?: 0) >= 100_000) " (польз. ${r.uid!! / 100_000})" else "", Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                        Text("с ${Fmt.human(r.since)}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                FilledTonalButton(onClick = { confirm = true }, enabled = !vm.acting, modifier = Modifier.padding(top = 6.dp)) {
                    Icon(Icons.Outlined.SettingsBackupRestore, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Вернуть всё")
                }
            }
        }
        Explain("«Остановить» — разовое: приложение оживает, как только его откроют. «Ограничить фон» — стандартное ограничение Android, его можно снять и в настройках батареи приложения. «Вернуть всё» вернёт режим, который был до Жора, включит пакеты, которые отключили профили, и откроет сеть тем, кому её закрыли; то, что вы с тех пор поменяли сами, не тронет.")
        Explain("Если удалить Жор, ядро через 10 минут само вернёт ограничения, выключит подробную историю Android, уберёт свой автозапуск и остановится. Записи останутся в ${Core.DIR}.")
        Explain("Отнять у Жора root мало: ядро уже работает от root и продолжит. Сначала нажмите «Вернуть всё» или удалите приложение. Без приложения то же делает команда zhord revert-all.")
    }
    if (confirm && !list.isNullOrEmpty()) AlertDialog(
        onDismissRequest = { confirm = false },
        title = { Text("Вернуть всё?") },
        text = { Text("Снова разрешить работу в фоне: ${list.joinToString { vm.labelOfPackage(it.pkg) }}. Вернётся режим, который был до Жора.") },
        confirmButton = { TextButton(onClick = { confirm = false; vm.revertAll() }) { Text("Вернуть") } },
        dismissButton = { TextButton(onClick = { confirm = false }) { Text("Отмена") } },
    )
}
