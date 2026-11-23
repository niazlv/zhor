package dev.niazlv.zhor

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material3.Icon
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Speed
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/** Энергия процесса zhord за период отчёта — честная цена самого профилировщика. */
data class SelfCost(val mah: Double, val mahOff: Double, val cpuMs: Long, val avgMa: Double, val sharePct: Double, val base: Double)

fun selfCost(r: Report): SelfCost? {
    val p = r.apps.firstOrNull { it.uid == 0 }?.processes?.firstOrNull { it.name == "zhord" } ?: return null
    val hours = r.totals.spanMs / 3_600_000.0
    val base = shareBase(r)
    val share = if (base > 0) 100 * p.mah / base else 0.0
    return SelfCost(p.mah, p.mahScreenOff, p.cpuMs, if (hours > 0) p.mah / hours else 0.0, share, base)
}

fun selfCostLvl(sharePct: Double): Lvl = when {
    sharePct <= 0 -> Lvl.NONE
    sharePct < 1 -> Lvl.GOOD
    sharePct < 3 -> Lvl.WARN
    else -> Lvl.BAD
}

/**
 * Карточка «Цена самого Жора»: энергия zhord из отчёта (главное), ток zhord в живой точке,
 * затем техника шага из `status.cost`.
 */
@Composable
fun SelfCostCard(vm: Vm, s: CoreStatus?, liveMa: Double?, liveMode: Boolean = false) {
    val r = vm.report
    val c = r?.let { selfCost(it) }
    Section("Цена самого Жора", sub = "сколько тратит сам профилировщик", help = "scan_cost", icon = Icons.Outlined.Speed) {
        if (c != null && r != null) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                MetricTile("Ток zhord", Fmt.d1(c.avgMa), Modifier.weight(1f), unit = "мА", sub = "в среднем за период")
                MetricTile(
                    "Доля расхода", if (c.sharePct in 0.0..0.05) "<0,1" else Fmt.d1(c.sharePct), Modifier.weight(1f), unit = "%",
                    lvl = selfCostLvl(c.sharePct), sub = "${Fmt.mah(c.mah)} из ${Fmt.mah(c.base)} мА·ч",
                )
            }
            Spacer(Modifier.height(6.dp))
            Text(
                "Период «${r.label}»: процессор ${Fmt.dur(c.cpuMs)}" + if (c.mahOff > 0.005) ", при выключенном экране ${Fmt.mah(c.mahOff)} мА·ч" else "",
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else if (r != null) {
            Text("Процесса zhord в отчёте за этот период нет — ядро почти ничего не потратило.", style = MaterialTheme.typography.bodySmall)
        }
        if (liveMode) {
            Spacer(Modifier.height(8.dp))
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(Lvl.INFO.bg()).padding(12.dp),
            ) {
                Icon(Icons.Outlined.Info, null, Modifier.size(18.dp), tint = Lvl.INFO.color())
                Spacer(Modifier.width(8.dp))
                Text(
                    "Пока открыт «Сейчас», ядро в живом режиме: шаг раз в ${s?.intervalMs?.takeIf { it > 0 }?.let { Fmt.d0(it / 1000.0) } ?: "2"} с, " +
                        "все процессы читаются каждый шаг. Поэтому ниже цена выше обычной. Обычная цена — средний ток за период" +
                        (c?.let { " (${Fmt.d1(it.avgMa)} мА)" } ?: "") + "; закроете экран — ядро вернётся к обычному шагу.",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        if (liveMa != null && liveMa > 0) {
            KV(
                if (liveMode) "Сейчас, в живом режиме" else "Прямо сейчас", "${Fmt.d1(liveMa)} мА",
                sub = "модель zhord в последнем шаге",
            )
        }
        if (s != null) {
            Spacer(Modifier.height(4.dp))
            if (s.costCpuUs > 0) {
                KV(
                    "Процессор на шаг", "${Fmt.d0(s.costCpuUs / 1000.0)} мс", Norms.scanCost(s.costCpuUs),
                    sub = "по часам ${Fmt.d0(s.scanCostUs / 1000.0)} мс: демон с низким приоритетом уступает другим" +
                        if (s.intervalMs > 0) " · шаг раз в ${Fmt.d0(s.intervalMs / 1000.0)} с" else "",
                )
            } else {
                KV("Проход сбора", "${s.scanCostUs / 1000} мс", Norms.scanCost(s.scanCostUs))
            }
            if (s.cpuTotalUs > 0 && s.started > 0) {
                val up = (System.currentTimeMillis() - s.started).coerceAtLeast(1)
                KV(
                    "Процессор с запуска", Fmt.dur(s.cpuTotalUs / 1000),
                    sub = "за ${Fmt.dur(up)} работы — ${Fmt.d2(100.0 * s.cpuTotalUs / 1000 / up)} % одного ядра",
                )
            }
            if (s.costProcs > 0) {
                KV(
                    "Прочитано за шаг", "${s.costProcs} проц.",
                    sub = (
                        if (s.skippedIdle > 0) "ещё ${s.skippedIdle} простаивающих пропущено (их читают раз в 6 шагов) · "
                        else "все процессы, без пропусков · "
                        ) +
                        "потоков ${s.costThreads}, time_in_state ${s.costTis}",
                )
            }
        }
        Explain("Честная цена — энергия процесса zhord: его потоки считаются так же, как у всех приложений, и он виден в списке как часть «root». Телефон он не будит.")
    }
}
