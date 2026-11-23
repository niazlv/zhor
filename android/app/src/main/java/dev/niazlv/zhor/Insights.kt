package dev.niazlv.zhor

/**
 * База для долей приложений: факт по счётчику, но не меньше суммы приложений. Факт считается только
 * без зарядки, а расход приложений — за весь период; на зарядке иначе выходят доли в тысячи процентов.
 */
fun shareBase(r: Report): Double = maxOf(r.totals.measuredMah, r.apps.sumOf { it.mah })

/** Оценки чисел по нормам из docs/api.md и «Как читать». */
object Norms {
    /** Слишком мало выключенного экрана, чтобы судить о сне. */
    const val MIN_OFF_MS = 10 * 60_000L

    fun deepSleep(pct: Double, offMs: Long): Lvl = when {
        offMs < MIN_OFF_MS -> Lvl.NONE
        pct >= 90 -> Lvl.GOOD
        pct >= 70 -> Lvl.WARN
        else -> Lvl.BAD
    }

    fun deepSleepWord(l: Lvl) = when (l) {
        Lvl.GOOD -> "спит хорошо"
        Lvl.WARN -> "спит так себе"
        Lvl.BAD -> "почти не спит"
        else -> "мало данных"
    }

    fun drainOff(pctH: Double, offMs: Long): Lvl = when {
        offMs < MIN_OFF_MS -> Lvl.NONE
        pctH < 1 -> Lvl.GOOD
        pctH <= 3 -> Lvl.WARN
        else -> Lvl.BAD
    }

    fun drainOffWord(l: Lvl) = when (l) {
        Lvl.GOOD -> "отлично"
        Lvl.WARN -> "заметно"
        Lvl.BAD -> "много"
        else -> "мало данных"
    }

    /** Ток при выключенном экране (мА). */
    fun offMa(ma: Double, offMs: Long): Lvl = when {
        offMs < MIN_OFF_MS -> Lvl.NONE
        ma < 25 -> Lvl.GOOD
        ma < 80 -> Lvl.WARN
        else -> Lvl.BAD
    }

    fun failRatio(suspends: Long, fails: Long): Lvl {
        if (suspends + fails < 20) return Lvl.NONE
        val r = fails.toDouble() / (suspends + fails).coerceAtLeast(1)
        return when {
            r < 0.1 -> Lvl.GOOD
            r < 0.3 -> Lvl.WARN
            else -> Lvl.BAD
        }
    }

    fun other(otherMah: Double, measured: Double): Lvl {
        if (measured < 5) return Lvl.NONE
        val r = otherMah / measured
        return when {
            r < 0.3 -> Lvl.GOOD
            r < 0.45 -> Lvl.WARN
            else -> Lvl.BAD
        }
    }

    fun health(full: Double, design: Double): Lvl {
        if (full <= 0 || design <= 0) return Lvl.NONE
        val r = full / design
        return when {
            r >= 0.9 -> Lvl.GOOD
            r >= 0.8 -> Lvl.WARN
            else -> Lvl.BAD
        }
    }

    fun temp(c: Double): Lvl = when {
        c <= 0 -> Lvl.NONE
        c < 38 -> Lvl.GOOD
        c < 43 -> Lvl.WARN
        else -> Lvl.BAD
    }

    fun scanCost(us: Long): Lvl = when {
        us <= 0 -> Lvl.NONE
        us < 150_000 -> Lvl.GOOD
        us < 400_000 -> Lvl.WARN
        else -> Lvl.BAD
    }

    /** Насколько модель расходится с фактом — словами. */
    fun modelVsFact(model: Double, fact: Double): String? {
        if (fact < 3 || model <= 0) return null
        val r = model / fact
        return when {
            r > 1.25 -> "Модель выше замера на ${Fmt.pctRaw((r - 1) * 100)} %: коэффициенты производителя завышены. Доли между приложениями от этого не меняются."
            r < 0.7 -> "Модель объясняет только ${Fmt.pctRaw(r * 100)} % замера: остальное — радио, GPS, Wi-Fi, GPU и периферия (см. «Модель Android»)."
            else -> "Модель и замер сходятся (${Fmt.pctRaw(r * 100)} %) — разбивке по приложениям можно доверять и в абсолютных числах."
        }
    }
}

/** Выводы ядра 0.3, а если их нет — свои, по тем же нормам. */
object Insights {
    fun of(r: Report): List<Insight> {
        r.insights?.let { if (it.isNotEmpty()) return order(it) }
        return order(local(r))
    }

