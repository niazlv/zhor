@file:OptIn(ExperimentalMaterial3Api::class)

package dev.niazlv.zhor

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.HelpOutline
import androidx.compose.material.icons.filled.Apps
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.Dashboard
import androidx.compose.material.icons.outlined.Apps
import androidx.compose.material.icons.outlined.Bedtime
import androidx.compose.material.icons.outlined.Bolt
import androidx.compose.material.icons.outlined.Dashboard
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.automirrored.outlined.MenuBook
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.navigation.NavHostController
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument

/** Переходы между экранами. Двойной тап во время анимации не открывает экран дважды. */
class Navi(val nav: NavHostController) {
    private fun go(route: String) {
        if (nav.currentBackStackEntry?.lifecycle?.currentState?.isAtLeast(Lifecycle.State.RESUMED) != false) nav.navigate(route)
    }

    fun app(uid: Int, seg: String? = null) = go(if (seg == null) "app/$uid" else "app/$uid?seg=$seg")
    fun seg(from: Long, to: Long) = go("seg/$from/$to")
    fun settings() = go("settings")
    fun data() = go("data")
    fun guide(term: String? = null) = go(if (term == null) "guide" else "guide?term=$term")
    fun onboarding() = go("onboarding")
    fun watch() = go("watch")
    fun compare(r: CmpReq) = go("compare?mode=${r.mode}&t=${r.t ?: 0}&uid=${r.uid ?: -1}")
    fun back() {
        if (nav.previousBackStackEntry != null) nav.popBackStack()
    }
}

private const val NAV_MS = 320

