@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)

package dev.niazlv.zhor

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material.icons.outlined.DataObject
import androidx.compose.material.icons.outlined.DeleteForever
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.FileDownload
import androidx.compose.material.icons.automirrored.outlined.InsertDriveFile
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.SaveAlt
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material.icons.outlined.Storage
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import java.util.Calendar

private data class Kind(val key: String, val title: String, val sub: String, val mime: String, val icon: ImageVector)

private val KINDS = listOf(
    Kind("report_json", "Отчёт JSON", "Всё, что показывает приложение: приложения, процессы, потоки, сон, лента. Для анализа или разработчику.", "application/json", Icons.Outlined.DataObject),
    Kind("report_text", "Отчёт текстом", "Читаемая сводка, как у zhord report. Удобно переслать в чат.", "text/plain", Icons.Outlined.Description),
    Kind("raw", "Сырые записи", "Все поминутные отрезки, снимки и события за период (.jsonl.gz). Для своей обработки.", "application/gzip", Icons.Outlined.Archive),
)

private fun groupName(k: String) = when (k) {
    "buckets" -> "Поминутные записи"
    "snaps" -> "Снимки статистики Android"
    "events" -> "События"
    "log" -> "Журнал ядра"
    else -> k
}

private fun groupExplain(k: String) = when (k) {
    "buckets" -> "ток, процессы и потоки по отрезкам ~1 мин, по файлу на день"
    "snaps" -> "срезы batterystats: раз в час, при смене экрана и зарядки"
    "events" -> "экран, зарядка, запуски ядра"
    "log" -> "сообщения ядра для отладки"
    else -> ""
}

@Composable
fun DataScreen(vm: Vm, navi: Navi) {
    LaunchedEffect(Unit) { vm.loadStorage() }
    Scaffold(
        topBar = {
            DetailTopBar("Данные и выгрузка", "что хранится и как забрать", onBack = { navi.back() }) {
                IconButton(onClick = { vm.loadStorage() }, enabled = vm.storage !is Res.Loading) { Icon(Icons.Outlined.Refresh, "пересчитать") }
            }
        },
    ) { pad ->
        LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
            item(key = "st") { StorageCard(vm) }
            item(key = "ex") { ExportCard(vm) }
            item(key = "pr") { PruneCard(vm) }
        }
    }
}

