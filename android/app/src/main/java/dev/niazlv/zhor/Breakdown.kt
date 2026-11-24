package dev.niazlv.zhor

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.DonutLarge
import androidx.compose.material.icons.outlined.Wifi
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

// ---- (0.8) из чего состоит «Android OS» ----

private fun partColor(key: String): Color = when (key) {
    "screen" -> Pal.screen
    "suspend" -> Pal.off
    "server_binder" -> Pal.cpu
    "server_own" -> Pal.cpuIdle
    "systemui" -> Pal.radio
    "sensors" -> Pal.sensors
    "security" -> Pal.bt
    "vendor" -> Pal.video
    "noncpu" -> Pal.wifi
    else -> Pal.other
}

/**
 * «Android OS» — не одна программа, а десятки процессов под системным uid. Карточка раскладывает её
 * расход по смыслу, чтобы было видно, откуда берутся мА·ч: «За кого работает system» ниже — только
 * часть system_server, которую Android сам раскладывает по заказчикам.
 */
@Composable
fun OsPartsCard(a: App, sw: SystemWork?) {
    Section(
        "Из чего состоит Android OS",
        sub = "${a.processes.size} системных процессов, а не одна программа",
        help = "os_parts", icon = Icons.Outlined.DonutLarge,
    ) {
        StackBar(a.parts.map { partColor(it.key) to it.mah }, height = 12.dp)
        Spacer(Modifier.height(8.dp))
        a.parts.forEach { p ->
            Row(Modifier.fillMaxWidth().padding(vertical = 5.dp)) {
                Box(Modifier.padding(top = 5.dp).size(10.dp).clip(RoundedCornerShape(3.dp)).background(partColor(p.key)))
                Spacer(Modifier.width(8.dp))
                Column(Modifier.weight(1f)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(p.title, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        Text(Fmt.pct(100 * p.share), Modifier.padding(horizontal = 6.dp), style = MaterialTheme.typography.labelMedium.merge(NumStyle), color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text("${Fmt.mah(p.mah)} мА·ч", style = MaterialTheme.typography.bodyMedium.merge(NumStyle), fontWeight = FontWeight.SemiBold)
                    }
                    val sub = buildList {
                        if (p.key != "noncpu") add("при выкл. экране ${Fmt.mah(p.mahScreenOff)} мА·ч")
                        if (p.top.isNotEmpty() && p.key != "noncpu") add("в основном: " + p.top.joinToString(", ") { "${short(it.first)} ${Fmt.mah(it.second)}" })
                    }.joinToString(" · ")
                    if (sub.isNotEmpty()) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 3, overflow = TextOverflow.Ellipsis)
                    Text(p.explain, style = MaterialTheme.typography.bodySmall)
                }
            }
        }
        val binder = a.parts.firstOrNull { it.key == "server_binder" }
        if (sw != null && sw.mahTotal > 0) {
            Explain(
                "Ниже, в «За кого работает system», — только запросы к system_server, которые Android раскладывает по заказчикам: " +
                    "${Fmt.mah(sw.mahTotal)} мА·ч" + (binder?.let { " (здесь это «${it.title}» — ${Fmt.mah(it.mah)})" } ?: "") +
                    ". Поэтому крупнейший заказчик там тратит единицы мА·ч, а «Android OS» — сотни: основная часть — экран, засыпания и своя работа системы.",
            )
        }
    }
}

/** Длинные имена процессов HAL — до сути: «vendor.qti.hardware.display.composer-service» → «display.composer-service». */
private fun short(name: String): String {
    val n = name.substringBefore(" [uid")
    if (n.length <= 28) return n
    val parts = n.split('.')
    return if (parts.size > 2) parts.takeLast(2).joinToString(".") else n.take(28) + "…"
}

// ---- (0.8) кто будит по сети ----

private fun netName(vm: Vm, w: NetWaker): String {
    val base = when (w.uid % 100_000) {
        0 -> "root: ядро и программы от root"
        1000 -> "Android OS"
        2000 -> "adb (отладка по USB и Wi-Fi)"
        1051 -> "DNS"
        1073 -> "сетевой стек Android"
        else -> vm.labelOfUid(w.uid, w.label)
    }
    val user = w.uid / 100_000
    return if (user > 0 && !base.contains("польз.")) "$base (польз. $user)" else base
}

