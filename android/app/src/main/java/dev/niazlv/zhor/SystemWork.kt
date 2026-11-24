package dev.niazlv.zhor

import android.content.Intent
import android.provider.Settings
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AccessibilityNew
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Цвета заказчиков на полосе «за кого работает system». */
private val CALLER_COLORS = listOf(Pal.wifi, Pal.radio, Pal.gnss, Pal.audio, Pal.video, Pal.sensors, Pal.bt, Pal.screen)
private const val SHOWN_CALLERS = 8

/** Имя заказчика: uid 1000 — сама система, uid 0 — нативные демоны, остальные — по PackageManager. */
private fun callerName(vm: Vm, c: SysCaller, seg: String?): String = when (c.uid % 100_000) {
    1000 -> "Сама система: SystemUI и службы"
    0 -> "Нативные демоны (netd и др.)"
    else -> vm.labelOfUid(c.uid, c.label, seg)
}

/** Карточка «Android OS»: кто заставил system_server работать, очереди, «сама система». */
@Composable
fun SystemWorkCard(r: Report, sw: SystemWork, vm: Vm, navi: Navi, seg: String?) {
    val sysApp = r.app(1000)
    Section(
        "За кого работает system", sub = "запросы к system_server — их Android раскладывает по заказчикам",
        help = "system_work", icon = Icons.Outlined.Hub,
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile("Работа system_server", Fmt.mah(sw.mahTotal), Modifier.weight(1f), unit = "мА·ч", sub = "CPU ${Fmt.dur(sw.totalMs.toLong())}")
            val own = if (sw.totalMs > 0) 100 * sw.ownMs / sw.totalMs else 0.0
            MetricTile("Из них по заказу", Fmt.pctRaw(100 - own), Modifier.weight(1f), unit = "%", sub = "сама система — ${Fmt.pctRaw(own)} %")
        }
        Explain(
            "Эти мА·ч — часть расхода «Android OS»" + (sysApp?.let { " (${Fmt.mah(it.mah)} мА·ч)" } ?: "") +
                (if (sw.serverMah > 0) ", и даже не весь system_server (${Fmt.mah(sw.serverMah)} мА·ч)" else "") +
                ", а не прибавка: ниже видно, кто заставил систему их потратить. Окно — ${Fmt.span(sw.from, sw.to)}.",
        )
        Spacer(Modifier.height(10.dp))
        val shown = sw.callers.take(SHOWN_CALLERS)
        val restMs = (sw.callers.drop(SHOWN_CALLERS).sumOf { it.cpuMs }).coerceAtLeast(0.0)
        StackBar(
            shown.mapIndexed { i, c -> CALLER_COLORS[i % CALLER_COLORS.size] to c.cpuMs } +
                listOf(Pal.other to restMs, Pal.cpuIdle to sw.ownMs),
            height = 12.dp,
        )
        Spacer(Modifier.height(6.dp))
        val maxShare = shown.maxOfOrNull { it.share }?.takeIf { it > 0 } ?: 1.0
        shown.forEachIndexed { i, c ->
            val target = c.uid % 100_000 != 1000 && r.app(c.uid) != null
            CallerRow(vm, c, callerName(vm, c, seg), CALLER_COLORS[i % CALLER_COLORS.size], maxShare, if (target) ({ navi.app(c.uid, seg) }) else null)
        }
        if (sw.callers.size > SHOWN_CALLERS) {
            Text(
                (sw.callers.size - SHOWN_CALLERS).toLong().let { n -> "ещё $n ${Fmt.plural(n, "заказчик", "заказчика", "заказчиков")} — ${Fmt.dur(restMs.toLong())} CPU" },
                Modifier.padding(start = 40.dp, top = 2.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (sw.ownMs > 0) {
            LegendRow(
                Pal.cpuIdle, "Сама система, без заказчика", Fmt.dur(sw.ownMs.toLong()),
                share = if (sw.totalMs > 0) Fmt.pct(100 * sw.ownMs / sw.totalMs) else null,
                sub = "≈ ${Fmt.mah(sw.ownMah)} мА·ч · свои очереди: память, питание, Wi-Fi — ниже",
            )
        }
        if (sw.queues.isNotEmpty()) {
            Spacer(Modifier.height(14.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Внутренние очереди", style = MaterialTheme.typography.titleSmall)
                    Text("что system_server делал в своих потоках", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                HelpButton("system_queue")
            }
            val maxQ = sw.queues.maxOf { it.cpuMs }.coerceAtLeast(0.001)
            MoreList(sw.queues, initial = 4, key = "q$seg") { q -> QueueRow(q, maxQ) }
        }
        sw.note?.let { Explain(it) }
        Explain(
            "Почему system то почти ничего не тратит, то много: он работает, когда его дёргают — приложения опрашивают аккаунты и пакеты, " +
                "обновляют уведомления, шлют события спецвозможностей; плюс установки и обновления приложений и снимки статистики самого Жора.",
        )
    }
}

@Composable
private fun CallerRow(vm: Vm, c: SysCaller, name: String, color: Color, maxShare: Double, onClick: (() -> Unit)?) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier).padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        UidIcon(vm, c.uid, name, 32.dp)
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(name, Modifier.weight(1f), maxLines = 2, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium)
                Text(Fmt.pct(100 * c.share), Modifier.padding(horizontal = 6.dp), style = MaterialTheme.typography.labelMedium.merge(NumStyle), color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("${Fmt.mah(c.mah)} мА·ч", style = MaterialTheme.typography.bodyMedium.merge(NumStyle), fontWeight = FontWeight.SemiBold)
            }
            Spacer(Modifier.height(3.dp))
            ShareBar((c.share / maxShare).toFloat(), color, height = 5.dp)
            val top = c.top.firstOrNull()
            Text(
                buildString {
                    append("CPU ${Fmt.dur(c.cpuMs.toLong())}")
                    if (c.calls > 0) append(" · вызовов ${Fmt.count(c.calls)}")
                    if (c.binderMs <= 0 && c.queueMs > 0) append(" · задачи в очередях")
                    if (top != null) append(" · чаще всего ${top.call}")
                },
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis,
            )
            top?.explain?.let { Text(it, style = MaterialTheme.typography.bodySmall, maxLines = 3, overflow = TextOverflow.Ellipsis) }
        }
        if (onClick != null) Icon(Icons.Outlined.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun QueueRow(q: SysQueue, max: Double) {
    Column(Modifier.padding(vertical = 5.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(q.thread, Modifier.weight(1f), fontFamily = FontFamily.Monospace, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(Fmt.pct(100 * q.share), Modifier.padding(horizontal = 6.dp), style = MaterialTheme.typography.labelMedium.merge(NumStyle), color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(Fmt.dur(q.cpuMs.toLong()), style = MaterialTheme.typography.bodySmall.merge(NumStyle), fontWeight = FontWeight.SemiBold)
        }
        ShareBar((q.cpuMs / max).toFloat(), Pal.cpuIdle, height = 4.dp)
        q.explain?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
        q.top.firstOrNull()?.let { m ->
            Text(
                "${m.handler.substringAfterLast('.')} · ${m.message} ×${Fmt.count(m.count)}",
                style = MaterialTheme.typography.labelSmall, fontFamily = FontFamily.Monospace, color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** Включённые спецвозможности: почему из-за них все приложения шлют события, и кнопка в настройки. */
@Composable
fun A11yCard(sw: SystemWork, vm: Vm) {
    if (sw.a11y.isEmpty()) return
    val ctx = LocalContext.current
    val ev = sw.callers.flatMap { c -> c.top.filter { it.call.endsWith("#sendAccessibilityEvent") } }
    val evMs = ev.sumOf { it.cpuMs }
    val evCalls = ev.sumOf { it.calls }
    val lvl = if (evMs >= 20_000) Lvl.WARN else Lvl.INFO
    Section(
        "Включены спецвозможности", sub = "из-за них приложения шлют системе события",
        help = "a11y", icon = Icons.Outlined.AccessibilityNew, iconTint = lvl.color(),
    ) {
        sw.a11y.forEach { svc ->
            val pkg = svc.substringBefore('/')
            val cls = svc.substringAfterLast('.').substringAfterLast('/')
            val ic = remember(pkg) { vm.iconOfPackage(pkg) }
            Row(Modifier.padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                if (ic != null) Image(ic, null, Modifier.size(28.dp).clip(CircleShape))
                else Icon(Icons.Outlined.AccessibilityNew, null, Modifier.size(28.dp), tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(10.dp))
                Column {
                    Text(vm.labelOfPackage(pkg), style = MaterialTheme.typography.bodyMedium)
                    Text(cls, style = MaterialTheme.typography.labelSmall, fontFamily = FontFamily.Monospace, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        Spacer(Modifier.height(4.dp))
        Text(
            "Пока включена хоть одна служба спецвозможностей, каждое приложение сообщает системе о каждом изменении экрана — " +
                "прокрутке, анимации, новом тексте. system_server принимает эти события и пересылает службе. Поэтому систему " +
                "нагружают даже приложения, которые сами ничего особенного не делают.",
            style = MaterialTheme.typography.bodySmall,
        )
        if (evMs > 0) {
            Spacer(Modifier.height(6.dp))
            KV("События спецвозможностей", "${Fmt.dur(evMs.toLong())} CPU", lvl, sub = "${Fmt.count(evCalls)} ${Fmt.plural(evCalls, "вызов", "вызова", "вызовов")} от ${ev.size} ${Fmt.plural(ev.size.toLong(), "заказчика", "заказчиков", "заказчиков")} за окно")
        }
        Explain("Если эта функция не нужна (например, жест скриншота тремя пальцами), выключите службу — нагрузка на систему уйдёт.")
        Spacer(Modifier.height(8.dp))
        FilledTonalButton(onClick = {
            runCatching { ctx.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
        }, Modifier.fillMaxWidth()) {
            Icon(Icons.AutoMirrored.Outlined.OpenInNew, null, Modifier.size(18.dp))
            Spacer(Modifier.width(6.dp))
            Text("Настройки спецвозможностей")
        }
    }
}

/** Карточка приложения: сколько system_server потратил по его запросам. */
@Composable
fun SystemLoadCard(a: App, r: Report, vm: Vm, navi: Navi, seg: String?) {
    val sw = r.systemWork
    val caller = sw?.callers?.firstOrNull { it.uid == a.uid }
    Section("Нагрузка на систему", sub = "сколько system_server работал по его запросам", help = "system_work", icon = Icons.Outlined.Hub) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile("Система потратила", Fmt.mah(a.systemMah), Modifier.weight(1f), unit = "мА·ч", sub = "≈ по его заказу")
            MetricTile(
                "CPU system_server", Fmt.dur(a.systemMs.toLong()), Modifier.weight(1f),
                sub = caller?.let { "${Fmt.pct(100 * it.share)} всей работы system" },
                lvl = if ((caller?.share ?: 0.0) >= 0.1) Lvl.WARN else Lvl.NONE,
            )
        }
        Explain("Эти мА·ч видны в «Android OS», а не у приложения: это не прибавка к итогу, а подсказка, сколько системной работы оно вызвало.")
        KV(
            "Итого вместе с нагрузкой на систему", "${Fmt.mah(a.mah + a.systemMah)} мА·ч",
            sub = "для сравнения; в общем расходе эти мА·ч уже учтены",
        )
        if (a.systemCalls.isNotEmpty()) {
            Spacer(Modifier.height(10.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Что оно просило у системы", Modifier.weight(1f), style = MaterialTheme.typography.titleSmall)
                HelpButton("binder")
            }
            val max = a.systemCalls.maxOf { it.cpuMs }.coerceAtLeast(0.001)
            MoreList(a.systemCalls, initial = 4, key = "sc${a.uid}$seg") { c -> CallRow(c, max) }
        }
        sw?.let { s -> s.note?.let { Explain(it) } ?: Explain("Окно разбора — ${Fmt.span(s.from, s.to)}.") }
        TextButton(onClick = { navi.app(1000, seg) }) {
            Text("За кого ещё работает system")
            Icon(Icons.Outlined.ChevronRight, null)
        }
    }
}

@Composable
private fun CallRow(c: SysCall, max: Double) {
    Column(Modifier.padding(vertical = 5.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(c.method.ifEmpty { c.call }, Modifier.weight(1f), fontFamily = FontFamily.Monospace, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                (if (c.calls > 0) "${Fmt.count(c.calls)}× · " else "") + Fmt.dur(c.cpuMs.toLong()),
                style = MaterialTheme.typography.bodySmall.merge(NumStyle), fontWeight = FontWeight.SemiBold,
            )
        }
        if (c.method.isNotEmpty()) Text(c.service, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
        ShareBar((c.cpuMs / max).toFloat(), Pal.wifi, height = 4.dp)
        c.explain?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
    }
}