@Composable
fun ZhorApp(vm: Vm) {
    val nav = rememberNavController()
    val navi = remember(nav) { Navi(nav) }
    var helpKey by rememberSaveable { mutableStateOf<String?>(null) }
    val snack = remember { SnackbarHostState() }
    LaunchedEffect(vm.toast) {
        vm.toast?.let {
            snack.showSnackbar(it)
            vm.toast = null
        }
    }
    val start = remember { if (vm.onboarded) "home" else "onboarding" }
    // нажали на уведомление сторожа: отчёт за тот период, поверх — приложение, если оно названо
    // нажали на уведомление об обновлении — в настройки, там блок «Обновления»
    LaunchedEffect(vm.pendingSettings, vm.ready) {
        if (vm.pendingSettings && vm.ready) {
            vm.pendingSettings = false
            nav.navigate("settings")
        }
    }
    LaunchedEffect(vm.pendingAlert, vm.ready) {
        val p = vm.pendingAlert ?: return@LaunchedEffect
        if (!vm.ready) return@LaunchedEffect
        vm.pendingAlert = null
        val (from, to, uid) = p
        vm.loadSegment(from, to)
        nav.navigate("seg/$from/$to")
        if (uid != null) nav.navigate("app/$uid?seg=${vm.segKey(from, to)}")
    }
    CompositionLocalProvider(LocalHelp provides { helpKey = it }) {
        Box(Modifier.fillMaxSize()) {
            NavHost(
                nav, startDestination = start,
                enterTransition = { slideInHorizontally(tween(NAV_MS, easing = FastOutSlowInEasing)) { it / 5 } + fadeIn(tween(NAV_MS)) },
                exitTransition = { slideOutHorizontally(tween(NAV_MS, easing = FastOutSlowInEasing)) { -it / 10 } + fadeOut(tween(NAV_MS / 2)) },
                popEnterTransition = { slideInHorizontally(tween(NAV_MS, easing = FastOutSlowInEasing)) { -it / 10 } + fadeIn(tween(NAV_MS)) },
                popExitTransition = { slideOutHorizontally(tween(NAV_MS, easing = FastOutSlowInEasing)) { it / 5 } + fadeOut(tween(NAV_MS / 2)) },
            ) {
                composable("onboarding") {
                    Onboarding(onDone = {
                        val again = nav.previousBackStackEntry != null
                        vm.setOnboarded()
                        if (again) nav.popBackStack() else nav.navigate("home") { popUpTo("onboarding") { inclusive = true } }
                    })
                }
                composable("home") { Home(vm, navi) }
                composable(
                    "app/{uid}?seg={seg}",
                    arguments = listOf(
                        navArgument("uid") { type = NavType.IntType },
                        navArgument("seg") { type = NavType.StringType; nullable = true; defaultValue = null },
                    ),
                ) { e -> AppDetailScreen(vm, navi, e.arguments?.getInt("uid") ?: 0, e.arguments?.getString("seg")) }
                composable(
                    "seg/{from}/{to}",
                    arguments = listOf(navArgument("from") { type = NavType.LongType }, navArgument("to") { type = NavType.LongType }),
                ) { e -> SegmentScreen(vm, navi, e.arguments?.getLong("from") ?: 0, e.arguments?.getLong("to") ?: 0) }
                composable("settings") { SettingsScreen(vm, navi) }
                composable("watch") { WatchScreen(vm, navi) }
                composable(
                    "compare?mode={mode}&t={t}&uid={uid}",
                    arguments = listOf(
                        navArgument("mode") { type = NavType.StringType; defaultValue = "rest" },
                        navArgument("t") { type = NavType.LongType; defaultValue = 0L },
                        navArgument("uid") { type = NavType.IntType; defaultValue = -1 },
                    ),
                ) { e ->
                    val a = e.arguments
                    val req = CmpReq(a?.getString("mode") ?: "rest", a?.getLong("t")?.takeIf { it > 0 }, a?.getInt("uid")?.takeIf { it >= 0 })
                    CompareScreen(vm, navi, req)
                }
                composable("data") { DataScreen(vm, navi) }
                composable(
                    "guide?term={term}",
                    arguments = listOf(navArgument("term") { type = NavType.StringType; nullable = true; defaultValue = null }),
                ) { e -> GuideScreen(vm, navi, e.arguments?.getString("term")) }
            }
            SnackbarHost(snack, Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 88.dp))
        }
    }
    helpKey?.let { key ->
        val term = Glossary.get(key)
        val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
        ModalBottomSheet(onDismissRequest = { helpKey = null }, sheetState = sheet) {
            Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp).padding(bottom = 24.dp)) {
                if (term == null) Text("Нет описания для «$key».") else TermBody(term, vm)
                Spacer(Modifier.height(12.dp))
                TextButton(onClick = { helpKey = null; navi.guide(key) }) {
                    Icon(Icons.AutoMirrored.Outlined.MenuBook, null)
                    Spacer(Modifier.padding(4.dp))
                    Text("Все термины и нормы")
                }
            }
        }
    }
}

private data class Tab(val title: String, val sub: String, val icon: ImageVector, val iconSel: ImageVector)

private val TABS = listOf(
    Tab("Сводка", "что жрёт батарею", Icons.Outlined.Dashboard, Icons.Filled.Dashboard),
    Tab("Приложения", "кто и сколько потратил", Icons.Outlined.Apps, Icons.Filled.Apps),
    Tab("Сон", "спит ли телефон и кто будит", Icons.Outlined.Bedtime, Icons.Filled.Bedtime),
    Tab("Сейчас", "ток батареи в реальном времени", Icons.Outlined.Bolt, Icons.Filled.Bolt),
)

/** Сторож включён, а уведомления запрещены: окно «нужны уведомления» — раз за запуск и не чаще раза в 3 дня. */
@Composable
private fun NotifPrompt(vm: Vm) {
    val watchOn = vm.alerts.value()?.watch == true
    if (!vm.ready || vm.fileMode || vm.report == null || !watchOn || vm.notifAllowed || vm.notifPromptShown || !vm.notifPromptDue()) return
    val ask = rememberNotifRequest(vm)
    androidx.compose.material3.AlertDialog(
        onDismissRequest = { vm.notifPromptShown = true },
        icon = { Icon(Icons.Outlined.NotificationsActive, null) },
        title = { Text("Сторожу нужны уведомления") },
        text = {
            Text(
                "Сторож присылает уведомление в тот момент, когда телефон не спит, батарея тает в покое или приложение " +
                    "работает в фоне. Без уведомлений тревоги будут только копиться в списке внутри Жора.\n\n" +
                    "Выключить сторожа можно в настройках → «Сторож и «до/после»».",
            )
        },
        confirmButton = { TextButton(onClick = { vm.notifPromptShown = true; ask() }) { Text("Включить") } },
        dismissButton = { TextButton(onClick = { vm.notifPromptShown = true; vm.notifPromptLater() }) { Text("Не сейчас") } },
    )
}