/** Экран «Сон»: какие приложения получали пакеты, пока экран был выключен. */
@Composable
fun NetWakersCard(vm: Vm, navi: Navi, r: Report, nw: NetWakers) {
    Section(
        "Кто будит по сети",
        sub = "в покое ${Fmt.dur(nw.idleMs)}" + if (nw.since > r.from + 10 * 60_000) " · замеры сети с ${Fmt.time(nw.since)}" else "",
        help = "net_wakers", icon = Icons.Outlined.Wifi,
    ) {
        val wph = nw.wakeupsPerHour
        if (nw.wifiWakeups + nw.mobileWakeups > 0) {
            KV(
                "Пакеты будили процессор", "${Fmt.count(wph.toLong())} / ч",
                if (wph > 1000) Lvl.BAD else if (wph > 200) Lvl.WARN else Lvl.NONE,
                sub = "Wi-Fi ${Fmt.count(nw.wifiWakeups)} · мобильная сеть ${Fmt.count(nw.mobileWakeups)} за покой",
            )
        }
        val hours = nw.idleMs / 3_600_000.0
        KV("Принято пакетов", Fmt.count(nw.rxPackets), sub = if (hours > 0) "${Fmt.count((nw.rxPackets / hours).toLong())} в час покоя, без VPN-клиентов" else null)
        Spacer(Modifier.height(8.dp))
        val max = nw.apps.filter { !it.n.vpn }.maxOfOrNull { it.n.share }?.takeIf { it > 0 } ?: 1.0
        MoreList(nw.apps, initial = 6, key = "netw") { w ->
            val target = r.app(w.uid) != null
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
                    .then(if (target) Modifier.clickable { navi.app(w.uid) } else Modifier).padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val name = netName(vm, w)
                UidIcon(vm, w.uid, name, 32.dp)
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(name, Modifier.weight(1f), maxLines = 2, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium)
                        if (!w.n.vpn) Text(Fmt.pct(100 * w.n.share), Modifier.padding(start = 6.dp), style = MaterialTheme.typography.bodyMedium.merge(NumStyle), fontWeight = FontWeight.SemiBold)
                    }
                    Spacer(Modifier.height(3.dp))
                    if (!w.n.vpn) ShareBar((w.n.share / max).toFloat(), Pal.wifi, height = 5.dp)
                    Text(
                        if (w.n.vpn) {
                            "VPN-клиент: ${Fmt.count(w.n.rxPerHour.toLong())} пакетов/ч — это зашифрованный трафик приложений, в долях не считается"
                        } else {
                            "${Fmt.count(w.n.rxPerHour.toLong())} пакетов/ч · принято ${Fmt.bytes(w.n.rxBytes)}" +
                                if (w.n.txBytes > 0) " · отправлено ${Fmt.bytes(w.n.txBytes)}" else ""
                        },
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 3, overflow = TextOverflow.Ellipsis,
                    )
                }
                if (target) Icon(Icons.Outlined.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Explain(
            "Во сне каждый входящий пакет будит процессор. Чей пакет разбудил, Android не говорит: драйвер Wi-Fi этого телефона такие пакеты не помечает. " +
                "Поэтому здесь — сколько пакетов каждое приложение получило, пока экран был выключен: кто получает больше, тот чаще и будит. " +
                "Трафик через VPN засчитан самим приложениям.",
        )
    }
}

/** Строки для карточки «Сеть» приложения: сколько оно получало в покое. */
@Composable
fun NetIdleRows(n: NetIdle) {
    if (n.vpn) {
        KV("В покое через него прошло", "${Fmt.count(n.rxPerHour.toLong())} пакетов/ч", sub = "VPN-клиент: это трафик других приложений, они и будят")
        return
    }
    KV(
        "В покое принято", "${Fmt.count(n.rxPerHour.toLong())} пакетов/ч",
        if (n.rxPerHour > 2000) Lvl.BAD else if (n.rxPerHour > 300) Lvl.WARN else Lvl.NONE,
        help = "net_wakers",
        sub = "${Fmt.pct(100 * n.share)} всех входящих пакетов в покое · ${Fmt.bytes(n.rxBytes)}",
    )
}
