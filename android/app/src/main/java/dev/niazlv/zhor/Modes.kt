package dev.niazlv.zhor

import androidx.compose.foundation.background
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.draw.alpha
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.BatterySaver
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Rule
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Speed
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

// ---- (0.8) группы и профили ----

/** Правила профиля. */
object Levels {
    val all = listOf("none", "watch", "bg", "hard", "nonet", "disable")
    /** «все остальные» нельзя лишить сети и отключить — это весь телефон */
    val forRest = listOf("none", "watch", "bg", "hard")
    const val REST = "rest"
    const val KEEP = "keep"

    fun name(l: String?) = when (l) {
        "none" -> "Не трогать"
        "watch" -> "Только если много ест"
        "bg" -> "Запрет фона"
        "hard" -> "Жёстко"
        "nonet" -> "Без сети"
        "disable" -> "Отключить"
        else -> "Нет правила"
    }

    fun explain(l: String) = when (l) {
        "none" -> "Профиль его не ограничивает. Если приложение ещё и в группе с правилом построже — это правило сильнее."
        "watch" -> "Пусть работает, пока тратит немного. Если при выключенном экране ест больше порога и давно не открывалось — остановить и запретить фон до конца профиля, с уведомлением."
        "bg" -> "Задания, будильники и сеть в фоне ждут, пока приложение не откроют. Уведомления через Google обычно приходят, но могут опаздывать."
        "hard" -> "Запрет фона и остановка при каждом выключении экрана. Уведомлений не будет, пока его не откроют."
        "nonet" -> "Как «жёстко», и ещё без сети: приложение не сможет ничего отправить и получить, пока правило действует. Только для обычных приложений, не для системных с общим uid."
        "disable" -> "Пакет отключается (как «Отключить» в настройках приложения): не запускается вовсе и пропадает из меню, пока правило действует. Выключится профиль — Жор включит его обратно. Единственное, что можно сделать с системными пакетами Xiaomi с общим uid."
        else -> ""
    }

    fun color(l: String?): Color = when (l) {
        "none" -> Pal.plug
        "watch" -> Pal.wifi
        "bg" -> Pal.screen
        "hard" -> Pal.cpu
        "nonet" -> Pal.radio
        "disable" -> Pal.gnss
        else -> Pal.other
    }
}

private fun newId(prefix: String) = prefix + System.currentTimeMillis().toString(36)

@Composable
private fun MPill(text: String, color: Color, muted: Boolean = false) {
    Box(Modifier.clip(RoundedCornerShape(50)).background(color.copy(alpha = 0.18f)).padding(horizontal = 10.dp, vertical = 3.dp)) {
        Text(text, style = MaterialTheme.typography.labelMedium, color = if (muted) MaterialTheme.colorScheme.onSurfaceVariant else color, maxLines = 1)
    }
}

@Composable
fun LevelPill(l: String?) = MPill(Levels.name(l), Levels.color(l), muted = l == null)

