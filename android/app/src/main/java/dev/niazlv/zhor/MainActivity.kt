package dev.niazlv.zhor

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels

class MainActivity : ComponentActivity() {
    private val vm: Vm by viewModels()
    private var resumedOnce = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        handle(intent)
        setContent { ZhorTheme { ZhorApp(vm) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handle(intent)
    }

    /** Нажали на уведомление сторожа — открыть отчёт за тот период (и приложение, если оно названо). */
    private fun handle(i: Intent?) {
        if (i?.getStringExtra(Notify.EXTRA_OPEN) != "alert") return
        val from = i.getLongExtra("from", 0)
        val to = i.getLongExtra("to", 0)
        if (from <= 0 || to <= from) return
        vm.openAlert(from, to, i.getIntExtra("uid", -1).takeIf { it > 0 })
        i.removeExtra(Notify.EXTRA_OPEN)
    }

    override fun onResume() {
        super.onResume()
        // при возврате в приложение — свежий отчёт без нового снимка batterystats (дёшево)
        if (resumedOnce) vm.refresh()
        resumedOnce = true
    }
}
