@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)

package dev.niazlv.zhor

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.HelpOutline
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.ErrorOutline
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Lightbulb
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Открыть подсказку по термину (нижний лист). */
val LocalHelp = staticCompositionLocalOf<(String) -> Unit> { {} }

// ---------------- карточки и строки ----------------

@Composable
fun Section(
    title: String,
    modifier: Modifier = Modifier,
    sub: String? = null,
    help: String? = null,
    icon: ImageVector? = null,
    iconTint: Color? = null,
    trailing: (@Composable RowScope.() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp),
        shape = RoundedCornerShape(22.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 16.dp).animateContentSize()) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (icon != null) {
                    val tint = iconTint ?: MaterialTheme.colorScheme.primary
                    Box(Modifier.size(32.dp).clip(CircleShape).background(tint.copy(alpha = 0.14f)), contentAlignment = Alignment.Center) {
                        Icon(icon, null, Modifier.size(18.dp), tint = tint)
                    }
                    Spacer(Modifier.width(10.dp))
                }
                Column(Modifier.weight(1f)) {
                    Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                    if (sub != null) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                trailing?.invoke(this)
                if (help != null) HelpButton(help)
            }
            Spacer(Modifier.height(10.dp))
            content()
        }
    }
}

@Composable
fun HelpButton(key: String, modifier: Modifier = Modifier) {
    val help = LocalHelp.current
    IconButton(onClick = { help(key) }, modifier.size(36.dp)) {
        Icon(Icons.AutoMirrored.Outlined.HelpOutline, "что это", Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** Пояснение мелким шрифтом с лампочкой. */
@Composable
fun Explain(text: String, modifier: Modifier = Modifier) {
    Row(modifier.padding(top = 6.dp)) {
        Icon(Icons.Outlined.Lightbulb, null, Modifier.size(15.dp).padding(top = 2.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f))
        Spacer(Modifier.width(6.dp))
        Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
fun LevelPill(lvl: Lvl, text: String, modifier: Modifier = Modifier) {
    Row(
        modifier.clip(RoundedCornerShape(50)).background(lvl.bg()).padding(horizontal = 10.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(7.dp).clip(CircleShape).background(lvl.color()))
        Spacer(Modifier.width(6.dp))
        Text(text, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurface)
    }
}

@Composable
fun Pill(text: String, color: Color, modifier: Modifier = Modifier) {
    Text(
        text,
        modifier.clip(RoundedCornerShape(50)).background(color.copy(alpha = 0.16f)).padding(horizontal = 8.dp, vertical = 2.dp),
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurface,
    )
}

@Composable
fun KV(k: String, v: String, lvl: Lvl = Lvl.NONE, help: String? = null, sub: String? = null, strong: Boolean = false) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(k, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (sub != null) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.8f))
        }
        Spacer(Modifier.width(8.dp))
        if (lvl != Lvl.NONE) {
            Box(Modifier.size(8.dp).clip(CircleShape).background(lvl.color()))
            Spacer(Modifier.width(6.dp))
        }
        Text(
            v, style = MaterialTheme.typography.bodyMedium.merge(NumStyle),
            fontWeight = if (strong || lvl != Lvl.NONE) FontWeight.SemiBold else FontWeight.Normal,
            textAlign = TextAlign.End, modifier = Modifier.widthIn(max = 220.dp),
        )
        if (help != null) HelpButton(help, Modifier.padding(start = 2.dp))
    }
}

/** Плитка с числом: подпись, значение, единица, пояснение. */
@Composable
fun MetricTile(
    label: String,
    value: String,
    modifier: Modifier = Modifier,
    unit: String? = null,
    sub: String? = null,
    lvl: Lvl = Lvl.NONE,
    bg: Color = MaterialTheme.colorScheme.surfaceContainerHigh,
    onClick: (() -> Unit)? = null,
) {
    Column(
        modifier.clip(RoundedCornerShape(16.dp)).background(bg)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (lvl != Lvl.NONE) {
                Box(Modifier.size(7.dp).clip(CircleShape).background(lvl.color()))
                Spacer(Modifier.width(5.dp))
            }
            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Row(verticalAlignment = Alignment.Bottom) {
            Text(value, style = MaterialTheme.typography.titleLarge.merge(NumStyle), fontWeight = FontWeight.SemiBold)
            if (unit != null) Text(" $unit", Modifier.padding(bottom = 3.dp), style = MaterialTheme.typography.bodySmall)
        }
        if (sub != null) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2)
    }
}

