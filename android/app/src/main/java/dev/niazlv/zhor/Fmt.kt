package dev.niazlv.zhor

import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale

/** Форматирование чисел и времени. Строки формата — только константы (динамический текст в format() роняет приложение). */
object Fmt {
    private val ru = Locale.forLanguageTag("ru")

    fun dur(ms: Long): String {
        if (ms <= 0) return "0 с"
        if (ms < 1000) return "$ms мс"
        val s = ms / 1000
        val d = s / 86400
        val h = (s % 86400) / 3600
        val m = (s % 3600) / 60
        val sec = s % 60
        return when {
            d > 0 -> "$d д $h ч"
            h > 0 -> "$h ч ${"%02d".format(m)} мин"
            m > 0 -> if (m >= 10) "$m мин" else "$m мин ${"%02d".format(sec)} с"
            else -> "$sec с"
        }
    }

    /** Коротко, для подписей на графиках. */
    fun durShort(ms: Long): String {
        val s = ms / 1000
        return when {
            s >= 86400 -> "${s / 86400} д"
            s >= 3600 -> if (s % 3600 >= 360) "%.1f ч".format(ru, s / 3600.0) else "${s / 3600} ч"
            s >= 60 -> "${s / 60} мин"
            else -> "$s с"
        }
    }

    fun mah(v: Double): String = when {
        v.isNaN() -> "—"
        v >= 100 -> "%.0f".format(ru, v)
        v >= 1 -> "%.1f".format(ru, v)
        v >= 0.01 || v == 0.0 -> "%.2f".format(ru, v)
        else -> "<0,01"
    }

    fun ma(v: Double): String = "%.0f".format(ru, v)
    fun d0(v: Double): String = "%.0f".format(ru, v)
    fun d1(v: Double): String = "%.1f".format(ru, v)
    fun d2(v: Double): String = "%.2f".format(ru, v)
    fun pct(v: Double): String = if (v in 0.0..0.5 && v > 0) "<1 %" else "${"%.0f".format(ru, v)} %"
    fun pctRaw(v: Double): String = "%.0f".format(ru, v)

    fun bytes(b: Long): String = when {
        b >= 1L shl 30 -> "%.1f ГБ".format(ru, b / 1073741824.0)
        b >= 1L shl 20 -> "%.1f МБ".format(ru, b / 1048576.0)
        b >= 1024 -> "${b / 1024} КБ"
        else -> "$b Б"
    }

    fun count(n: Long): String = when {
        n >= 1_000_000 -> "%.1f млн".format(ru, n / 1e6)
        n >= 10_000 -> "${n / 1000} тыс."
        else -> n.toString()
    }

    private fun f(p: String, t: Long) = SimpleDateFormat(p, ru).format(Date(t))
    fun time(t: Long): String = f("dd.MM HH:mm", t)
    fun clock(t: Long): String = f("HH:mm", t)
    fun clockSec(t: Long): String = f("HH:mm:ss", t)
    fun date(t: Long): String = f("d MMMM", t)
    fun stamp(t: Long): String = f("yyyy-MM-dd_HH-mm", t)

    fun sameDay(a: Long, b: Long): Boolean {
        val ca = Calendar.getInstance().apply { timeInMillis = a }
        val cb = Calendar.getInstance().apply { timeInMillis = b }
        return ca.get(Calendar.YEAR) == cb.get(Calendar.YEAR) && ca.get(Calendar.DAY_OF_YEAR) == cb.get(Calendar.DAY_OF_YEAR)
    }

    /** «сегодня 03:16», «вчера 22:10», «21.11 14:00». */
    fun human(t: Long, now: Long = System.currentTimeMillis()): String {
        if (t <= 0) return "—"
        val y = now - 86_400_000
        return when {
            sameDay(t, now) -> "сегодня ${clock(t)}"
            sameDay(t, y) -> "вчера ${clock(t)}"
            else -> time(t)
        }
    }

    /** Интервал: «03:16–04:02» в пределах дня, иначе с датами. */
    fun span(a: Long, b: Long): String = if (sameDay(a, b)) "${clock(a)}–${clock(b)}" else "${time(a)} – ${time(b)}"

    fun ago(t: Long, now: Long = System.currentTimeMillis()): String {
        val d = (now - t).coerceAtLeast(0)
        return when {
            d < 60_000 -> "только что"
            d < 3_600_000 -> "${d / 60_000} мин назад"
            d < 86_400_000 -> "${d / 3_600_000} ч назад"
            else -> "${d / 86_400_000} д назад"
        }
    }

    /** Склонение: plural(5, "раз", "раза", "раз"). */
    fun plural(n: Long, one: String, few: String, many: String): String {
        val a = n % 100
        val b = n % 10
        return when {
            a in 11..14 -> many
            b == 1L -> one
            b in 2..4 -> few
            else -> many
        }
    }

    fun times(n: Long) = "$n ${plural(n, "раз", "раза", "раз")}"

    val comp = mapOf(
        "cpu" to "процессор", "screen" to "экран", "gnss" to "GPS", "mobile_radio" to "мобильная связь",
        "wifi" to "Wi-Fi", "audio" to "звук", "video" to "видео", "camera" to "камера", "sensors" to "датчики",
        "wakelock" to "wakelock", "bluetooth" to "Bluetooth", "flashlight" to "фонарик", "phone" to "звонки",
        "memory" to "память", "ambient_display" to "AOD", "idle" to "простой",
    )

    fun compName(k: String) = comp[k] ?: k

    fun kindName(k: String) = when (k) {
        "app" -> "приложение"
        "system" -> "системная служба"
        "kernel" -> "ядро Linux и root-демоны"
        "isolated" -> "изолированный процесс"
        else -> k
    }

    fun stateName(k: String) = when (k) {
        "fg" -> "на экране"
        "bg" -> "в фоне"
        "fgs" -> "фоновая служба"
        "cached" -> "в кэше"
        else -> k
    }

    fun threadName(n: String) = when (n) {
        "<main>" -> "главный поток"
        "<прочие потоки>" -> "прочие потоки"
        "<завершённые потоки>" -> "завершённые потоки"
        else -> n
    }
}
