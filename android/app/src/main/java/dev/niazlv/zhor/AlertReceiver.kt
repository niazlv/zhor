package dev.niazlv.zhor

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import org.json.JSONObject

/**
 * Тревоги сторожа. Ядро (root) шлёт явную рассылку `dev.niazlv.zhor.ALERT` — приёмник не
 * экспортирован, достучаться до него может только root и система. Уведомление показывает
 * приложение от своего имени; «Не напоминать» возвращается сюда же и приглушает ключ в ядре.
 */
class AlertReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        when (intent.action) {
            ACTION_ALERT -> Notify.alert(ctx, intent)
            ACTION_MUTE -> {
                val key = intent.getStringExtra("key") ?: return
                val id = intent.getIntExtra("nid", 0)
                NotificationManagerCompat.from(ctx).cancel(id)
                val pending = goAsync()
                Thread {
                    try {
                        RootShell.exec("${Core.BIN} query ${RootShell.q(JSONObject().put("cmd", "mute").put("key", key).put("on", true).toString())}")
                    } finally {
                        pending.finish()
                    }
                }.start()
            }
        }
    }

    companion object {
        const val ACTION_ALERT = "dev.niazlv.zhor.ALERT"
        const val ACTION_MUTE = "dev.niazlv.zhor.MUTE"
    }
}

object Notify {
    const val CHANNEL = "watch"
    const val EXTRA_OPEN = "zhor_open"

    fun ensureChannel(ctx: Context) {
        val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
        if (nm.getNotificationChannel(CHANNEL) != null) return
        val ch = NotificationChannel(CHANNEL, "Сторож", NotificationManager.IMPORTANCE_DEFAULT).apply {
            description = "Телефон не спит, батарея тает в покое, приложение работает в фоне — в момент, когда это происходит."
        }
        nm.createNotificationChannel(ch)
    }

    fun permitted(ctx: Context): Boolean = Build.VERSION.SDK_INT < 33 ||
        ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    /** Разрешение есть, уведомления приложения включены и канал «Сторож» не выключен. */
    fun allowed(ctx: Context): Boolean {
        if (!permitted(ctx)) return false
        if (!NotificationManagerCompat.from(ctx).areNotificationsEnabled()) return false
        val ch = ctx.getSystemService(NotificationManager::class.java)?.getNotificationChannel(CHANNEL)
        return ch == null || ch.importance != NotificationManager.IMPORTANCE_NONE
    }

    /** Системные настройки уведомлений: приложения целиком или канала «Сторож», если выключен только он. */
    fun openSettings(ctx: Context) {
        val appOn = NotificationManagerCompat.from(ctx).areNotificationsEnabled()
        val i = if (appOn) {
            ensureChannel(ctx)
            Intent(Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_CHANNEL_ID, CHANNEL)
        } else {
            Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
        }
        i.putExtra(Settings.EXTRA_APP_PACKAGE, ctx.packageName).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            ctx.startActivity(i)
        } catch (_: Exception) {
            ctx.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, android.net.Uri.fromParts("package", ctx.packageName, null)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }

    /** Пакет → подпись, но только сам пакет: имена процессов («com.app:sync») остаются как есть. */
    fun replacePkg(text: String, pkg: String?, label: String): String {
        if (pkg == null || label == pkg || !text.contains(pkg)) return text
        return Regex(Regex.escape(pkg) + "(?![:.\\w])").replace(text, Regex.escapeReplacement(label))
    }

    /** Подпись приложения вместо имени пакета в тексте ядра. */
    private fun nice(ctx: Context, text: String, pkg: String?): String {
        if (pkg == null || !text.contains(pkg)) return text
        val label = try {
            val pm = ctx.packageManager
            pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString()
        } catch (_: Exception) {
            return text
        }
        return replacePkg(text, pkg, label)
    }

    fun alert(ctx: Context, i: Intent) {
        if (!allowed(ctx)) return
        ensureChannel(ctx)
        val key = i.getStringExtra("key") ?: "alert"
        val pkg = i.getStringExtra("pkg")
        val title = nice(ctx, i.getStringExtra("title") ?: "Жор", pkg)
        val text = nice(ctx, i.getStringExtra("text") ?: "", pkg)
        val nid = key.hashCode()
        val open = Intent(ctx, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
            putExtra(EXTRA_OPEN, "alert")
            putExtra("from", i.getLongExtra("from", 0))
            putExtra("to", i.getLongExtra("to", 0))
            if (i.hasExtra("uid")) putExtra("uid", i.getIntExtra("uid", -1))
        }
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        val b = NotificationCompat.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_zhor)
            .setColor(0xFFE8553E.toInt())
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setWhen(i.getLongExtra("t", System.currentTimeMillis()))
            .setShowWhen(true)
            .setAutoCancel(true)
            .setCategory(NotificationCompat.CATEGORY_STATUS)
            .setContentIntent(PendingIntent.getActivity(ctx, nid, open, flags))
        if (i.getStringExtra("rule") != "test") {
            val mute = Intent(ctx, AlertReceiver::class.java).apply {
                action = AlertReceiver.ACTION_MUTE
                putExtra("key", key)
                putExtra("nid", nid)
            }
            b.addAction(0, "Не напоминать", PendingIntent.getBroadcast(ctx, nid, mute, flags))
        }
        try {
            NotificationManagerCompat.from(ctx).notify(nid, b.build())
        } catch (_: SecurityException) {
        }
    }
}