@Composable
private fun StorageCard(vm: Vm) {
    Section("Место на диске", sub = "считается один раз при открытии экрана", icon = Icons.Outlined.Storage) {
        AnimatedContent(vm.storage, contentKey = { it::class }, transitionSpec = { fadeIn(tween(220)) togetherWith fadeOut(tween(120)) }, label = "st") { st ->
            Column {
                when (st) {
                    is Res.Ok -> StorageBody(st.v)
                    is Res.Err -> Text(st.msg, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                    else -> {
                        val b = shimmerBrush()
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            SkeletonLine(0.4f, 28.dp, b)
                            repeat(3) { SkeletonLine(0.85f, 12.dp, b) }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun StorageBody(s: Storage) {
    Row(verticalAlignment = Alignment.Bottom) {
        AnimatedNumber(s.totalBytes / 1048576.0, { Fmt.d1(it) }, MaterialTheme.typography.displaySmall)
        Text(" МБ", Modifier.padding(bottom = 6.dp))
        Spacer(Modifier.weight(1f))
        Text(s.dir, style = MaterialTheme.typography.labelSmall, fontFamily = FontFamily.Monospace, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
    if (s.approx) Text("Оценка по размерам файлов: ядро старше 0.3 не считает записи само.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    Spacer(Modifier.height(10.dp))
    val colors = listOf(Pal.wifi, Pal.radio, Pal.screen, Pal.other)
    StackBar(s.groups.mapIndexed { i, g -> colors[i % colors.size] to g.bytes.toDouble() }, height = 10.dp)
    Spacer(Modifier.height(6.dp))
    s.groups.forEachIndexed { i, g ->
        LegendRow(
            colors[i % colors.size], groupName(g.kind), Fmt.bytes(g.bytes),
            sub = buildString {
                append(groupExplain(g.kind))
                if (g.records > 0) append(" · ${Fmt.count(g.records)} записей")
                if (g.files > 1) append(" · ${g.files} файлов")
                if (g.from > 0 && g.to > 0) append(" · ${Fmt.span(g.from, g.to)}")
            },
        )
    }
    if (s.days.isNotEmpty()) {
        Spacer(Modifier.height(12.dp))
        Text("Поминутные записи по дням", style = MaterialTheme.typography.titleSmall)
        Spacer(Modifier.height(6.dp))
        DayBars(s.days)
        val avg = s.days.dropLast(1).map { it.bytes }.average().takeIf { !it.isNaN() } ?: s.days.last().bytes.toDouble()
        Explain("Полный день записей — около ${Fmt.bytes(avg.toLong())}. Старые дни ядро удаляет само по сроку хранения из настроек.")
    }
}

@Composable
private fun ExportCard(vm: Vm) {
    val ctx = LocalContext.current
    var kind by rememberSaveable { mutableIntStateOf(0) }
    var range by rememberSaveable { mutableStateOf(vm.range) }
    val k = KINDS[kind]
    val saveJson = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { it?.let(vm::saveExportTo) }
    val saveTxt = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/plain")) { it?.let(vm::saveExportTo) }
    val saveGz = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/gzip")) { it?.let(vm::saveExportTo) }
    Section("Выгрузка", sub = "файл на телефон или в другое приложение", icon = Icons.Outlined.FileDownload) {
        Text("Что выгрузить", style = MaterialTheme.typography.labelLarge)
        Spacer(Modifier.height(4.dp))
        KINDS.forEachIndexed { i, kk ->
            val sel = i == kind
            Row(
                Modifier.fillMaxWidth().padding(vertical = 3.dp).clip(RoundedCornerShape(16.dp))
                    .border(BorderStroke(if (sel) 2.dp else 1.dp, if (sel) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant), RoundedCornerShape(16.dp))
                    .clickable { kind = i; vm.clearExport() }.padding(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(kk.icon, null, tint = if (sel) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    Text(kk.title, style = MaterialTheme.typography.bodyLarge, fontWeight = if (sel) FontWeight.SemiBold else FontWeight.Normal)
                    Text(kk.sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                RadioButton(selected = sel, onClick = { kind = i; vm.clearExport() })
            }
        }
        Spacer(Modifier.height(10.dp))
        Text("За какой период", style = MaterialTheme.typography.labelLarge)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            RANGES.filter { it.first != "charge" }.forEach { (rk, l) ->
                FilterChip(selected = range == rk, onClick = { range = rk; vm.clearExport() }, label = { Text(l) })
            }
        }
        Spacer(Modifier.height(10.dp))
        AnimatedContent(vm.export, contentKey = { it::class }, transitionSpec = { (fadeIn(tween(220)) + slideInVertically { it / 4 }) togetherWith fadeOut(tween(120)) }, label = "ex") { st ->
            Column {
                when (st) {
                    is Res.Loading -> Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(vertical = 8.dp)) {
                        CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(10.dp))
                        Text("Готовлю файл…", style = MaterialTheme.typography.bodyMedium)
                    }
                    is Res.Ok -> {
                        val r = st.v
                        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Lvl.GOOD.bg()).padding(12.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Icon(Icons.AutoMirrored.Outlined.InsertDriveFile, null, tint = Lvl.GOOD.color())
                                Spacer(Modifier.width(8.dp))
                                Text(r.file.name, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, fontFamily = FontFamily.Monospace, fontSize = 12.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            }
                            Spacer(Modifier.height(6.dp))
                            KV("Размер", Fmt.bytes(r.bytes), strong = true)
                            r.records.filter { it.value > 0 }.forEach { (rk, n) ->
                                KV(
                                    when (rk) {
                                        "buckets" -> "Поминутных записей"
                                        "snaps" -> "Снимков"
                                        "events" -> "Событий"
                                        "apps" -> "Приложений в отчёте"
                                        "timeline" -> "Точек ленты"
                                        "lines" -> "Строк"
                                        else -> rk
                                    },
                                    Fmt.count(n),
                                )
                            }
                            if (r.from > 0 && r.to > r.from) KV("Период", Fmt.span(r.from, r.to))
                        }
                        Spacer(Modifier.height(10.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button(onClick = {
                                val name = r.file.name
                                when (r.kind) {
                                    "report_text" -> saveTxt.launch(name)
                                    "raw" -> saveGz.launch(name)
                                    else -> saveJson.launch(name)
                                }
                            }, Modifier.weight(1f)) {
                                Icon(Icons.Outlined.SaveAlt, null, Modifier.size(18.dp))
                                Spacer(Modifier.width(6.dp))
                                Text("Сохранить…")
                            }
                            OutlinedButton(onClick = {
                                runCatching {
                                    val uri = FileProvider.getUriForFile(ctx, "dev.niazlv.zhor.files", r.file)
                                    val send = Intent(Intent.ACTION_SEND).setType(KINDS.first { it.key == r.kind }.mime)
                                        .putExtra(Intent.EXTRA_STREAM, uri).putExtra(Intent.EXTRA_SUBJECT, r.file.name)
                                        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                                    ctx.startActivity(Intent.createChooser(send, "Отправить отчёт Жора"))
                                }.onFailure { vm.toast = "Не удалось поделиться: ${it.message}" }
                            }, Modifier.weight(1f)) {
                                Icon(Icons.Outlined.Share, null, Modifier.size(18.dp))
                                Spacer(Modifier.width(6.dp))
                                Text("Поделиться")
                            }
                        }
                        TextButton(onClick = { vm.clearExport() }) { Text("Выбрать другое") }
                    }
                    is Res.Err -> {
                        ErrorBox(st.msg, st.unsupported)
                        Spacer(Modifier.height(8.dp))
                        PrepareButton { vm.runExport(k.key, Span(range)) }
                    }
                    else -> PrepareButton { vm.runExport(k.key, Span(range)) }
                }
            }
        }
        Explain("Файл сначала собирается в папке приложения, затем вы выбираете, куда его сохранить. Жор ничего не отправляет сам.")
    }
}

@Composable
private fun PrepareButton(onClick: () -> Unit) {
    FilledTonalButton(onClick = onClick, Modifier.fillMaxWidth()) {
        Icon(Icons.Outlined.Download, null, Modifier.size(18.dp))
        Spacer(Modifier.width(6.dp))
        Text("Подготовить файл")
    }
}

@Composable
private fun ErrorBox(msg: String, info: Boolean) {
    val lvl = if (info) Lvl.INFO else Lvl.BAD
    Text(msg, Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(lvl.bg()).padding(12.dp), style = MaterialTheme.typography.bodySmall)
}

private fun dayStart(daysAgo: Int): Long = Calendar.getInstance().apply {
    add(Calendar.DAY_OF_YEAR, -daysAgo)
    set(Calendar.HOUR_OF_DAY, 0); set(Calendar.MINUTE, 0); set(Calendar.SECOND, 0); set(Calendar.MILLISECOND, 0)
}.timeInMillis

private fun isoDay(t: Long) = java.text.SimpleDateFormat("yyyy-MM-dd", java.util.Locale.US).format(java.util.Date(t))

@Composable
private fun PruneCard(vm: Vm) {
    var days by rememberSaveable { mutableIntStateOf(3) }
    var ask by rememberSaveable { mutableStateOf(false) }
    var askAll by rememberSaveable { mutableStateOf(false) }
    val st = vm.storage.value()
    val before = dayStart(days)
    val hit = st?.days?.filter { it.date < isoDay(before) } ?: emptyList()
    Section("Очистка", sub = "удалить старые записи вручную", icon = Icons.Outlined.DeleteSweep) {
        Text("Удалить записи старше", style = MaterialTheme.typography.labelLarge)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            listOf(1 to "1 дня", 3 to "3 дней", 7 to "недели").forEach { (d, l) ->
                FilterChip(selected = days == d, onClick = { days = d }, label = { Text(l) })
            }
        }
        Text(
            if (st == null) "Сначала посчитаю, что хранится…"
            else if (hit.isEmpty()) "Записей раньше ${Fmt.time(before)} нет — удалять нечего."
            else "Под удаление: ${hit.size} дн. поминутных записей (${Fmt.bytes(hit.sumOf { it.bytes })}) и снимки раньше ${Fmt.time(before)}.",
            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        OutlinedButton(
            onClick = { ask = true }, enabled = vm.prune !is Res.Loading && !vm.fileMode && hit.isNotEmpty(),
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Удалить записи до ${Fmt.time(before)}…") }
        TextButton(
            onClick = { askAll = true }, enabled = vm.prune !is Res.Loading && !vm.fileMode,
            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
        ) {
            Icon(Icons.Outlined.DeleteForever, null, Modifier.size(18.dp))
            Spacer(Modifier.width(6.dp))
            Text("Удалить все записи…")
        }
        when (val p = vm.prune) {
            is Res.Loading -> Text("Удаляю…", Modifier.padding(top = 6.dp), style = MaterialTheme.typography.bodySmall)
            is Res.Ok -> Text(p.v, Modifier.padding(top = 6.dp), style = MaterialTheme.typography.bodySmall, color = Lvl.GOOD.color())
            is Res.Err -> ErrorBox(p.msg, p.unsupported)
            else -> {}
        }
        Explain("Без очистки ядро и так хранит записи ограниченный срок (см. «Сбор данных» в настройках). Удалённое не вернуть: отчёты за это время станут пустыми.")
    }
    if (ask) {
        AlertDialog(
            onDismissRequest = { ask = false },
            icon = { Icon(Icons.Outlined.DeleteSweep, null) },
            title = { Text("Удалить записи до ${Fmt.time(before)}?") },
            text = {
                Text(
                    "Будут удалены поминутные записи, снимки и события раньше ${Fmt.time(before)}" +
                        (if (hit.isNotEmpty()) " — ${hit.size} дн., около ${Fmt.bytes(hit.sumOf { it.bytes })}" else "") +
                        ". Отчёты за это время станут пустыми. Отменить нельзя.",
                )
            },
            confirmButton = {
                TextButton(onClick = { ask = false; vm.runPrune(before, false) }, colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("Удалить") }
            },
            dismissButton = { TextButton(onClick = { ask = false }) { Text("Отмена") } },
        )
    }
    if (askAll) {
        var sure by rememberSaveable { mutableStateOf(false) }
        AlertDialog(
            onDismissRequest = { askAll = false },
            icon = { Icon(Icons.Outlined.DeleteForever, null, tint = MaterialTheme.colorScheme.error) },
            title = { Text("Удалить все записи?") },
            text = {
                Column {
                    Text(
                        "Сотрутся все записи Жора" + (st?.let { " (${Fmt.bytes(it.totalBytes)})" } ?: "") +
                            ": история тока, процессов, сна и снимки. Ядро продолжит запись с нуля. Отменить нельзя.",
                    )
                    Row(Modifier.padding(top = 8.dp).clickable { sure = !sure }, verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(sure, { sure = it })
                        Text("Понимаю, что история пропадёт")
                    }
                }
            },
            confirmButton = {
                TextButton(
                    onClick = { askAll = false; vm.runPrune(null, true) }, enabled = sure,
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) { Text("Удалить всё") }
            },
            dismissButton = { TextButton(onClick = { askAll = false }) { Text("Отмена") } },
        )
    }
}
