package dev.niazlv.zhor

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Block
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.ZoomIn
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@Composable
fun SegmentScreen(vm: Vm, navi: Navi, from: Long, to: Long) {
    LaunchedEffect(from, to) { vm.loadSegment(from, to) }
    val key = vm.segKey(from, to)
    val st = vm.segs[key]?.res ?: Res.Loading
    Scaffold(
        topBar = {
            DetailTopBar("Отрезок ${Fmt.span(from, to)}", "${Fmt.date(from)} · ${Fmt.dur(to - from)}", onBack = { navi.back() }) {
                IconButton(onClick = { vm.loadSegment(from, to, force = true) }, enabled = st !is Res.Loading) { Icon(Icons.Outlined.Refresh, "пересчитать") }
            }
        },
    ) { pad ->
        AnimatedContent(st, Modifier.padding(pad), contentKey = { it::class }, transitionSpec = { fadeIn(tween(250)) togetherWith fadeOut(tween(120)) }, label = "seg") { s ->
            when (s) {
                is Res.Ok -> SegmentBody(vm, navi, s.v, key)
                is Res.Err -> Column {
                    ErrorCard("Отчёт за отрезок не построен", s.msg) { vm.loadSegment(from, to, force = true) }
                }
                else -> SkeletonScreen("Считаю полный отчёт за отрезок…")
            }
        }
    }
}

@Composable
private fun SegmentBody(vm: Vm, navi: Navi, r: Report, key: String) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
        item(key = "intro") {
            Row(Modifier.padding(horizontal = 16.dp, vertical = 6.dp)) {
                Icon(Icons.Outlined.ZoomIn, null, tint = MaterialTheme.colorScheme.primary)
                Text(
                    "Полный отчёт только за этот кусок графика. На ленте можно выбрать точку и приблизиться ещё.",
                    Modifier.padding(start = 8.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        reportOverview(r, vm, navi, seg = key, goTab = null)
        r.history?.let { h ->
            if (h.lanes.isNotEmpty() || h.items.isNotEmpty()) item(key = "hist") { PhoneHistorySection(vm, navi, r, h, key) }
        }
        val k = r.kernel
        if (k.wakeupSources.isNotEmpty()) item(key = "ws") {
            Section("Кто будил и держал", sub = "источники пробуждения в этом отрезке", help = "wakeup_source", icon = Icons.Outlined.NotificationsActive) {
                val max = k.wakeupSources.maxOf { it.heldMs }.coerceAtLeast(1)
                MoreList(k.wakeupSources, initial = 5, key = "sws$key") { w ->
                    Column(Modifier.padding(vertical = 4.dp)) {
                        Row {
                            Text(w.name, Modifier.weight(1f), fontFamily = FontFamily.Monospace, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text("${Fmt.dur(w.heldMs)} · ${Fmt.times(w.wakeups)}", style = MaterialTheme.typography.bodySmall.merge(NumStyle))
                        }
                        ShareBar(w.heldMs.toFloat() / max, Pal.radio, height = 4.dp)
                    }
                }
            }
        }
        if (k.abortReasons.isNotEmpty()) item(key = "ab") {
            Section("Почему не засыпал", help = "abort", icon = Icons.Outlined.Block) { ItemList(k.abortReasons, Pal.cpu, showMs = false, key = "sab$key") }
        }
    }
}