/** Число, которое «доезжает» до значения. */
@Composable
fun AnimatedNumber(
    value: Double,
    fmt: (Double) -> String,
    style: TextStyle,
    modifier: Modifier = Modifier,
    color: Color = Color.Unspecified,
    fontWeight: FontWeight? = FontWeight.Bold,
) {
    val a = remember { Animatable(0f) }
    LaunchedEffect(value) { a.animateTo(value.toFloat(), tween(700, easing = FastOutSlowInEasing)) }
    Text(fmt(a.value.toDouble()), modifier, style = style.merge(NumStyle), color = color, fontWeight = fontWeight)
}

/** Доля 0..1, плавно растущая с нуля при первом показе. */
@Composable
fun animatedFraction(target: Float, delayMs: Int = 0): Float {
    val a = remember { Animatable(0f) }
    LaunchedEffect(target) { a.animateTo(target.coerceIn(0f, 1f), tween(650, delayMs, FastOutSlowInEasing)) }
    return a.value
}

// ---------------- полосы ----------------

/** Полоса из долей (цвет, значение), выезжает слева. */
@Composable
fun StackBar(parts: List<Pair<Color, Double>>, modifier: Modifier = Modifier, height: Dp = 10.dp) {
    val total = parts.sumOf { it.second.coerceAtLeast(0.0) }
    val track = MaterialTheme.colorScheme.surfaceContainerHighest
    val p = animatedFraction(1f)
    Canvas(modifier.fillMaxWidth().height(height).clip(RoundedCornerShape(height / 2))) {
        drawRect(track)
        if (total <= 0) return@Canvas
        var x = 0f
        val w = size.width * p
        val gap = 2.dp.toPx()
        parts.filter { it.second > 0 }.forEach { (c, v) ->
            val pw = (v / total * w).toFloat()
            if (pw > 0.5f) drawRect(c, Offset(x, 0f), Size((pw - gap).coerceAtLeast(1f), size.height))
            x += pw
        }
    }
}

/** Полоса «доля от максимума». */
@Composable
fun ShareBar(fraction: Float, color: Color, modifier: Modifier = Modifier, height: Dp = 6.dp, delayMs: Int = 0) {
    val f = animatedFraction(fraction.coerceIn(0.004f, 1f), delayMs)
    val track = MaterialTheme.colorScheme.surfaceContainerHighest
    Canvas(modifier.fillMaxWidth().height(height)) {
        val r = CornerRadius(size.height / 2, size.height / 2)
        drawRoundRect(track, cornerRadius = r)
        drawRoundRect(color, size = Size(size.width * f, size.height), cornerRadius = r)
    }
}