/** Выбор правила. [unset] — подпись варианта «без своего правила» (null — варианта нет). */
@Composable
fun LevelDialog(title: String, cur: String?, unset: String?, onPick: (String?) -> Unit, onDismiss: () -> Unit, levels: List<String> = Levels.all) {
    var sel by rememberSaveable { mutableStateOf(cur) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column(Modifier.heightIn(max = 520.dp)) {
                LazyColumn {
                    val opts: List<String?> = (if (unset != null) listOf<String?>(null) else emptyList()) + levels
                    items(opts) { l ->
                        Row(
                            Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
                                .selectable(selected = sel == l, role = Role.RadioButton, onClick = { sel = l }).padding(vertical = 6.dp),
                            verticalAlignment = Alignment.Top,
                        ) {
                            RadioButton(selected = sel == l, onClick = null)
                            Spacer(Modifier.width(8.dp))
                            Column {
                                Text(if (l == null) unset!! else Levels.name(l), style = MaterialTheme.typography.bodyLarge)
                                if (l != null) Text(Levels.explain(l), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = { onPick(sel) }, enabled = sel != cur) { Text("Сохранить") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

/** Название для ввода (новая группа, профиль, переименование). */
@Composable
fun NameDialog(title: String, init: String, onOk: (String) -> Unit, onDismiss: () -> Unit) {
    var t by rememberSaveable { mutableStateOf(init) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { OutlinedTextField(t, { t = it.take(60) }, singleLine = true, placeholder = { Text("например, «Банки»") }) },
        confirmButton = { TextButton(onClick = { onOk(t.trim()) }, enabled = t.isNotBlank()) { Text("Готово") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

private fun userSuffix(uid: Int) = (uid / 100_000).let { if (it > 0) " (польз. $it)" else "" }

/** Имя копии приложения: по отчёту, иначе по пакету. */
fun appName(vm: Vm, uid: Int, pkgs: List<String>): String {
    vm.report?.app(uid)?.let { return vm.labelOf(it) }
    val p = pkgs.firstOrNull() ?: return "uid $uid"
    return vm.labelOfPackage(p) + userSuffix(uid)
}

/** Почему профиль включается — коротко. */
private fun whenText(w: MWhen): String = buildList {
    if (w.manual) add("вручную")
    if (w.always) add("всегда")
    if (w.saver) add("с экономией энергии")
    if (w.belowPct > 0) add("заряд ≤ ${w.belowPct} %")
    if (w.abroad) add("за границей")
    if (w.night) add("ночью ${hhmm(w.nightFrom)}–${hhmm(w.nightTo)}")
    if (w.btAway.isNotEmpty()) add("когда рядом нет: " + w.btAway.joinToString(", "))
}.joinToString(", ").ifEmpty { "не включается сам — только вручную" }

private fun hhmm(m: Int) = "%02d:%02d".format(m / 60, m % 60)

// ---- карточка приложения ----

/** Группы приложения и что с ним в каждом профиле. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AppModesCard(vm: Vm, a: App) {
    LaunchedEffect(Unit) { if (vm.modes == null) vm.loadModes() }
    val info = vm.modes?.value() ?: return
    val cfg = info.cfg
    val label = vm.labelOf(a)
    val mine = cfg.groupsOf(a.uid, a.packages)
    val protected = info.isProtected(a.packages)
    var pickGroups by rememberSaveable { mutableStateOf(false) }
    var rule by rememberSaveable { mutableStateOf<String?>(null) }
    val editable = !vm.modesSaving && !vm.fileMode
    Section("Группы и профили", sub = if (mine.isEmpty()) "без группы" else mine.joinToString(", ") { it.name }, help = "modes", icon = Icons.Outlined.Folder) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            mine.forEach { MPill(it.name, if (it.id == Levels.KEEP) Pal.plug else MaterialTheme.colorScheme.primary) }
            OutlinedButton(onClick = { pickGroups = true }, enabled = editable) { Text(if (mine.isEmpty()) "Добавить в группу" else "Изменить") }
        }
        if (protected) Note(Lvl.INFO, "Приложение нужно для работы телефона — профили его не ограничивают, что бы ни было в правилах.")
        else if (mine.any { it.id == Levels.KEEP }) Note(Lvl.INFO, "В «Важном»: профили его не трогают.")
        Spacer(Modifier.height(6.dp))
        Text("Что с ним в профилях", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        cfg.profiles.forEach { p ->
            val own = p.apps[a.uid]
            val fromGroups = mine.mapNotNull { g -> p.rules[g.id]?.let { g to it } }
            val effLevel = own ?: fromGroups.minByOrNull { Levels.all.indexOf(it.second) }?.second ?: p.rules[Levels.REST]
            val src = when {
                own != null -> "своё правило"
                fromGroups.isNotEmpty() -> "по группе «${fromGroups.minBy { Levels.all.indexOf(it.second) }.first.name}»"
                p.rules[Levels.REST] != null -> "как все остальные"
                else -> "правила нет"
            }
            val active = info.state.active.any { it.id == p.id }
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable(enabled = editable && !protected) { rule = p.id }.padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(p.name + if (active) " · сейчас включён" else "", style = MaterialTheme.typography.bodyMedium)
                    Text(src, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                LevelPill(effLevel)
            }
        }
        info.eff(a.uid)?.let { e ->
            val pn = cfg.profile(e.profile)?.name ?: e.profile
            Note(Lvl.INFO, "Сейчас действует: ${Levels.name(e.level).lowercase()} (профиль «$pn»)" + if (e.tripped) " — уже сработало «много ест», фон запрещён до конца профиля." else ".")
        }
    }
    if (pickGroups) GroupsDialog(cfg, a.uid, label, onDone = { add, del ->
        pickGroups = false
        if (add.isNotEmpty() || del.isNotEmpty()) {
            var c = cfg
            add.forEach { c = c.withMember(a.uid, it, true) }
            del.forEach { c = c.withMember(a.uid, it, false) }
            vm.setModes(c, "Группы «$label» сохранены")
        }
    }, onDismiss = { pickGroups = false })
    rule?.let { pid ->
        val p = cfg.profile(pid)
        if (p == null) rule = null else LevelDialog(
            "«$label» в профиле «${p.name}»", p.apps[a.uid], "Как у его групп",
            onPick = { rule = null; vm.setAppRule(pid, a.uid, it) }, onDismiss = { rule = null },
        )
    }
}

/** Отметить группы приложения. */
@Composable
private fun GroupsDialog(cfg: ModesCfg, uid: Int, label: String, onDone: (List<String>, List<String>) -> Unit, onDismiss: () -> Unit) {
    val init = cfg.groups.filter { uid in it.members }.map { it.id }.toSet()
    val sel = remember { mutableStateListOf<String>().apply { addAll(init) } }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Группы «$label»") },
        text = {
            Column {
                cfg.groups.forEach { g ->
                    Row(
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable { if (g.id in sel) sel.remove(g.id) else sel.add(g.id) }.padding(vertical = 2.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = g.id in sel, onCheckedChange = null)
                        Spacer(Modifier.width(6.dp))
                        Column {
                            Text(g.name, style = MaterialTheme.typography.bodyLarge)
                            if (g.id == Levels.KEEP) Text("профили не трогают никогда", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
                Text("Групп можно отметить несколько. Внутри профиля из их правил действует самое мягкое.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        },
        confirmButton = { TextButton(onClick = { onDone(sel.filter { it !in init }, init.filter { it !in sel }) }) { Text("Сохранить") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

/** Сводка: какие профили сейчас включены. */
@Composable
fun ModesBanner(vm: Vm, navi: Navi) {
    val info = vm.modes?.value() ?: return
    if (info.state.active.isEmpty()) return
    Section(
        "Профили: " + info.state.active.joinToString(", ") { it.name },
        sub = info.state.active.joinToString(" · ") { it.why.joinToString(", ") },
        icon = Icons.Outlined.BatterySaver, iconTint = Pal.plug,
    ) {
        val n = info.claims.size
        val w = info.effective.count { it.level == "watch" }
        Text(
            buildList {
                if (n > 0) add("фон ограничен у $n ${Fmt.plural(n.toLong(), "приложения", "приложений", "приложений")}")
                if (w > 0) add("за $w ${Fmt.plural(w.toLong(), "приложением", "приложениями", "приложениями")} следит «много ест»")
            }.joinToString(", ").replaceFirstChar { it.uppercase() }.ifEmpty { "Под правила профилей сейчас никто не попал." },
            style = MaterialTheme.typography.bodyMedium,
        )
        TextButton(onClick = { navi.modes() }) { Text("Профили и группы") }
    }
}

// ---- главный экран ----

@Composable
fun ModesScreen(vm: Vm, navi: Navi) {
    LaunchedEffect(Unit) { vm.loadModes() }
    var newGroup by rememberSaveable { mutableStateOf(false) }
    var newProfile by rememberSaveable { mutableStateOf(false) }
    Scaffold(topBar = { DetailTopBar("Профили и группы", "экономия без «ультра»", onBack = { navi.back() }) }) { pad ->
        LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
            when (val m = vm.modes) {
                is Res.Ok -> {
                    val info = m.v
                    item(key = "now") { NowCard(info) }
                    item(key = "profiles") { ProfilesCard(vm, navi, info) { newProfile = true } }
                    item(key = "groups") { GroupsCard(vm, navi, info) { newGroup = true } }
                    if (info.log.isNotEmpty()) item(key = "log") { ModesLogCard(vm, navi, info) }
                    item(key = "safe") {
                        Section("Как это устроено", help = "modes") {
                            Explain(
                                "Группа — ваш список приложений, приложение может быть в нескольких. Профиль — когда включаться и что делать с группами. " +
                                    "Внутри профиля из правил групп приложения действует самое мягкое (белый список сильнее чёрного), между включёнными профилями — самое строгое. " +
                                    "«Важное» не трогает ничто. Всё, что ограничили профили, при их выключении возвращается как было; «Вернуть всё» в «Стороже» и удаление Жора снимают и это. " +
                                    "Рабочий стол, клавиатуру, звонки, SMS, сервисы Google и root Жор не трогает никогда, VPN-клиенты — только если вы сами положили их в группу.",
                            )
                        }
                    }
                }
                is Res.Err -> item { ErrorCard("Профили недоступны", m.msg, if (m.unsupported) null else ({ vm.loadModes() })) }
                else -> item { SkeletonScreen() }
            }
        }
    }
    val cfg = vm.modes?.value()?.cfg
    if (newGroup && cfg != null) NameDialog("Новая группа", "", onOk = { n ->
        newGroup = false
        val id = newId("g")
        vm.setModes(cfg.withGroup(MGroup(id, n, emptyList())), "Группа «$n» создана")
        navi.mgroup(id)
    }, onDismiss = { newGroup = false })
    if (newProfile && cfg != null) NameDialog("Новый профиль", "", onOk = { n ->
        newProfile = false
        val id = newId("p")
        vm.setModes(cfg.withProfile(MProfile(id, n, true, MWhen(manual = false), emptyMap(), emptyMap(), 5.0, 2000, 3)), "Профиль «$n» создан")
        navi.mprofile(id)
    }, onDismiss = { newProfile = false })
}

@Composable
private fun NowCard(info: ModesInfo) {
    val st = info.state
    Section(
        if (st.active.isEmpty()) "Сейчас профили выключены" else "Сейчас включены",
        sub = if (st.country.isNotEmpty()) "сеть ${st.country.uppercase()}" + (if (st.home.isNotEmpty()) " · дом ${st.home.uppercase()}" else "") + if (st.abroad) " · за границей" else "" else null,
        icon = Icons.Outlined.BatterySaver, iconTint = if (st.active.isEmpty()) null else Pal.plug,
    ) {
        st.active.forEach { a ->
            KV(a.name, Fmt.human(a.since), sub = a.why.joinToString(", "))
        }
        val bg = info.claims.size
        val watch = info.effective.count { it.level == "watch" && !it.tripped }
        val hard = info.effective.count { it.level == "hard" }
        if (st.active.isNotEmpty()) {
            KV("Фон ограничен", "$bg")
            if (info.disabled.isNotEmpty()) KV("Отключено пакетов", "${info.disabled.size}")
            if (info.nonet.isNotEmpty()) KV("Без сети", "${info.nonet.size}")
            if (info.components.isNotEmpty()) KV("Отключено компонентов", "${info.components.size}")
            if (hard > 0) KV("Жёстко (стоп при выключении экрана)", "$hard")
            if (watch > 0) KV("Следит «много ест»", "$watch")
        }
    }
}

@Composable
private fun ProfilesCard(vm: Vm, navi: Navi, info: ModesInfo, onNew: () -> Unit) {
    val cfg = info.cfg
    val editable = !vm.modesSaving && !vm.fileMode
    Section("Профили", sub = "переключатель — включить вручную", icon = Icons.Outlined.Rule) {
        cfg.profiles.forEach { p ->
            val active = info.state.active.any { it.id == p.id }
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable { navi.mprofile(p.id) }.padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(10.dp).clip(RoundedCornerShape(50)).background(if (active) Pal.plug else MaterialTheme.colorScheme.outlineVariant))
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    Text(p.name + if (!p.on) " · отключён" else "", style = MaterialTheme.typography.bodyLarge)
                    Text(whenText(p.`when`), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    val rules = cfg.groups.mapNotNull { g -> p.rules[g.id]?.let { "${g.name}: ${Levels.name(it).lowercase()}" } } +
                        listOfNotNull(p.rules[Levels.REST]?.let { "остальные: ${Levels.name(it).lowercase()}" })
                    if (rules.isNotEmpty()) Text(rules.joinToString(" · "), style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
                Switch(
                    checked = p.`when`.manual, enabled = editable && p.on,
                    onCheckedChange = { vm.setModes(cfg.withProfile(p.copy(`when` = p.`when`.copy(manual = it))), if (it) "«${p.name}» включён вручную" else "«${p.name}»: ручное включение снято") },
                )
            }
        }
        TextButton(onClick = onNew, enabled = editable) {
            Icon(Icons.Outlined.Add, null, Modifier.size(18.dp))
            Spacer(Modifier.width(4.dp))
            Text("Новый профиль")
        }
    }
}

@Composable
private fun GroupsCard(vm: Vm, navi: Navi, info: ModesInfo, onNew: () -> Unit) {
    val cfg = info.cfg
    Section("Группы", sub = "свои списки приложений", icon = Icons.Outlined.Folder) {
        cfg.groups.forEach { g ->
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).clickable { navi.mgroup(g.id) }.padding(vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(g.name, style = MaterialTheme.typography.bodyLarge)
                    val rules = cfg.profiles.mapNotNull { p -> p.rules[g.id]?.let { "${p.name}: ${Levels.name(it).lowercase()}" } }
                    Text(
                        if (g.id == Levels.KEEP) "не трогать никогда" else if (rules.isNotEmpty()) rules.joinToString(" · ") else "правил нет",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis,
                    )
                    // первые иконки — чтобы группу было видно с первого взгляда
                    val pkgs = (g.packages.filter { info.present[it].orEmpty().isNotEmpty() } + g.members.mapNotNull { info.pkgsOf(it).firstOrNull() } +
                        g.components.map { it.substringBefore('/') }).distinct()
                    if (pkgs.isNotEmpty()) Row(Modifier.padding(top = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                        pkgs.take(6).forEach { p ->
                            PkgIcon(vm, p, pkgName(vm, p), 24.dp)
                            Spacer(Modifier.width(4.dp))
                        }
                        if (pkgs.size > 6) Text("+${pkgs.size - 6}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                Text("${g.size}", style = MaterialTheme.typography.titleMedium.merge(NumStyle), color = MaterialTheme.colorScheme.onSurfaceVariant)
                Icon(Icons.Outlined.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        TextButton(onClick = onNew) {
            Icon(Icons.Outlined.Add, null, Modifier.size(18.dp))
            Spacer(Modifier.width(4.dp))
            Text("Новая группа")
        }
    }
}

private fun modeActionName(a: ActionRec) = when (a.action) {
    "restrict" -> "запрет фона"
    "unrestrict" -> "фон вернул"
    "disable" -> "отключил"
    "enable" -> "включил обратно"
    "cdisable" -> "отключил компонент"
    "cenable" -> "включил компонент обратно"
    "nonet" -> "закрыл сеть"
    "net" -> "открыл сеть"
    "stop" -> if (a.by == "watch" || a.by == "clean") "много ест: остановил" else "остановил"
    else -> a.action
}

@Composable
private fun ModesLogCard(vm: Vm, navi: Navi, info: ModesInfo) {
    Section("Что делали профили", sub = "новые сверху", icon = Icons.Outlined.History) {
        MoreList(info.log, initial = 6, key = "mlog") { a ->
            // компонент «пакет/класс» — иконка пакета-хозяина и понятное имя компонента
            val host = a.pkg.substringBefore('/')
            val cls = a.pkg.substringAfter('/', "")
            val name = if (cls.isNotEmpty()) KNOWN[cls] ?: cls.substringAfterLast('.') else pkgName(vm, a.pkg)
            val user = (a.uid ?: 0) / 100_000
            Row(Modifier.fillMaxWidth().padding(vertical = 5.dp), verticalAlignment = Alignment.CenterVertically) {
                PkgIcon(vm, host, name, 32.dp)
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(name, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(
                        "${modeActionName(a)} · ${Fmt.human(a.t)}" + (if (user > 0) " · ${spaceName(user)}" else "") + if (!a.ok) " · не вышло" else "",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (a.action == "stop" && a.out.isNotBlank()) Text(a.out, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
                if (a.ok && a.action == "restrict" && a.uid != null) TextButton(onClick = { navi.compare(CmpReq("action", a.t, a.uid)) }) { Text("До/после") }
            }
        }
    }
}

// ---- профиль ----

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ProfileScreen(vm: Vm, navi: Navi, id: String) {
    // со свежим списком Bluetooth-устройств — для условия «нет устройства рядом»
    LaunchedEffect(Unit) { vm.loadModes(withBt = true) }
    var pickBt by rememberSaveable { mutableStateOf(false) }
    val info = vm.modes?.value()
    val p = info?.cfg?.profile(id)
    var rename by rememberSaveable { mutableStateOf(false) }
    var rule by rememberSaveable { mutableStateOf<String?>(null) }
    var del by rememberSaveable { mutableStateOf(false) }
    Scaffold(topBar = { DetailTopBar(p?.name ?: "Профиль", "профиль", onBack = { navi.back() }) }) { pad ->
        if (info == null || p == null) {
            Box(Modifier.padding(pad)) { if (info == null) SkeletonScreen() else Note(Lvl.INFO, "Профиль удалён") }
            return@Scaffold
        }
        val cfg = info.cfg
        val editable = !vm.modesSaving && !vm.fileMode
        fun save(np: MProfile, ok: String? = null) = vm.setModes(cfg.withProfile(np), ok)
        val w = p.`when`
        LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
            item(key = "head") {
                val active = info.state.active.firstOrNull { it.id == p.id }
                Section(p.name, sub = active?.let { "включён: " + it.why.joinToString(", ") } ?: "сейчас выключен", icon = Icons.Outlined.Rule, iconTint = if (active != null) Pal.plug else null) {
                    SwitchRow("Профиль работает", "Выключите — не включится ни по одному условию, ограничения вернутся.", p.on, editable) { save(p.copy(on = it)) }
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(onClick = { rename = true }, enabled = editable) { Text("Переименовать") }
                        OutlinedButton(onClick = { del = true }, enabled = editable) { Text("Удалить") }
                    }
                }
            }
            item(key = "when") {
                Section("Когда включается", sub = "достаточно любого условия", icon = Icons.Outlined.Schedule) {
                    SwitchRow("Вручную", "Включить сейчас, пока не выключите.", w.manual, editable) { save(p.copy(`when` = w.copy(manual = it))) }
                    SwitchRow("Всегда", "Работает постоянно.", w.always, editable) { save(p.copy(`when` = w.copy(always = it))) }
                    SwitchRow("С экономией энергии Android", "Включили экономию в шторке или она включилась сама. Не на зарядке.", w.saver, editable) { save(p.copy(`when` = w.copy(saver = it))) }
                    SwitchRow(
                        "За границей",
                        "Сеть оператора другой страны, чем домашняя (${(cfg.homeCountry.ifEmpty { info.state.home }).uppercase().ifEmpty { "по SIM" }}). Сейчас сеть ${info.state.country.uppercase().ifEmpty { "неизвестна" }}.",
                        w.abroad, editable,
                    ) { save(p.copy(`when` = w.copy(abroad = it))) }
                    SwitchRow("Ночью", "С ${hhmm(w.nightFrom)} до ${hhmm(w.nightTo)} по местному времени.", w.night, editable) { save(p.copy(`when` = w.copy(night = it))) }
                    Row(
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable(enabled = editable) { pickBt = true }.padding(vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text("Когда рядом нет устройства", style = MaterialTheme.typography.bodyLarge)
                            Text(
                                if (w.btAway.isEmpty()) "Часы, наушники — по Bluetooth. Например, гасить приложение часов, пока часы не подключены."
                                else w.btAway.joinToString(", ") { n ->
                                    val d = info.state.bt.firstOrNull { it.name == n }
                                    n + when (d?.connected) { true -> " — подключено"; false -> " — не подключено"; null -> "" }
                                } + ". Включится, если устройства нет дольше 10 минут.",
                                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        Spacer(Modifier.width(8.dp))
                        Icon(Icons.Outlined.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    if (w.night) {
                        val presets = listOf(22 to 7, 23 to 7, 0 to 8, 1 to 9)
                        val cur = presets.indexOfFirst { it.first * 60 == w.nightFrom && it.second * 60 == w.nightTo }
                        Choice("Окно ночи", "", presets.mapIndexed { i, (a, b) -> Opt("${a}–$b", i.toLong()) }, cur.toLong(), editable) {
                            val (a, b) = presets[it.toInt()]
                            save(p.copy(`when` = w.copy(nightFrom = a * 60, nightTo = b * 60)))
                        }
                    }
                    Choice(
                        "При заряде не выше", "Не на зарядке.",
                        listOf(Opt("нет", 0), Opt("15 %", 15), Opt("20 %", 20), Opt("30 %", 30), Opt("50 %", 50)), w.belowPct.toLong(), editable,
                    ) { save(p.copy(`when` = w.copy(belowPct = it.toInt()))) }
                }
            }
            item(key = "rules") {
                Section("Что делать с группами", sub = "нажмите на группу, чтобы выбрать правило", icon = Icons.Outlined.Folder) {
                    cfg.groups.filter { it.id != Levels.KEEP }.forEach { g ->
                        RuleRow(g.name, "${g.size} ${Fmt.plural(g.size.toLong(), "приложение", "приложения", "приложений")}", p.rules[g.id], editable) { rule = g.id }
                    }
                    RuleRow("Все остальные", "приложения без группы и из групп без правила в этом профиле", p.rules[Levels.REST], editable) { rule = Levels.REST }
                    Explain("Если приложение в нескольких группах, из их правил действует самое мягкое. «Важное» профили не трогают.")
                }
            }
            if (p.apps.isNotEmpty()) item(key = "apps") {
                Section("Свои правила приложений", sub = "поверх групп") {
                    p.apps.forEach { (uid, l) ->
                        Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                            val name = appName(vm, uid, info.pkgsOf(uid))
                            UidIcon(vm, uid, name, 28.dp)
                            Spacer(Modifier.width(10.dp))
                            Text(name, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            LevelPill(l)
                            IconButton(onClick = { vm.setAppRule(p.id, uid, null) }, enabled = editable) { Icon(Icons.Outlined.Close, "убрать") }
                        }
                    }
                }
            }
            if (p.rules.values.any { it == "watch" } || p.apps.values.any { it == "watch" }) item(key = "heavy") {
                Section("«Много ест» — это", sub = "для правила «только если много ест»", icon = Icons.Outlined.Speed) {
                    Choice("Процессор в фоне больше", "в среднем за 30 минут при выключенном экране", listOf(Opt("3 мА", 3), Opt("5 мА", 5), Opt("10 мА", 10), Opt("20 мА", 20)), p.heavyMa.toLong(), editable) {
                        save(p.copy(heavyMa = it.toDouble()))
                    }
                    Choice("Или входящих пакетов в час больше", "каждый пакет во сне будит процессор", listOf(Opt("500", 500), Opt("1000", 1000), Opt("2000", 2000), Opt("5000", 5000)), p.heavyRxH.toLong(), editable) {
                        save(p.copy(heavyRxH = it.toInt()))
                    }
                    Choice("И не открывали дольше", "", listOf(Opt("1 ч", 1), Opt("3 ч", 3), Opt("6 ч", 6), Opt("12 ч", 12)), p.idleH.toLong(), editable) {
                        save(p.copy(idleH = it.toInt()))
                    }
                }
            }
        }
        if (rename) NameDialog("Название профиля", p.name, onOk = { rename = false; save(p.copy(name = it)) }, onDismiss = { rename = false })
        if (pickBt) BtDialog(info.state.bt, w.btAway, onDone = { sel -> pickBt = false; save(p.copy(`when` = w.copy(btAway = sel))) }, onDismiss = { pickBt = false })
        if (del) AlertDialog(
            onDismissRequest = { del = false },
            title = { Text("Удалить профиль «${p.name}»?") },
            text = { Text("Всё, что он ограничил, вернётся как было.") },
            confirmButton = { TextButton(onClick = { del = false; vm.setModes(cfg.withoutProfile(p.id), "Профиль удалён"); navi.back() }) { Text("Удалить") } },
            dismissButton = { TextButton(onClick = { del = false }) { Text("Отмена") } },
        )
        rule?.let { gid ->
            val title = if (gid == Levels.REST) "Все остальные" else cfg.group(gid)?.name ?: gid
            LevelDialog(
                "«$title» в профиле «${p.name}»", p.rules[gid], if (gid == Levels.REST) "Нет правила (не трогать)" else "Нет правила (как все остальные)",
                onPick = { l -> rule = null; save(p.copy(rules = if (l == null) p.rules - gid else p.rules + (gid to l))) },
                onDismiss = { rule = null },
                levels = if (gid == Levels.REST) Levels.forRest else Levels.all,
            )
        }
    }
}

/** Выбор Bluetooth-устройств для условия «нет рядом». */
@Composable
private fun BtDialog(devs: List<BtDev>, cur: List<String>, onDone: (List<String>) -> Unit, onDismiss: () -> Unit) {
    val sel = remember { mutableStateListOf<String>().apply { addAll(cur) } }
    // выбранное раньше, но уже не сопряжённое, тоже показываем — чтобы можно было снять
    val all = (devs.map { it.name } + cur).distinct()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Когда рядом нет устройства") },
        text = {
            Column {
                if (all.isEmpty()) Text("Сопряжённых устройств не видно. Включите Bluetooth и откройте экран ещё раз.", style = MaterialTheme.typography.bodyMedium)
                all.forEach { n ->
                    val d = devs.firstOrNull { it.name == n }
                    Row(
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable { if (n in sel) sel.remove(n) else sel.add(n) }.padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = n in sel, onCheckedChange = null)
                        Spacer(Modifier.width(6.dp))
                        Column {
                            Text(n, style = MaterialTheme.typography.bodyLarge)
                            Text(
                                when (d?.connected) { true -> "подключено сейчас"; false -> "не подключено"; null -> "больше не сопряжено" },
                                style = MaterialTheme.typography.bodySmall, color = if (d?.connected == true) Pal.plug else MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
                Text("Профиль включится, когда ни одного из отмеченных нет дольше 10 минут, и выключится, как только любое подключится.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        },
        confirmButton = { TextButton(onClick = { onDone(sel.toList()) }) { Text("Готово") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

@Composable
private fun SwitchRow(title: String, sub: String, checked: Boolean, enabled: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.width(8.dp))
        Switch(checked = checked, onCheckedChange = onChange, enabled = enabled)
    }
}

@Composable
private fun RuleRow(title: String, sub: String, level: String?, enabled: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable(enabled = enabled, onClick = onClick).padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        LevelPill(level)
    }
}

// ---- группа ----

/** Понятные имена служебных пакетов и компонентов, у которых в системе нет внятной подписи. */
private val KNOWN = mapOf(
    "com.miui.analytics" to "Аналитика Xiaomi",
    "com.miui.msa.global" to "Системная реклама Xiaomi (MSA)",
    "com.miui.misightservice" to "MiSight — сбор событий системы",
    "com.miui.bugreport" to "Отчёты об ошибках Xiaomi",
    "com.bsp.catchlog" to "Сбор системных логов",
    "com.xiaomi.mipicks" to "GetApps — магазин Xiaomi",
    "com.xiaomi.discover" to "Обновления системных приложений Xiaomi",
    "com.xiaomi.glgm" to "Игровые сервисы Xiaomi",
    "com.xiaomi.joyose" to "Joyose — облачные профили производительности",
    "com.xiaomi.xmsf" to "Сервисы Xiaomi: пуши и аккаунт",
    "com.xiaomi.xmsfkeeper" to "Страж сервисов Xiaomi",
    "com.xiaomi.aicr" to "HyperAI — движок",
    "com.xiaomi.aiservice" to "HyperAI — службы",
    "io.appmetrica.analytics.internal.AppMetricaService" to "AppMetrica: аналитика",
    "io.appmetrica.analytics.internal.PreloadInfoContentProvider" to "AppMetrica: данные предустановки",
)

/** Имя пакета для людей: известное служебное → подпись из системы → хвост имени пакета. */
fun pkgName(vm: Vm, pkg: String): String {
    KNOWN[pkg]?.let { return it }
    val l = vm.labelOfPackage(pkg)
    return if (l.isBlank() || l == pkg) pkg.substringAfterLast('.').replaceFirstChar { it.uppercase() } else l
}

/** Иконка пакета (видна и для отключённых и стоящих только во втором пространстве), иначе — буква. */
@Composable
fun PkgIcon(vm: Vm, pkg: String, name: String, size: Dp = 40.dp) {
    val ic = remember(pkg) { vm.iconOfPackage(pkg) }
    if (ic != null) {
        Image(ic, null, Modifier.size(size).clip(CircleShape))
    } else {
        Box(Modifier.size(size).clip(CircleShape).background(Pal.radio.copy(alpha = 0.22f)), contentAlignment = Alignment.Center) {
            Text(name.take(1).uppercase(), fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.onSurface)
        }
    }
}

private fun spaceName(user: Int) = when (user) {
    0 -> "основное"
    10 -> "второе пространство"
    999 -> "двойные приложения"
    else -> "пользователь $user"
}

private fun spaceShort(user: Int) = when (user) {
    0 -> "основное"
    10 -> "второе"
    999 -> "двойные"
    else -> "польз. $user"
}

/** «основное и второе пространство» — где стоит пакет. */
private fun spacesText(uids: List<Int>): String {
    val users = uids.map { it / 100_000 }.distinct().sorted()
    return when {
        users.isEmpty() -> "не установлено"
        users == listOf(0) -> "основное пространство"
        users.size == 1 -> spaceName(users[0])
        else -> "пространства: " + users.joinToString(" · ") { spaceShort(it) }
    }
}

/** Строка группы: пакет (во всех пространствах), отдельная копия или компонент. */
private data class GEntry(
    val key: String, val pkg: String, val name: String, val sub: String, val installed: Boolean,
    val status: Pair<String, Color>?, val target: Int?,
)

private fun groupEntries(vm: Vm, info: ModesInfo, g: MGroup): List<GEntry> {
    fun status(pkg: String, uids: List<Int>, comp: String? = null): Pair<String, Color>? = when {
        comp != null && info.components.any { it.pkg == comp } -> "отключён" to Levels.color("disable")
        comp == null && info.disabled.any { it.pkg == pkg } -> "отключено" to Levels.color("disable")
        comp == null && info.nonet.any { it.uid in uids } -> "без сети" to Levels.color("nonet")
        comp == null && info.claims.any { it.uid in uids } -> "фон запрещён" to Levels.color("bg")
        else -> null
    }
    fun target(uids: List<Int>) = uids.firstOrNull { vm.report?.app(it) != null }
    val out = mutableListOf<GEntry>()
    g.packages.forEach { pkg ->
        val uids = info.present[pkg].orEmpty()
        out += GEntry("p:$pkg", pkg, pkgName(vm, pkg), spacesText(uids), uids.isNotEmpty(), status(pkg, uids), target(uids))
    }
    g.members.forEach { uid ->
        val pkg = info.pkgsOf(uid).firstOrNull() ?: return@forEach
        out += GEntry("u:$uid", pkg, pkgName(vm, pkg), "только " + spaceName(uid / 100_000), true, status(pkg, listOf(uid)), target(listOf(uid)))
    }
    g.components.forEach { c ->
        val pkg = c.substringBefore('/')
        val cls = c.substringAfter('/')
        val uids = info.present[pkg].orEmpty()
        out += GEntry(
            "c:$c", pkg, KNOWN[cls] ?: cls.substringAfterLast('.'), "внутри «${pkgName(vm, pkg)}» · " + spacesText(uids),
            uids.isNotEmpty(), status(pkg, uids, c), null,
        )
    }
    return out.sortedWith(compareByDescending<GEntry> { it.installed }.thenBy { it.name.lowercase() })
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun GroupScreen(vm: Vm, navi: Navi, id: String) {
    LaunchedEffect(Unit) { if (vm.modes == null) vm.loadModes() }
    val info = vm.modes?.value()
    val g = info?.cfg?.group(id)
    var rename by rememberSaveable { mutableStateOf(false) }
    var pick by rememberSaveable { mutableStateOf(false) }
    var del by rememberSaveable { mutableStateOf(false) }
    Scaffold(topBar = { DetailTopBar(g?.name ?: "Группа", g?.let { "${it.size} ${Fmt.plural(it.size.toLong(), "приложение", "приложения", "приложений")}" }, onBack = { navi.back() }) }) { pad ->
        if (info == null || g == null) {
            Box(Modifier.padding(pad)) { if (info == null) SkeletonScreen() else Note(Lvl.INFO, "Группа удалена") }
            return@Scaffold
        }
        val cfg = info.cfg
        val editable = !vm.modesSaving && !vm.fileMode
        val entries = groupEntries(vm, info, g)
        LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
            item(key = "rules") {
                Section(if (g.id == Levels.KEEP) "Жор их не трогает" else "Что с группой в профилях", icon = Icons.Outlined.Rule) {
                    if (g.id == Levels.KEEP) {
                        Text("Ни один профиль не ограничивает эти приложения — даже если они ещё и в группе с правилом построже.", style = MaterialTheme.typography.bodyMedium)
                    } else {
                        cfg.profiles.forEach { p ->
                            val active = info.state.active.any { it.id == p.id }
                            Row(
                                Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable { navi.mprofile(p.id) }.padding(vertical = 8.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Box(Modifier.size(8.dp).clip(CircleShape).background(if (active) Pal.plug else MaterialTheme.colorScheme.outlineVariant))
                                Spacer(Modifier.width(10.dp))
                                Text(p.name, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
                                LevelPill(p.rules[g.id])
                            }
                        }
                        Text("Нажмите на профиль, чтобы поменять правило. «Нет правила» — как все остальные в этом профиле.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 10.dp)) {
                        FilledTonalButton(onClick = { pick = true; if (vm.installed == null) vm.loadModes(withInstalled = true) }, enabled = !vm.modesSaving) {
                            Icon(Icons.Outlined.Edit, null, Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Изменить состав")
                        }
                        TextButton(onClick = { rename = true }, enabled = editable) { Text("Переименовать") }
                        if (g.id != Levels.KEEP) TextButton(onClick = { del = true }, enabled = editable) { Text("Удалить", color = MaterialTheme.colorScheme.error) }
                    }
                }
            }
            item(key = "list") {
                Section("Приложения", sub = if (entries.isEmpty()) null else "нажмите — откроется карточка") {
                    if (entries.isEmpty()) {
                        Text("Пока пусто — нажмите «Изменить состав».", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    entries.forEach { e ->
                        val alpha = if (e.installed) 1f else 0.5f
                        Row(
                            Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp))
                                .clickable(enabled = e.target != null) { e.target?.let { navi.app(it) } }
                                .padding(vertical = 8.dp).alpha(alpha),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            PkgIcon(vm, e.pkg, e.name, 40.dp)
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(e.name, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                Text(e.sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                e.status?.let { (t, c) ->
                                    Spacer(Modifier.height(4.dp))
                                    MPill(t, c)
                                }
                            }
                            if (e.target != null) Icon(Icons.Outlined.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
        }
        if (rename) NameDialog("Название группы", g.name, onOk = { rename = false; vm.setModes(cfg.withGroup(g.copy(name = it))) }, onDismiss = { rename = false })
        if (del) AlertDialog(
            onDismissRequest = { del = false },
            title = { Text("Удалить группу «${g.name}»?") },
            text = { Text("Приложения останутся, но правила профилей для этой группы пропадут. Что они ограничили — вернётся как было.") },
            confirmButton = { TextButton(onClick = { del = false; vm.setModes(cfg.withoutGroup(g.id), "Группа удалена"); navi.back() }) { Text("Удалить") } },
            dismissButton = { TextButton(onClick = { del = false }) { Text("Отмена") } },
        )
        if (pick) AppPicker(vm, info, g, onDone = { add, remove ->
            pick = false
            if (add.isNotEmpty() || remove.isNotEmpty()) {
                val removeUids = g.members.filter { u -> info.pkgsOf(u).any { it in remove } }
                val ng = g.copy(packages = (g.packages - remove.toSet()) + add.filter { it !in g.packages }, members = g.members - removeUids.toSet())
                vm.setModes(cfg.withGroup(ng), "«${g.name}»: ${ng.size} ${Fmt.plural(ng.size.toLong(), "приложение", "приложения", "приложений")}")
            }
        }, onDismiss = { pick = false })
    }
}

/**
 * Состав группы: все приложения телефона по пакетам (сразу во всех пространствах), с иконками, поиском и галочками.
 * Сверху — уже выбранные.
 */
@Composable
private fun AppPicker(vm: Vm, info: ModesInfo, g: MGroup, onDone: (add: List<String>, remove: List<String>) -> Unit, onDismiss: () -> Unit) {
    data class PRow(val pkg: String, val name: String, val uids: List<Int>, val mah: Double?)
    val inGroup = remember(g) { (g.packages + g.members.flatMap { info.pkgsOf(it) }).toSet() }
    val sel = remember { mutableStateListOf<String>().apply { addAll(inGroup) } }
    var q by rememberSaveable { mutableStateOf("") }
    val src = vm.installed
    val rows = remember(src, vm.report) {
        val byPkg = mutableMapOf<String, MutableList<Int>>()
        val m = src ?: vm.report?.apps.orEmpty().filter { it.kind == "app" }.associate { it.uid to it.packages }
        m.forEach { (uid, ps) -> ps.forEach { byPkg.getOrPut(it) { mutableListOf() } += uid } }
        byPkg.map { (pkg, uids) -> PRow(pkg, pkgName(vm, pkg), uids.sorted(), uids.mapNotNull { vm.report?.app(it)?.mah }.takeIf { it.isNotEmpty() }?.sum()) }
            .sortedWith(compareByDescending<PRow> { it.pkg in inGroup }.thenByDescending { it.mah ?: -1.0 }.thenBy { it.name.lowercase() })
    }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Scaffold(
            topBar = {
                Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        IconButton(onClick = onDismiss) { Icon(Icons.Outlined.Close, "закрыть") }
                        Column(Modifier.weight(1f)) {
                            Text("Состав «${g.name}»", style = MaterialTheme.typography.titleLarge)
                            Text("выбрано ${sel.size} · во всех пространствах сразу", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        FilledTonalButton(onClick = { onDone(sel.filter { it !in inGroup }, inGroup.filter { it !in sel }) }) { Text("Готово") }
                    }
                    OutlinedTextField(
                        q, { q = it }, Modifier.fillMaxWidth().padding(top = 8.dp), singleLine = true,
                        leadingIcon = { Icon(Icons.Outlined.Search, null) }, placeholder = { Text("Найти приложение") },
                    )
                }
            },
        ) { pad ->
            val shown = rows.filter { q.isBlank() || it.name.contains(q, true) || it.pkg.contains(q, true) }
            LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = PaddingValues(start = 8.dp, end = 16.dp, bottom = 24.dp)) {
                if (src == null) item { Text("Загружаю все приложения телефона…", Modifier.padding(16.dp), style = MaterialTheme.typography.bodySmall) }
                items(shown, key = { it.pkg }) { r ->
                    val on = r.pkg in sel
                    Row(
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).clickable { if (on) sel.remove(r.pkg) else sel.add(r.pkg) }.padding(vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = on, onCheckedChange = null)
                        Spacer(Modifier.width(4.dp))
                        PkgIcon(vm, r.pkg, r.name, 36.dp)
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(r.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            val other = info.cfg.groups.filter { it.id != g.id && (r.pkg in it.packages || it.members.any { u -> u in r.uids }) }
                            val sub = buildList {
                                add(spacesText(r.uids))
                                r.mah?.let { add("${Fmt.mah(it)} мА·ч за отчёт") }
                                if (other.isNotEmpty()) add("ещё в: " + other.joinToString(", ") { it.name })
                                if (r.pkg in info.protected) add("защищено")
                            }.joinToString(" · ")
                            Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
        }
    }
}