    fun forApp(r: Report, uid: Int): List<Insight> = r.insights?.filter { it.uid == uid } ?: emptyList()

    private fun order(l: List<Insight>) = l.sortedBy {
        when (it.level) {
            "bad" -> 0
            "warn" -> 1
            "info" -> 2
            else -> 3
        }
    }

    private fun local(r: Report): List<Insight> {
        val out = mutableListOf<Insight>()
        val t = r.totals
        if (t.spanMs == 0L) {
            out += Insight("info", "Данных пока нет", "Ядро ещё не записало ничего за этот период. Выберите период побольше или подождите пару минут.", null, null)
            return out
        }
        val sleep = Norms.deepSleep(t.deepSleepPct, t.screenOffMs)
        if (sleep == Lvl.BAD || sleep == Lvl.WARN) {
            val ws = r.kernel.wakeupSources.firstOrNull()
            val text = buildString {
                append("Без зарядки с выключенным экраном телефон спал только ${Fmt.pctRaw(t.deepSleepPct)} % времени (норма — больше 90 %).")
                if (ws != null) append(" Чаще всех будил «${ws.name}»: ${Fmt.times(ws.wakeups)}, держал ${Fmt.dur(ws.heldMs)}.")
            }
            out += Insight(if (sleep == Lvl.BAD) "bad" else "warn", "Телефон плохо спит", text, null, "sleep")
        } else if (sleep == Lvl.GOOD) {
            out += Insight("good", "Телефон хорошо спит", "Без зарядки с выключенным экраном ${Fmt.pctRaw(t.deepSleepPct)} % времени в глубоком сне.", null, "sleep")
        }
        val drain = Norms.drainOff(t.pctPerHourOff, t.screenOffMs)
        if (drain == Lvl.BAD) {
            out += Insight(
                "bad", "Быстрый разряд в покое",
                "При выключенном экране уходило ${Fmt.d1(t.pctPerHourOff)} %/ч (норма — меньше 1 %/ч), в среднем ${Fmt.ma(t.measuredOffMa)} мА.",
                null, "sleep",
            )
        }
        r.apps.filter { it.cpuMahScreenOff > 0.3 }.maxByOrNull { it.cpuMahScreenOff }?.let { a ->
            if (t.screenOffMs >= Norms.MIN_OFF_MS) out += Insight(
                "warn", "Больше всех в фоне — ${a.label}",
                "При выключенном экране потратил ${Fmt.mah(a.cpuMahScreenOff)} мА·ч на процессор" +
                    (if (a.cpuMah > 0) " (${Fmt.pctRaw(100 * a.cpuMahScreenOff / a.cpuMah)} % своего CPU)." else "."),
                a.uid, "apps",
            )
        }
        r.apps.firstOrNull()?.let { a ->
            val base = shareBase(r)
            val share = if (base > 0) " — ${Fmt.pctRaw(100 * a.mah / base)} % израсходованного" else ""
            out += Insight("info", "Больше всех — ${a.label}", "${Fmt.mah(a.mah)} мА·ч$share. ${a.why.firstOrNull() ?: ""}", a.uid, "apps")
        }
        if (Norms.other(t.otherMah, t.measuredMah) == Lvl.BAD) {
            val g = r.sys?.global?.filter { it.key != "cpu" && it.key != "screen" }?.maxByOrNull { it.value }
            out += Insight(
                "warn", "Много «прочего»",
                "${Fmt.pctRaw(100 * t.otherMah / t.measuredMah)} % расхода не объясняется процессором и экраном" +
                    (g?.let { " — по модели Android больше всего ${Fmt.compName(it.key)} (${Fmt.mah(it.value)} мА·ч)." } ?: " — это радио, GPS, Wi-Fi и периферия."),
                null, "battery",
            )
        }
        if (Norms.failRatio(t.suspends, t.suspendFails) == Lvl.BAD) {
            val ab = r.kernel.abortReasons.firstOrNull()
            out += Insight(
                "warn", "Засыпание часто срывается",
                "Сорвано ${t.suspendFails} из ${t.suspends + t.suspendFails} попыток уснуть." + (ab?.let { " Чаще всего: ${it.name.removePrefix("Abort: ")}." } ?: ""),
                null, "kernel",
            )
        }
        if (t.gpuBusyPct > 30) out += Insight("info", "GPU много работал", "Видеоускоритель занят ${Fmt.pctRaw(t.gpuBusyPct)} % бодрствования — игры, видео, анимации.", null, "cpu")
        return out
    }
}
