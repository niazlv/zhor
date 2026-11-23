@file:OptIn(ExperimentalMaterial3Api::class)

package dev.niazlv.zhor

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

private data class Sort(val title: String, val explain: String, val help: String)

private val SORTS = listOf(
    Sort("Всего", "Всё, что потратило приложение: процессор по ядру Жора и остальные компоненты (экран, радио, GPS…) по модели Android.", "app_total"),
    Sort("Экран выкл.", "Процессор, пока экран был выключен, — главный признак фонового жора. Тут не должно быть приложений, которыми вы не пользовались.", "screen_off_cpu"),
    Sort("Процессор", "Только процессор, посчитанный ядром по потокам и частотам.", "model"),
    Sort("Вне экрана", "Сколько потрачено не на первом плане: в фоне, фоновой службой, в кэше (модель Android).", "states"),
    Sort("Wakelock", "Сколько времени приложение не давало телефону уснуть.", "wakelock"),
    Sort(
        "Нагружают систему",
        "Сколько system_server потратил по запросам приложения (binder-вызовы и задачи в очередях). Эти мА·ч входят в «Android OS», доля — от всей работы system.",
        "system_work",
    ),
)

@Composable
fun AppsTab(vm: Vm, navi: Navi) {
    val r = vm.report
    var sort by rememberSaveable { mutableIntStateOf(0) }
    var q by rememberSaveable { mutableStateOf("") }
    var appsOnly by rememberSaveable { mutableStateOf(false) }
    PullToRefreshBox(isRefreshing = vm.loading && r != null, onRefresh = { vm.refresh(fresh = true) }, modifier = Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
            item(key = "range") { RangeHeader(vm, r) }
            item(key = "search") {
                OutlinedTextField(
                    q, { q = it }, Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
                    placeholder = { Text("Найти приложение или процесс", maxLines = 1) },
                    leadingIcon = { Icon(Icons.Outlined.Search, null) },
                    trailingIcon = { if (q.isNotEmpty()) IconButton(onClick = { q = "" }) { Icon(Icons.Outlined.Close, "очистить") } },
                    singleLine = true, shape = RoundedCornerShape(28.dp),
                )
            }
            item(key = "sort") {
                Row(Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    SORTS.forEachIndexed { i, s -> FilterChip(selected = sort == i, onClick = { sort = i }, label = { Text(s.title) }) }
                    FilterChip(selected = appsOnly, onClick = { appsOnly = !appsOnly }, label = { Text("Без системных") })
                }
            }
            if (r == null) {
                if (vm.error != null) item { ErrorCard("Отчёт не загрузился", vm.error!!) { vm.refresh() } } else item { SkeletonScreen() }
                return@LazyColumn
            }
            val total = r.totals.measuredMah.takeIf { it > 0 } ?: r.apps.sumOf { it.mah }
            val wl = { a: App -> a.wakelocks.sumOf { it.ms } }
            val value: (App) -> Double = when (sort) {
                1 -> { a -> a.cpuMahScreenOff }
                2 -> { a -> a.cpuMah }
                3 -> { a -> a.bgMah }
                4 -> { a -> wl(a).toDouble() }
                5 -> { a -> if (a.uid == 1000) 0.0 else a.systemMah }
                else -> { a -> a.mah }
            }
            val needle = q.trim().lowercase()
            val list = r.apps.asSequence()
                .filter { !appsOnly || it.kind == "app" }
                .filter { a ->
                    needle.isEmpty() || vm.labelOf(a).lowercase().contains(needle) ||
                        a.packages.any { it.lowercase().contains(needle) } || a.processes.any { it.name.lowercase().contains(needle) }
                }
                .filter { sort == 0 || value(it) > 0.0005 }
                .sortedByDescending(value).toList()
            val max = list.firstOrNull()?.let(value)?.takeIf { it > 0 } ?: 1.0
            item(key = "explain") {
                Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
                    Explain(SORTS[sort].explain)
                    Text(
                        "${list.size} из ${r.apps.size} · всего по счётчику ${Fmt.mah(r.totals.measuredMah)} мА·ч, " +
                            "CPU по ядру ${Fmt.mah(r.apps.sumOf { it.cpuMah })} мА·ч",
                        Modifier.padding(top = 4.dp), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (list.isEmpty()) {
                item(key = "empty") {
                    if (needle.isNotEmpty()) EmptyState(Icons.Outlined.SearchOff, "Ничего не нашлось", "Нет приложений, пакетов или процессов с «$q» в этом периоде.")
                    else EmptyState(Icons.Outlined.SearchOff, "Пусто", "В этом периоде никто не тратил по выбранному признаку — это хорошо.")
                }
            }
            items(list, key = { it.uid }) { a ->
                val v = value(a)
                val sub = when (sort) {
                    1 -> if (a.cpuMah > 0) "${Fmt.pctRaw(100 * a.cpuMahScreenOff / a.cpuMah)} % своего процессора — при выключенном экране" else null
                    3 -> a.states.filter { it.key != "fg" && it.value > 0.005 }.entries.joinToString(" · ") { "${Fmt.stateName(it.key)} ${Fmt.mah(it.value)}" }.ifEmpty { null }
                    4 -> a.wakelocks.firstOrNull()?.let { "чаще всего «${it.name}» · ${Fmt.times(it.count)}" }
                    5 -> a.systemCalls.firstOrNull()?.let { c -> "CPU системы ${Fmt.dur(a.systemMs.toLong())} · чаще всего ${c.call}" + (c.explain?.let { " — ${it.replaceFirstChar { ch -> ch.lowercase() }}" } ?: "") }
                        ?: "CPU системы ${Fmt.dur(a.systemMs.toLong())} — задачи в её очередях"
                    else -> a.why.firstOrNull()?.let { vm.nice(it, r) }
                }
                val idx = remember(a.uid) { list.indexOf(a) }
                Column(Modifier.padding(horizontal = 12.dp).animateItem()) {
                    AppRow(
                        vm, a, v, max, when (sort) {
                            4 -> 0.0
                            5 -> r.systemWork?.mahTotal ?: 0.0
                            else -> total
                        }, sub, idx,
                        valueText = if (sort == 4) Fmt.dur(v.toLong()) else "${Fmt.mah(v)} мА·ч",
                        parts = when (sort) {
                            1, 2 -> listOf(Pal.cpu to v)
                            3 -> a.states.filter { it.key != "fg" }.map { Pal.state(it.key) to it.value }
                            4 -> listOf(Pal.wakelock to v)
                            5 -> listOf(Pal.wifi to v)
                            else -> null
                        },
                    ) { navi.app(a.uid) }
                }
            }
        }
    }
}
