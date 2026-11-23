@file:OptIn(ExperimentalMaterial3Api::class)

package dev.niazlv.zhor

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.HelpOutline
import androidx.compose.material.icons.outlined.Bedtime
import androidx.compose.material.icons.outlined.BatteryChargingFull
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.automirrored.outlined.CompareArrows
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.School
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material.icons.outlined.TaskAlt
import androidx.compose.material.icons.outlined.TipsAndUpdates
import androidx.compose.material.icons.outlined.TouchApp
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/** Тело термина: что это, подробно, нормы, что делать. */
@Composable
fun TermBody(t: Term, vm: Vm?, showTitle: Boolean = true) {
    Column {
        if (showTitle) {
            Text(t.title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(6.dp))
        }
        Text(t.short, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium)
        Spacer(Modifier.height(8.dp))
        Text(t.body, style = MaterialTheme.typography.bodyMedium)
        if (t.key == "mah" || t.key == "battery_health") {
            val full = vm?.report?.fullMah ?: 0.0
            if (full > 0) Explain("У вашего телефона полная ёмкость сейчас ${Fmt.d0(full)} мА·ч, то есть 1 % ≈ ${Fmt.d0(full / 100)} мА·ч.")
        }
        t.norm?.let { Block(Icons.Outlined.TaskAlt, "Как оценивать", it, Lvl.GOOD) }
        t.todo?.let { Block(Icons.Outlined.TipsAndUpdates, "Что делать", it, Lvl.INFO) }
    }
}

