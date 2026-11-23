package dev.niazlv.zhor

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
        setContent { ZhorTheme { ZhorApp(vm) } }
    }

    override fun onResume() {
        super.onResume()
        // при возврате в приложение — свежий отчёт без нового снимка batterystats (дёшево)
        if (resumedOnce) vm.refresh()
        resumedOnce = true
    }
}