@Composable
private fun Home(vm: Vm, navi: Navi) {
    NotifPrompt(vm)
    var tab by rememberSaveable { mutableIntStateOf(0) }
    BackHandler(tab != 0) { tab = 0 }
    val holder = rememberSaveableStateHolder()
    val scroll = TopAppBarDefaults.pinnedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            Column {
                TopAppBar(
                    title = {
                        Column {
                            Text("Жор", fontWeight = FontWeight.Bold)
                            val sub = when {
                                tab == 3 || vm.report == null -> TABS[tab].sub
                                vm.fileMode -> "отчёт из файла"
                                else -> "${TABS[tab].sub} · обновлено ${Fmt.ago(vm.updatedAt)}"
                            }
                            AnimatedContent(sub, transitionSpec = { fadeIn(tween(200)) togetherWith fadeOut(tween(120)) }, label = "sub") { s ->
                                Text(s, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                    },
                    actions = {
                        if (tab != 3) IconButton(onClick = { vm.refresh(fresh = true) }, enabled = vm.ready && !vm.loading && !vm.fileMode) {
                            Icon(Icons.Outlined.Refresh, "обновить со свежим снимком")
                        }
                        IconButton(onClick = { navi.guide() }) { Icon(Icons.AutoMirrored.Outlined.HelpOutline, "как читать") }
                        IconButton(onClick = { navi.settings() }) { Icon(Icons.Outlined.Settings, "настройки") }
                    },
                    scrollBehavior = scroll,
                )
                AnimatedVisibility(vm.loading && tab != 3) { LinearProgressIndicator(Modifier.fillMaxWidth()) }
            }
        },
        bottomBar = {
            NavigationBar {
                TABS.forEachIndexed { i, t ->
                    NavigationBarItem(
                        selected = tab == i, onClick = { tab = i },
                        icon = { Icon(if (tab == i) t.iconSel else t.icon, null) }, label = { Text(t.title) },
                    )
                }
            }
        },
    ) { pad ->
        Box(Modifier.padding(pad).fillMaxSize()) {
            when {
                vm.starting -> SkeletonScreen("Запускаю ядро (root)…")
                !vm.ready -> StartError(vm)
                else -> AnimatedContent(
                    tab,
                    transitionSpec = {
                        (fadeIn(tween(220, delayMillis = 60)) + scaleIn(tween(220, delayMillis = 60), initialScale = 0.98f)) togetherWith fadeOut(tween(90))
                    },
                    label = "tab",
                ) { t ->
                    holder.SaveableStateProvider(t) {
                        when (t) {
                            0 -> SummaryTab(vm, navi) { tab = it }
                            1 -> AppsTab(vm, navi)
                            2 -> SleepTab(vm, navi)
                            else -> LiveTab(vm, navi)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun StartError(vm: Vm) {
    Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        EmptyState(
            Icons.Outlined.Lock, "Жору нужен root",
            (vm.error ?: "Ядро не запустилось.") +
                "\n\nЖор читает статистику ядра Linux и батареи, доступную только root. Разрешите root в KernelSU или Magisk и нажмите «Повторить». " +
                "Без root можно посмотреть отчёт из файла: положите выгрузку `zhord report --json` в Android/data/dev.niazlv.zhor/files/report.json.",
            action = "Повторить", onAction = { vm.start() },
        )
    }
}

/** Верхняя панель вложенных экранов. */
@Composable
fun DetailTopBar(title: String, sub: String?, onBack: () -> Unit, actions: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit = {}) {
    TopAppBar(
        title = {
            Column {
                Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (sub != null) Text(sub, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        },
        navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "назад") } },
        actions = actions,
    )
}