@Composable
private fun Block(icon: ImageVector, title: String, text: String, lvl: Lvl) {
    Row(Modifier.padding(top = 10.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(lvl.bg()).padding(12.dp)) {
        Icon(icon, null, Modifier.size(20.dp), tint = lvl.color())
        Spacer(Modifier.width(10.dp))
        Column {
            Text(title, style = MaterialTheme.typography.labelLarge, color = lvl.color())
            Text(text, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

private val NORMS = listOf(
    Triple("Разряд без экрана", "< 1 %/ч", "> 3 %/ч"),
    Triple("Глубокий сон без экрана", "> 90 %", "< 70 %"),
    Triple("Ток во сне", "5–20 мА", "> 80 мА"),
    Triple("Ток с экраном", "200–500 мА", "> 1200 мА"),
    Triple("Срывы засыпания", "< 10 %", "> 30 %"),
    Triple("«Прочее» от факта", "< 30 %", "> 45 %"),
    Triple("Ёмкость от паспортной", "> 90 %", "< 80 %"),
    Triple("Температура батареи", "< 38 °C", "> 43 °C"),
)

@Composable
fun GuideScreen(vm: Vm, navi: Navi, term: String?) {
    var q by rememberSaveable { mutableStateOf("") }
    var open by rememberSaveable { mutableStateOf(term) }
    val state = rememberLazyListState()
    val needle = q.trim().lowercase()
    val list = Glossary.terms.filter {
        needle.isEmpty() || it.title.lowercase().contains(needle) || it.short.lowercase().contains(needle) || it.body.lowercase().contains(needle)
    }
    LaunchedEffect(term) {
        if (term != null) {
            // 3 = поиск, нормы, знакомство; дальше — заголовки категорий и термины
            var idx = 3
            for (c in Glossary.cats) {
                val inCat = Glossary.terms.filter { it.cat == c }
                if (inCat.isEmpty()) continue
                idx++
                val i = inCat.indexOfFirst { it.key == term }
                if (i >= 0) {
                    state.scrollToItem(idx + i)
                    break
                }
                idx += inCat.size
            }
        }
    }
    Scaffold(topBar = { DetailTopBar("Как читать", "термины, нормы и что делать", onBack = { navi.back() }) }) { pad ->
        LazyColumn(Modifier.padding(pad).fillMaxSize(), state = state, contentPadding = PaddingValues(bottom = 32.dp)) {
            item(key = "q") {
                OutlinedTextField(
                    q, { q = it }, Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
                    placeholder = { Text("Найти термин", maxLines = 1) },
                    leadingIcon = { Icon(Icons.Outlined.Search, null) },
                    trailingIcon = { if (q.isNotEmpty()) IconButton(onClick = { q = "" }) { Icon(Icons.Outlined.Close, "очистить") } },
                    singleLine = true, shape = RoundedCornerShape(28.dp),
                )
            }
            item(key = "norms") {
                if (needle.isEmpty()) Section("Нормы одним взглядом", sub = "для телефона в обычном использовании") {
                    Row(Modifier.padding(bottom = 4.dp)) {
                        Text("", Modifier.weight(1f))
                        Text("хорошо", Modifier.width(92.dp), style = MaterialTheme.typography.labelMedium, color = Lvl.GOOD.color(), textAlign = TextAlign.End)
                        Text("плохо", Modifier.width(80.dp), style = MaterialTheme.typography.labelMedium, color = Lvl.BAD.color(), textAlign = TextAlign.End)
                    }
                    NORMS.forEach { (n, g, b) ->
                        Row(Modifier.padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(n, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                            Text(g, Modifier.width(92.dp), style = MaterialTheme.typography.bodyMedium.merge(NumStyle), textAlign = TextAlign.End)
                            Text(b, Modifier.width(80.dp), style = MaterialTheme.typography.bodyMedium.merge(NumStyle), textAlign = TextAlign.End)
                        }
                    }
                }
            }
            item(key = "intro") {
                if (needle.isEmpty()) Section("С чего начать") {
                    Text(
                        "1. «Сводка» → «Главное»: выводы по периоду.\n" +
                            "2. «Экран и сон»: если с выключенным экраном уходит больше 1–3 %/ч — смотрите вкладку «Сон», кто будит.\n" +
                            "3. «Приложения» → «Экран выкл.»: кто работает, пока вы не пользуетесь телефоном.\n" +
                            "4. Карточка приложения: процессы, потоки, wakelock и что с этим делать.\n" +
                            "5. Лента: коснитесь подозрительного всплеска и откройте этот отрезок.",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    TextButton(onClick = { navi.onboarding() }) {
                        Icon(Icons.Outlined.School, null)
                        Spacer(Modifier.width(6.dp))
                        Text("Показать знакомство")
                    }
                }
            }
            if (list.isEmpty()) item(key = "empty") { EmptyState(Icons.Outlined.SearchOff, "Не нашёл", "Нет терминов с «$q». Попробуйте другое слово.") }
            Glossary.cats.forEach { c ->
                val inCat = list.filter { it.cat == c }
                if (inCat.isNotEmpty()) {
                    item(key = "c$c") {
                        Text(c, Modifier.padding(start = 20.dp, top = 16.dp, bottom = 4.dp), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
                    }
                    items(inCat, key = { it.key }) { t ->
                        TermCard(t, vm, open == t.key || needle.isNotEmpty() && list.size <= 2) { open = if (open == t.key) null else t.key }
                    }
                }
            }
        }
    }
}

@Composable
private fun TermCard(t: Term, vm: Vm, expanded: Boolean, onToggle: () -> Unit) {
    val rot by animateFloatAsState(if (expanded) 180f else 0f, label = "r")
    val bg by animateColorAsState(if (expanded) MaterialTheme.colorScheme.surfaceContainer else MaterialTheme.colorScheme.surfaceContainerLow, label = "bg")
    val pad by animateDpAsState(if (expanded) 16.dp else 12.dp, label = "p")
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 3.dp).clip(RoundedCornerShape(18.dp)).background(bg)
            .clickable(onClick = onToggle).padding(horizontal = 16.dp, vertical = pad).animateContentSize(),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(t.title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                if (!expanded) Text(t.short, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Icon(Icons.Outlined.ExpandMore, null, Modifier.rotate(rot), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        AnimatedVisibility(expanded, enter = expandVertically() + fadeIn(), exit = shrinkVertically() + fadeOut()) {
            Column(Modifier.padding(top = 8.dp)) { TermBody(t, vm, showTitle = false) }
        }
    }
}

// ---------------- знакомство ----------------

private data class Page(val icon: ImageVector, val title: String, val text: String)

private val PAGES = listOf(
    Page(
        Icons.Outlined.BatteryChargingFull, "Кто тратит батарею",
        "Жор считает энергию каждого процесса и даже потока: сколько он работал на каждой частоте процессора. " +
            "И сверяет это с реальным счётчиком заряда в батарее.",
    ),
    Page(
        Icons.AutoMirrored.Outlined.CompareArrows, "Факт и модель",
        "Факт — сколько реально ушло по батарее. Модель — оценка по процессору и экрану: она знает, кто тратил. " +
            "Разница — «прочее»: связь, GPS, Wi-Fi, видеоускоритель.",
    ),
    Page(
        Icons.Outlined.Bedtime, "Главное — сон",
        "С выключенным экраном исправный телефон спит больше 90 % времени и теряет меньше 1 % в час. " +
            "Если нет — вкладка «Сон» покажет, кто его будит.",
    ),
    Page(
        Icons.Outlined.TouchApp, "Графики можно трогать",
        "Коснитесь ленты или проведите по ней пальцем: увидите ток, модель и кто тратил в этой точке. " +
            "«Открыть этот отрезок» — полный отчёт только за него.",
    ),
    Page(
        Icons.AutoMirrored.Outlined.HelpOutline, "Знак вопроса рядом с числом",
        "Объясняет, что это за число, какое значение хорошее, какое плохое и что делать. " +
            "Все термины и нормы — в разделе «Как читать».",
    ),
)

@Composable
fun Onboarding(onDone: () -> Unit) {
    val pager = rememberPagerState { PAGES.size }
    val scope = rememberCoroutineScope()
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
        Column(Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding()) {
            Row(Modifier.fillMaxWidth().padding(8.dp), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = onDone) { Text("Пропустить") }
            }
            HorizontalPager(pager, Modifier.weight(1f)) { i ->
                val p = PAGES[i]
                Column(Modifier.fillMaxSize().padding(horizontal = 32.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                    val off = (pager.currentPage - i) + pager.currentPageOffsetFraction
                    val scale = 1f - kotlin.math.abs(off).coerceAtMost(1f) * 0.25f
                    Box(
                        Modifier.size(132.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(p.icon, null, Modifier.size((64 * scale).dp), tint = MaterialTheme.colorScheme.onPrimaryContainer)
                    }
                    Spacer(Modifier.height(32.dp))
                    Text(p.title, style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
                    Spacer(Modifier.height(14.dp))
                    Text(p.text, style = MaterialTheme.typography.bodyLarge, textAlign = TextAlign.Center, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            Row(Modifier.fillMaxWidth().padding(24.dp), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    repeat(PAGES.size) { i ->
                        val w by animateDpAsState(if (pager.currentPage == i) 22.dp else 8.dp, label = "dot")
                        val c by animateColorAsState(
                            if (pager.currentPage == i) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant, label = "dc",
                        )
                        Box(Modifier.height(8.dp).width(w).clip(CircleShape).background(c))
                    }
                }
                val last = pager.currentPage == PAGES.size - 1
                Button(onClick = { if (last) onDone() else scope.launch { pager.animateScrollToPage(pager.currentPage + 1) } }) {
                    Text(if (last) "Начать" else "Дальше")
                }
            }
        }
    }
}
