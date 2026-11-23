package dev.niazlv.zhor

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.SystemUpdate
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

private fun published(iso: String): String =
    runCatching { Fmt.human(java.time.Instant.parse(iso).toEpochMilli()) }.getOrDefault(iso.take(10))

/**
 * Заметки к выпуску для экрана: без служебного хвоста (установка и проверка — для страницы на GitHub), без
 * обратных кавычек и **, строки CHANGELOG, перенесённые посреди фразы, склеены, пункты — «•».
 */
private fun notesShort(s: String): String {
    val out = mutableListOf<String>()
    var cur: StringBuilder? = null
    fun flush() {
        cur?.let { out += it.toString() }
        cur = null
    }
    for (raw in s.substringBefore("### Установка").replace("`", "").replace("**", "").lines()) {
        val l = raw.trim()
        val c = cur
        when {
            l.isEmpty() -> {
                flush()
                if (out.isNotEmpty() && out.last().isNotEmpty()) out += ""
            }
            l.startsWith("#") -> {
                flush()
                out += l.trimStart('#').trim()
            }
            l.startsWith("- ") -> {
                flush()
                cur = StringBuilder("• " + l.removePrefix("- "))
            }
            c != null -> c.append(' ').append(l)
            else -> cur = StringBuilder(l)
        }
    }
    flush()
    return out.joinToString("\n").trim()
}

/** Настройки: версия, проверка раз в сутки, «Обновить» — только по кнопке. */
@Composable
fun UpdateCard(vm: Vm) {
    val ctx = LocalContext.current
    val installed = remember { Updater.installedVersion(ctx) }
    val cfg = vm.config.value()
    var confirm by rememberSaveable { mutableStateOf(false) }
    val st = vm.upd
    Section("Обновления", sub = "установлена $installed", icon = Icons.Outlined.SystemUpdate) {
        when (st) {
            is Upd.Available, is Upd.Failed -> {
                val r = (st as? Upd.Available)?.r ?: (st as Upd.Failed).r
                if (st is Upd.Failed) Note(Lvl.BAD, st.msg)
                if (r != null) {
                    if (st is Upd.Failed) Spacer(Modifier.height(8.dp))
                    Text("Доступна ${r.version}", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                    Text("вышла ${published(r.published)} · ${Fmt.bytes(r.apkSize)}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    val notes = notesShort(r.notes)
                    if (notes.isNotEmpty()) {
                        var all by rememberSaveable { mutableStateOf(false) }
                        val lines = notes.lines()
                        Text(
                            if (all || lines.size <= 8) notes else lines.take(8).joinToString("\n") + "\n…",
                            Modifier.padding(top = 8.dp), style = MaterialTheme.typography.bodySmall,
                        )
                        Row {
                            if (lines.size > 8) TextButton(onClick = { all = !all }) { Text(if (all) "Свернуть" else "Что нового — полностью") }
                            TextButton(onClick = { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(r.url))) }) { Text("На GitHub") }
                        }
                    }
                    if (vm.fileMode) {
                        // без root ставить некому — только скачать вручную
                        FilledTonalButton(onClick = { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(r.url))) }, Modifier.padding(top = 4.dp)) {
                            Text("Скачать APK на GitHub")
                        }
                    } else FilledTonalButton(onClick = { confirm = true }, Modifier.padding(top = 4.dp)) {
                        Icon(Icons.Outlined.SystemUpdate, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(6.dp))
                        Text("Обновить до ${r.version}")
                    }
                    if (confirm) AlertDialog(
                        onDismissRequest = { confirm = false },
                        title = { Text("Обновить Жор до ${r.version}?") },
                        text = {
                            Text(
                                "Жор скачает ${Fmt.bytes(r.apkSize)} с GitHub, сверит контрольную сумму и подпись — она должна " +
                                    "совпасть с установленной — и поставит обновление через root. Приложение закроется и откроется " +
                                    "снова; записи и настройки сохранятся.",
                            )
                        },
                        confirmButton = { TextButton(onClick = { confirm = false; vm.startUpdate(r) }) { Text("Обновить") } },
                        dismissButton = { TextButton(onClick = { confirm = false }) { Text("Отмена") } },
                    )
                }
            }
            is Upd.Downloading -> {
                Text("Скачиваю ${st.r.version}: ${Fmt.bytes(st.done)} из ${Fmt.bytes(st.total)}", style = MaterialTheme.typography.bodyMedium)
                LinearProgressIndicator(
                    progress = { if (st.total > 0) (st.done.toFloat() / st.total).coerceIn(0f, 1f) else 0f },
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
            }
            is Upd.Verifying -> {
                Text("Сверяю контрольную сумму и подпись…", style = MaterialTheme.typography.bodyMedium)
                LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 8.dp))
            }
            is Upd.Installing -> {
                Text("Ставлю ${st.r.version}… Жор сейчас закроется и откроется снова.", style = MaterialTheme.typography.bodyMedium)
                LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 8.dp))
            }
            Upd.Checking -> {
                Text("Спрашиваю GitHub…", style = MaterialTheme.typography.bodyMedium)
                LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 8.dp))
            }
            Upd.UpToDate, Upd.Idle -> KV(
                "Последняя проверка",
                if (vm.updChecked > 0) Fmt.human(vm.updChecked) else "ещё не было",
                sub = if (st == Upd.UpToDate) "установлена последняя версия" else null,
            )
        }
        val busy = st is Upd.Checking || st is Upd.Downloading || st is Upd.Verifying || st is Upd.Installing
        if (st !is Upd.Available && !busy) {
            OutlinedButton(onClick = { vm.checkUpdate() }, Modifier.padding(top = 8.dp)) { Text("Проверить сейчас") }
        }
        if (cfg?.updateCheck != null) {
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Проверять раз в сутки", style = MaterialTheme.typography.bodyLarge)
                    Text(
                        "Найдётся новая версия — придёт уведомление. Ставится только по кнопке «Обновить».",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.width(8.dp))
                Switch(checked = cfg.updateCheck, onCheckedChange = { vm.setUpdateCheck(it) }, enabled = !vm.configSaving)
            }
        }
        Explain("Интернет нужен Жору только для этого: спросить у GitHub номер последней версии и, по вашей кнопке, скачать её. О телефоне и ваших записях ничего не отправляется.")
    }
}

/** На сводке: вышла новая версия. */
@Composable
fun UpdateBanner(vm: Vm, onOpen: () -> Unit) {
    val r = (vm.upd as? Upd.Available)?.r ?: return
    Section("Доступен Жор ${r.version}", sub = "установлена ${Updater.installedVersion(LocalContext.current)} · вышла ${published(r.published)}", icon = Icons.Outlined.SystemUpdate) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilledTonalButton(onClick = onOpen) { Text("Что нового и обновить") }
        }
    }
}