/** Строка легенды: цвет, название, значение, доля. */
@Composable
fun LegendRow(color: Color, name: String, value: String, share: String? = null, sub: String? = null, help: String? = null) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(10.dp).clip(RoundedCornerShape(3.dp)).background(color))
        Spacer(Modifier.width(8.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(name, Modifier.weight(1f, fill = false), style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                if (help != null) {
                    val h = LocalHelp.current
                    Icon(
                        Icons.AutoMirrored.Outlined.HelpOutline, "что это",
                        Modifier.padding(start = 4.dp).size(16.dp).clip(CircleShape).clickable { h(help) },
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (sub != null) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (share != null) Text(share, Modifier.padding(start = 6.dp, end = 10.dp), style = MaterialTheme.typography.bodySmall.merge(NumStyle), color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.bodyMedium.merge(NumStyle), fontWeight = FontWeight.SemiBold)
    }
}

@Composable
fun Legend(items: List<Pair<Color, String>>) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        items.forEach { (c, s) ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(9.dp).clip(RoundedCornerShape(2.dp)).background(c))
                Spacer(Modifier.width(5.dp))
                Text(s, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

// ---------------- списки ----------------

/** Первые [initial] строк, остальное — по «Показать ещё» с анимацией. */
@Composable
fun <T> MoreList(items: List<T>, initial: Int = 5, key: String = "", row: @Composable (T) -> Unit) {
    var open by rememberSaveable(key) { mutableStateOf(false) }
    Column(Modifier.animateContentSize(tween(300))) {
        items.take(initial).forEach { row(it) }
        AnimatedVisibility(open && items.size > initial, enter = expandVertically() + fadeIn(), exit = shrinkVertically() + fadeOut()) {
            Column { items.drop(initial).forEach { row(it) } }
        }
        if (items.size > initial) {
            val rot by animateFloatAsState(if (open) 180f else 0f, label = "chev")
            TextButton(onClick = { open = !open }, contentPadding = ButtonDefaults.TextButtonContentPadding) {
                Text(if (open) "Свернуть" else "Показать ещё ${items.size - initial}")
                Icon(Icons.Outlined.ExpandMore, null, Modifier.rotate(rot))
            }
        }
    }
}

/** Строка «имя — счётчики — пояснение» для wakelock, будильников и т.п. */
@Composable
fun ItemRow(i: Item, max: Long, color: Color, showMs: Boolean = true) {
    Column(Modifier.padding(vertical = 5.dp)) {
        Row(verticalAlignment = Alignment.Top) {
            Text(i.name, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, maxLines = 3, overflow = TextOverflow.Ellipsis)
            Spacer(Modifier.width(8.dp))
            Text(
                listOfNotNull(if (i.count > 0) "${Fmt.count(i.count)}×" else null, if (showMs && i.ms > 0) Fmt.dur(i.ms) else null).joinToString(" · "),
                style = MaterialTheme.typography.bodyMedium.merge(NumStyle), fontWeight = FontWeight.SemiBold,
            )
        }
        if (max > 0) {
            Spacer(Modifier.height(3.dp))
            ShareBar((if (showMs && i.ms > 0) i.ms else i.count).toFloat() / max, color, height = 4.dp)
        }
        i.explain?.let { Text(it, Modifier.padding(top = 2.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
    }
}

@Composable
fun ItemList(list: List<Item>, color: Color, showMs: Boolean = true, initial: Int = 5, key: String) {
    val max = if (showMs) list.maxOfOrNull { if (it.ms > 0) it.ms else it.count } ?: 0 else list.maxOfOrNull { it.count } ?: 0
    MoreList(list, initial, key) { ItemRow(it, max, color, showMs) }
}

// ---------------- иконки приложений ----------------

@Composable
fun AppIcon(vm: Vm, a: App, size: Dp = 40.dp) {
    val ic = remember(a.uid, a.packages) { vm.iconOf(a) }
    if (ic != null) {
        Image(ic, null, Modifier.size(size).clip(CircleShape))
    } else {
        Letter(
            when (a.kind) {
                "kernel" -> "Я"
                "system" -> "S"
                else -> vm.labelOf(a).take(1).uppercase()
            },
            when (a.kind) {
                "kernel" -> Pal.cpu
                "system" -> Pal.radio
                "isolated" -> Pal.wifi
                else -> Pal.other
            },
            size,
        )
    }
}

@Composable
fun UidIcon(vm: Vm, uid: Int, name: String, size: Dp = 32.dp) {
    val ic = remember(uid) { vm.iconOfUid(uid) }
    if (ic != null) Image(ic, null, Modifier.size(size).clip(CircleShape))
    else Letter(if (uid == 0) "Я" else name.take(1).uppercase(), if (uid == 0) Pal.cpu else Pal.radio, size)
}

@Composable
private fun Letter(s: String, c: Color, size: Dp) {
    Box(Modifier.size(size).clip(CircleShape).background(c.copy(alpha = 0.22f)), contentAlignment = Alignment.Center) {
        Text(s, fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.onSurface)
    }
}

// ---------------- выводы ----------------

@Composable
fun InsightCard(i: Insight, onClick: (() -> Unit)?) {
    val lvl = lvlOf(i.level)
    val icon = when (lvl) {
        Lvl.BAD -> Icons.Outlined.ErrorOutline
        Lvl.WARN -> Icons.Outlined.WarningAmber
        Lvl.GOOD -> Icons.Outlined.CheckCircle
        else -> Icons.Outlined.Info
    }
    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp).clip(RoundedCornerShape(16.dp)).background(lvl.bg())
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, Modifier.size(22.dp), tint = lvl.color())
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Text(i.title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
            if (i.text.isNotBlank()) Text(i.text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.85f))
        }
        if (onClick != null) Icon(Icons.Outlined.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

// ---------------- загрузка, пусто, ошибка ----------------

@Composable
fun shimmerBrush(): Brush {
    val base = MaterialTheme.colorScheme.surfaceContainerHighest
    val hi = MaterialTheme.colorScheme.surfaceContainerLow
    val t = rememberInfiniteTransition(label = "sh")
    val x by t.animateFloat(-400f, 1400f, infiniteRepeatable(tween(1300, easing = LinearEasing), RepeatMode.Restart), label = "x")
    return Brush.linearGradient(listOf(base, hi, base), start = Offset(x, 0f), end = Offset(x + 400f, 200f))
}

@Composable
fun SkeletonLine(width: Float = 1f, height: Dp = 14.dp, brush: Brush) {
    Box(Modifier.fillMaxWidth(width).height(height).clip(RoundedCornerShape(6.dp)).background(brush))
}

@Composable
fun SkeletonCard(lines: Int = 3, chart: Boolean = false) {
    val b = shimmerBrush()
    Card(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp), shape = RoundedCornerShape(22.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            SkeletonLine(0.45f, 18.dp, b)
            if (chart) Box(Modifier.fillMaxWidth().height(120.dp).clip(RoundedCornerShape(12.dp)).background(b))
            repeat(lines) { SkeletonLine(if (it % 2 == 0) 0.9f else 0.7f, 12.dp, b) }
        }
    }
}

@Composable
fun SkeletonScreen(msg: String? = null) {
    Column {
        if (msg != null) Text(msg, Modifier.padding(horizontal = 20.dp, vertical = 8.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        SkeletonCard(lines = 2)
        SkeletonCard(lines = 3, chart = true)
        SkeletonCard(lines = 4)
    }
}

@Composable
fun EmptyState(icon: ImageVector, title: String, text: String, modifier: Modifier = Modifier, action: String? = null, onAction: (() -> Unit)? = null) {
    Column(modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 36.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Box(Modifier.size(64.dp).clip(CircleShape).background(MaterialTheme.colorScheme.secondaryContainer), contentAlignment = Alignment.Center) {
            Icon(icon, null, Modifier.size(32.dp), tint = MaterialTheme.colorScheme.onSecondaryContainer)
        }
        Spacer(Modifier.height(14.dp))
        Text(title, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
        Spacer(Modifier.height(6.dp))
        Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
        if (action != null && onAction != null) {
            Spacer(Modifier.height(14.dp))
            FilledTonalButton(onClick = onAction) { Text(action) }
        }
    }
}

@Composable
fun ErrorCard(title: String, msg: String, onRetry: (() -> Unit)? = null) {
    Surface(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp), shape = RoundedCornerShape(20.dp),
        color = MaterialTheme.colorScheme.errorContainer,
    ) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.ErrorOutline, null, tint = MaterialTheme.colorScheme.onErrorContainer)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onErrorContainer)
                Text(msg, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onErrorContainer, maxLines = 6, overflow = TextOverflow.Ellipsis)
            }
            if (onRetry != null) IconButton(onClick = onRetry) { Icon(Icons.Outlined.Refresh, "повторить", tint = MaterialTheme.colorScheme.onErrorContainer) }
        }
    }
}

@Composable
fun Centered(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally, content = content)
}

// ---------------- выбор периода ----------------

val RANGES = listOf(
    "since_unplug" to "С зарядки",
    "screen_off" to "Экран выкл.",
    "1h" to "1 ч",
    "3h" to "3 ч",
    "6h" to "6 ч",
    "12h" to "12 ч",
    "24h" to "24 ч",
    "48h" to "48 ч",
    "charge" to "По Android",
    "all" to "Всё",
)

fun rangeExplain(k: String) = when (k) {
    "since_unplug" -> "с момента, как телефон сняли с зарядки"
    "screen_off" -> "последний период с выключенным экраном"
    "charge" -> "с полной зарядки по статистике Android"
    "all" -> "всё, что записано"
    else -> "скользящее окно до текущего момента"
}

@Composable
fun RangeHeader(vm: Vm, r: Report?) {
    if (vm.fileMode) {
        // отчёт из файла: период задан файлом, переключать нечего
        Text(
            "Отчёт из файла: ${r?.label ?: "—"}" + if (r != null && r.to > r.from) " · ${Fmt.span(r.from, r.to)} (${Fmt.dur(r.to - r.from)})" else "",
            Modifier.padding(horizontal = 16.dp, vertical = 8.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        return
    }
    Column(Modifier.padding(top = 4.dp)) {
        Row(
            Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            RANGES.forEach { (k, l) -> FilterChip(selected = vm.range == k, onClick = { vm.selectRange(k) }, label = { Text(l) }) }
        }
        Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                buildString {
                    append(rangeExplain(vm.range))
                    if (r != null && r.to > r.from) append(" · ${Fmt.span(r.from, r.to)} (${Fmt.dur(r.to - r.from)})")
                },
                Modifier.weight(1f), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            HelpButton("range")
        }
    }
}

/** Полоса предупреждения о покрытии данными. */
@Composable
fun CoverageNote(r: Report) {
    if (r.recordedFrom <= r.from + 300_000) return
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp).clip(RoundedCornerShape(14.dp))
            .background(Lvl.INFO.bg()).padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Outlined.Info, null, Modifier.size(18.dp), tint = Lvl.INFO.color())
        Spacer(Modifier.width(8.dp))
        Text(
            "Ядро пишет с ${Fmt.human(r.recordedFrom)}: процессы, потоки и ток — только с этого момента, " +
                "за начало периода — статистика Android.",
            Modifier.weight(1f), style = MaterialTheme.typography.bodySmall,
        )
        HelpButton("coverage")
    }
}

/** Плашка «нет root — показан файл». */
@Composable
fun FileModeNote() {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp).clip(RoundedCornerShape(14.dp))
            .background(Lvl.WARN.bg()).padding(12.dp),
    ) {
        Icon(Icons.Outlined.Info, null, Modifier.size(18.dp), tint = Lvl.WARN.color())
        Spacer(Modifier.width(8.dp))
        Text("Нет root — показан сохранённый отчёт report.json. Живые данные, отрезки и выгрузка недоступны.", style = MaterialTheme.typography.bodySmall)
    }
}

@Composable
fun FillWidthBox(content: @Composable () -> Unit) {
    Box(Modifier.fillMaxWidth().fillMaxHeight()) { content() }
}
