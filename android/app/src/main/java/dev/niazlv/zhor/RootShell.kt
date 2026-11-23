package dev.niazlv.zhor

import java.io.BufferedReader
import java.io.BufferedWriter
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.util.concurrent.atomic.AtomicInteger

/**
 * Одна долгоживущая root-оболочка (`su`): команды пишутся в stdin, конец ответа
 * отмечается строкой-маркером с кодом выхода. Так root запрашивается один раз,
 * а не на каждый запрос к ядру.
 */
object RootShell {
    data class Result(val code: Int, val out: String)

    private var proc: Process? = null
    private var w: BufferedWriter? = null
    private var r: BufferedReader? = null
    private val seq = AtomicInteger()

    @Synchronized
    private fun ensure(): Boolean {
        val p = proc
        if (p != null && p.isAlive) return true
        return try {
            val np = ProcessBuilder("su").start()
            // stderr сливаем, чтобы оболочка не встала на полном буфере
            Thread {
                try {
                    np.errorStream.bufferedReader().forEachLine { }
                } catch (_: Exception) {
                }
            }.apply { isDaemon = true }.start()
            proc = np
            w = BufferedWriter(OutputStreamWriter(np.outputStream))
            r = BufferedReader(InputStreamReader(np.inputStream), 1 shl 16)
            true
        } catch (e: Exception) {
            proc = null
            false
        }
    }

    @Synchronized
    fun exec(cmd: String): Result {
        if (!ensure()) return Result(-1, "")
        val marker = "__ZHOR_END_${seq.incrementAndGet()}__"
        return try {
            val wr = w!!
            wr.write(cmd)
            wr.write("\necho \"$marker \$?\"\n")
            wr.flush()
            val sb = StringBuilder()
            val rd = r!!
            while (true) {
                val line = rd.readLine() ?: break
                if (line.startsWith(marker)) {
                    val code = line.substringAfter(' ').trim().toIntOrNull() ?: -1
                    return Result(code, sb.toString())
                }
                sb.append(line).append('\n')
            }
            // оболочка умерла (отказ в root?)
            proc?.destroy()
            proc = null
            Result(-1, sb.toString())
        } catch (e: Exception) {
            proc?.destroy()
            proc = null
            Result(-1, e.message ?: "")
        }
    }

    fun isRoot(): Boolean = exec("id -u").out.trim() == "0"

    /** Строка для одинарных кавычек sh. */
    fun q(s: String): String = "'" + s.replace("'", "'\\''") + "'"
}
