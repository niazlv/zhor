package dev.niazlv.zhor

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/**
 * Обновления. Это ЕДИНСТВЕННОЕ место, где приложение ходит в сеть: раз в сутки (по напоминанию ядра или
 * при открытии) спрашивает у GitHub последний выпуск. Ничего о телефоне не отправляет — обычный GET
 * к api.github.com. Скачивает APK и ставит его только по кнопке «Обновить»: перед установкой сверяет
 * контрольную сумму из SHA256SUMS выпуска, имя пакета и сертификат подписи с установленным Жором,
 * а ставит ядро (`pm install` от root).
 */
object Updater {
    private const val REPO = "niazlv/zhor"
    private const val API = "https://api.github.com/repos/$REPO/releases/latest"
    const val CHANNEL = "updates"
    private const val NOTIF_ID = 0x7a0
    private const val EVERY_MS = 24 * 3_600_000L

    data class Rel(
        val version: String, val tag: String, val notes: String, val url: String, val published: String,
        val apkName: String, val apkUrl: String, val apkSize: Long, val sumsUrl: String?,
    ) {
        fun json(): JSONObject = JSONObject().put("version", version).put("tag", tag).put("notes", notes).put("url", url)
            .put("published", published).put("apk_name", apkName).put("apk_url", apkUrl).put("apk_size", apkSize).put("sums_url", sumsUrl)

        companion object {
            fun fromJson(o: JSONObject) = Rel(
                o.optString("version"), o.optString("tag"), o.optString("notes"), o.optString("url"), o.optString("published"),
                o.optString("apk_name"), o.optString("apk_url"), o.optLong("apk_size"), o.optString("sums_url").takeIf { it.isNotEmpty() },
            )
        }
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("zhor", Context.MODE_PRIVATE)

    fun installedVersion(ctx: Context): String =
        runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull() ?: "0"

    /** Сравнение версий «0.10.2» > «0.9.9»; хвост вроде «-beta» не учитывается. */
    fun cmp(a: String, b: String): Int {
        fun parts(v: String) = v.trimStart('v').substringBefore('-').split('.').map { it.toIntOrNull() ?: 0 }
        val x = parts(a)
        val y = parts(b)
        for (i in 0 until maxOf(x.size, y.size)) {
            val d = (x.getOrElse(i) { 0 }).compareTo(y.getOrElse(i) { 0 })
            if (d != 0) return d
        }
        return 0
    }

    private fun open(url: String): HttpURLConnection = (URL(url).openConnection() as HttpURLConnection).apply {
        connectTimeout = 15_000
        readTimeout = 30_000
        instanceFollowRedirects = true
        setRequestProperty("User-Agent", "Zhor-updater")
        setRequestProperty("Accept", "application/vnd.github+json")
    }

    /** Последний выпуск на GitHub. */
    fun fetchLatest(): Rel {
        val c = open(API)
        try {
            if (c.responseCode != 200) throw IllegalStateException("GitHub ответил ${c.responseCode}")
            val o = JSONObject(c.inputStream.bufferedReader().readText())
            val tag = o.getString("tag_name")
            val version = tag.trimStart('v')
            val assets = o.optJSONArray("assets")
            var apk: JSONObject? = null
            var sums: String? = null
            if (assets != null) for (i in 0 until assets.length()) {
                val a = assets.getJSONObject(i)
                when (a.optString("name")) {
                    "zhor-$version.apk" -> apk = a
                    "zhor.apk" -> if (apk == null) apk = a
                    "SHA256SUMS" -> sums = a.optString("browser_download_url")
                }
            }
            val ap = apk ?: throw IllegalStateException("в выпуске $tag нет APK")
            return Rel(
                version, tag, o.optString("body"), o.optString("html_url"), o.optString("published_at"),
                ap.optString("name"), ap.optString("browser_download_url"), ap.optLong("size"), sums,
            )
        } finally {
            c.disconnect()
        }
    }

    /** Найденный выпуск, если он новее установленного. Запоминает время проверки и выпуск. */
    fun check(ctx: Context): Rel? {
        val r = fetchLatest()
        val p = prefs(ctx).edit().putLong("upd_checked", System.currentTimeMillis())
        val newer = cmp(r.version, installedVersion(ctx)) > 0
        if (newer) p.putString("upd_rel", r.json().toString()) else p.remove("upd_rel")
        p.apply()
        return if (newer) r else null
    }

    fun lastChecked(ctx: Context): Long = prefs(ctx).getLong("upd_checked", 0)

    /** Выпуск из прошлой проверки, если он всё ещё новее установленного. */
    fun known(ctx: Context): Rel? = prefs(ctx).getString("upd_rel", null)
        ?.let { runCatching { Rel.fromJson(JSONObject(it)) }.getOrNull() }
        ?.takeIf { cmp(it.version, installedVersion(ctx)) > 0 }

    fun due(ctx: Context): Boolean = System.currentTimeMillis() - lastChecked(ctx) > EVERY_MS

    /** Скачать APK в cache/update. [progress] — (скачано, всего). */
    fun download(ctx: Context, r: Rel, progress: (Long, Long) -> Unit): File {
        val dir = File(ctx.cacheDir, "update").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() }
        val out = File(dir, r.apkName)
        val c = open(r.apkUrl)
        try {
            c.setRequestProperty("Accept", "application/octet-stream")
            if (c.responseCode != 200) throw IllegalStateException("скачивание: GitHub ответил ${c.responseCode}")
            val total = c.contentLengthLong.takeIf { it > 0 } ?: r.apkSize
            var done = 0L
            c.inputStream.use { input ->
                out.outputStream().use { o ->
                    val buf = ByteArray(64 * 1024)
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        o.write(buf, 0, n)
                        done += n
                        progress(done, total)
                    }
                }
            }
        } finally {
            c.disconnect()
        }
        return out
    }

    private fun sha256(f: File): String {
        val md = MessageDigest.getInstance("SHA-256")
        f.inputStream().use { i ->
            val buf = ByteArray(64 * 1024)
            while (true) {
                val n = i.read(buf)
                if (n < 0) break
                md.update(buf, 0, n)
            }
        }
        return md.digest().joinToString("") { "%02x".format(it) }
    }

    private fun certs(info: PackageInfo?): Set<String> {
        val s = info?.signingInfo ?: return emptySet()
        val list = if (s.hasMultipleSigners()) s.apkContentsSigners else s.signingCertificateHistory
        return list.orEmpty().map { MessageDigest.getInstance("SHA-256").digest(it.toByteArray()).joinToString("") { b -> "%02x".format(b) } }.toSet()
    }

    /**
     * Проверить скачанное: контрольная сумма из SHA256SUMS того же выпуска, тот же пакет, версия выпуска и
     * сертификат подписи — как у установленного Жора (чужую сборку Android и сам не поставил бы поверх).
     */
    fun verify(ctx: Context, f: File, r: Rel) {
        val sumsUrl = r.sumsUrl ?: throw IllegalStateException("в выпуске нет SHA256SUMS — ставить не буду")
        val c = open(sumsUrl)
        val sums = try {
            if (c.responseCode != 200) throw IllegalStateException("SHA256SUMS: GitHub ответил ${c.responseCode}")
            c.inputStream.bufferedReader().readText()
        } finally {
            c.disconnect()
        }
        val want = sums.lines().map { it.trim().split(Regex("\\s+")) }.firstOrNull { it.size >= 2 && it[1].trimStart('*') == r.apkName }?.get(0)?.lowercase()
            ?: throw IllegalStateException("в SHA256SUMS нет ${r.apkName}")
        val got = sha256(f)
        if (got != want) throw IllegalStateException("контрольная сумма не совпала — файл повреждён или подменён")
        val pm = ctx.packageManager
        val archive = pm.getPackageArchiveInfo(f.path, PackageManager.GET_SIGNING_CERTIFICATES)
            ?: throw IllegalStateException("скачанный файл — не APK")
        if (archive.packageName != ctx.packageName) throw IllegalStateException("в APK другое приложение: ${archive.packageName}")
        if (archive.versionName != r.version) throw IllegalStateException("в APK версия ${archive.versionName}, а не ${r.version}")
        val mine = certs(pm.getPackageInfo(ctx.packageName, PackageManager.GET_SIGNING_CERTIFICATES))
        val theirs = certs(archive)
        if (mine.isEmpty() || theirs.isEmpty() || theirs.intersect(mine).isEmpty()) {
            throw IllegalStateException("APK подписан другим ключом — это не официальная сборка, ставить не буду")
        }
    }

    // ---- уведомление ----

    fun ensureChannel(ctx: Context) {
        val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
        if (nm.getNotificationChannel(CHANNEL) != null) return
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, "Обновления", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Вышла новая версия Жора. Ставится только по вашей кнопке."
            },
        )
    }

    /** Фоновая проверка по напоминанию ядра: новое — уведомить один раз на версию. */
    fun backgroundCheck(ctx: Context) {
        val r = runCatching { check(ctx) }.getOrNull() ?: return
        val p = prefs(ctx)
        if (p.getString("upd_notified", null) == r.version) return
        if (!Notify.permitted(ctx) || !NotificationManagerCompat.from(ctx).areNotificationsEnabled()) return
        ensureChannel(ctx)
        val open = Intent(ctx, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
            putExtra(Notify.EXTRA_OPEN, "update")
        }
        val first = r.notes.lines().map { it.trim() }.firstOrNull { it.isNotEmpty() && !it.startsWith("#") } ?: "Что нового — в приложении."
        val n = NotificationCompat.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_zhor)
            .setColor(0xFFE8553E.toInt())
            .setContentTitle("Доступен Жор ${r.version}")
            .setContentText(first)
            .setStyle(NotificationCompat.BigTextStyle().bigText("$first\n\nУстановлен ${installedVersion(ctx)}. Обновление — только по вашей кнопке в приложении."))
            .setAutoCancel(true)
            .setContentIntent(PendingIntent.getActivity(ctx, NOTIF_ID, open, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            .build()
        try {
            NotificationManagerCompat.from(ctx).notify(NOTIF_ID, n)
            p.edit().putString("upd_notified", r.version).apply()
        } catch (_: SecurityException) {
        }
    }
}
