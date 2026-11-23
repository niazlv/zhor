package dev.niazlv.zhor

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle

/** Оценка числа: хорошо / так себе / плохо / просто сведения. */
enum class Lvl { GOOD, WARN, BAD, INFO, NONE }

@Immutable
data class Semantic(
    val good: Color, val onGood: Color, val goodBg: Color,
    val warn: Color, val onWarn: Color, val warnBg: Color,
    val bad: Color, val onBad: Color, val badBg: Color,
    val info: Color, val infoBg: Color,
)

private val LightSem = Semantic(
    good = Color(0xFF2E7D4F), onGood = Color.White, goodBg = Color(0xFFD7F0DF),
    warn = Color(0xFFA86500), onWarn = Color.White, warnBg = Color(0xFFFFE8C2),
    bad = Color(0xFFC62828), onBad = Color.White, badBg = Color(0xFFFFDAD6),
    info = Color(0xFF3567B5), infoBg = Color(0xFFDCE6FA),
)
private val DarkSem = Semantic(
    good = Color(0xFF7FD69E), onGood = Color(0xFF00391C), goodBg = Color(0xFF1E3B2A),
    warn = Color(0xFFFFC266), onWarn = Color(0xFF452B00), warnBg = Color(0xFF443214),
    bad = Color(0xFFFF8A80), onBad = Color(0xFF5C0A0A), badBg = Color(0xFF4A2120),
    info = Color(0xFF9DBDF5), infoBg = Color(0xFF223250),
)

val LocalSem = staticCompositionLocalOf { LightSem }

@Composable
fun ZhorTheme(content: @Composable () -> Unit) {
    val ctx = LocalContext.current
    val dark = isSystemInDarkTheme()
    val scheme = if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
    // цифры одной ширины: анимированные значения не «дрожат»
    val base = Typography()
    val tnum = "tnum"
    val typo = base.copy(
        displayLarge = base.displayLarge.copy(fontFeatureSettings = tnum),
        displayMedium = base.displayMedium.copy(fontFeatureSettings = tnum),
        displaySmall = base.displaySmall.copy(fontFeatureSettings = tnum),
        headlineLarge = base.headlineLarge.copy(fontFeatureSettings = tnum),
        headlineMedium = base.headlineMedium.copy(fontFeatureSettings = tnum),
        headlineSmall = base.headlineSmall.copy(fontFeatureSettings = tnum),
    )
    CompositionLocalProvider(LocalSem provides if (dark) DarkSem else LightSem) {
        MaterialTheme(colorScheme = scheme, typography = typo, content = content)
    }
}

val NumStyle = TextStyle(fontFeatureSettings = "tnum")

@Composable
fun Lvl.color(): Color {
    val s = LocalSem.current
    return when (this) {
        Lvl.GOOD -> s.good
        Lvl.WARN -> s.warn
        Lvl.BAD -> s.bad
        Lvl.INFO -> s.info
        Lvl.NONE -> MaterialTheme.colorScheme.onSurfaceVariant
    }
}

@Composable
fun Lvl.bg(): Color {
    val s = LocalSem.current
    return when (this) {
        Lvl.GOOD -> s.goodBg
        Lvl.WARN -> s.warnBg
        Lvl.BAD -> s.badBg
        Lvl.INFO -> s.infoBg
        Lvl.NONE -> MaterialTheme.colorScheme.surfaceContainerHighest
    }
}

fun lvlOf(s: String?): Lvl = when (s) {
    "good" -> Lvl.GOOD
    "warn" -> Lvl.WARN
    "bad" -> Lvl.BAD
    "info" -> Lvl.INFO
    else -> Lvl.NONE
}

/** Цвета компонентов расхода — одинаковые во всём приложении. */
object Pal {
    val cpu = Color(0xFFE8553E)
    val cpuIdle = Color(0xFFC58B7C)
    val screen = Color(0xFFF2B33D)
    val gnss = Color(0xFFD64593)
    val radio = Color(0xFF8E6CDB)
    val wifi = Color(0xFF3D8FE0)
    val audio = Color(0xFF2BB3A3)
    val video = Color(0xFFE07A3D)
    val camera = Color(0xFF6D8A3D)
    val sensors = Color(0xFF55B04A)
    val wakelock = Color(0xFF8A8A8A)
    val bt = Color(0xFF4F5FC4)
    val other = Color(0xFF9E9E9E)
    val off = Color(0xFF5A6B8C)
    val plug = Color(0xFF4CAF50)
    val fg = Color(0xFFF2B33D)
    val bg = Color(0xFFE8553E)
    val fgs = Color(0xFF8E6CDB)
    val cached = Color(0xFF9E9E9E)

    fun comp(k: String): Color = when (k) {
        "cpu" -> cpu
        "screen" -> screen
        "gnss" -> gnss
        "mobile_radio", "phone" -> radio
        "wifi" -> wifi
        "audio" -> audio
        "video" -> video
        "camera", "flashlight" -> camera
        "sensors" -> sensors
        "wakelock" -> wakelock
        "bluetooth" -> bt
        else -> other
    }

    fun state(k: String): Color = when (k) {
        "fg" -> fg
        "bg" -> bg
        "fgs" -> fgs
        else -> cached
    }
}
