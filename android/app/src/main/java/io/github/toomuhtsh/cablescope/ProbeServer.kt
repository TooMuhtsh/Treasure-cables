package io.github.toomuhtsh.cablescope

import org.json.JSONObject
import java.io.BufferedReader
import java.io.Closeable
import java.io.InputStreamReader
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.security.SecureRandom
import java.util.concurrent.atomic.AtomicReference

/**
 * Serveur de la sonde : implémente le protocole décrit dans
 * docs/protocole-sonde.md. Sans dépendance à Android (testé sur la JVM) :
 * l'application fournit ce qui touche au téléphone par [ProbeHost].
 */
class ProbeServer(
    private val host: ProbeHost,
    private val address: InetAddress,
    private val controlPort: Int = CONTROL_PORT,
    private val dataPort: Int = DATA_PORT,
) : Closeable {
    private val control = ServerSocket(controlPort, 1, address)
    private val data = ServerSocket(dataPort, 4, address)
    private val session = AtomicReference<String?>(null)
    @Volatile private var busy = false
    @Volatile private var closed = false

    fun start() {
        thread("sonde-contrôle") { acceptLoop(control, ::serveControl) }
        thread("sonde-débit") { acceptLoop(data) { socket -> thread("sonde-mesure") { serveData(socket) } } }
    }

    override fun close() {
        closed = true
        runCatching { control.close() }
        runCatching { data.close() }
    }

    private fun acceptLoop(server: ServerSocket, handle: (Socket) -> Unit) {
        while (!closed) {
            val socket = try {
                server.accept()
            } catch (_: SocketException) {
                return // serveur fermé
            }
            handle(socket)
        }
    }

    // --- Contrôle -----------------------------------------------------------

    private fun serveControl(socket: Socket) {
        if (busy) {
            socket.use { send(it.getOutputStream(), error("busy", "Un test est déjà en cours.")) }
            return
        }
        busy = true
        thread("sonde-session") {
            try {
                socket.use { runSession(it) }
            } catch (_: Exception) {
                // connexion perdue : l'application l'annonce ci-dessous
            } finally {
                session.set(null)
                busy = false
                host.onEvent(ProbeEvent.Disconnected)
            }
        }
    }

    private fun runSession(socket: Socket) {
        val input = BufferedReader(InputStreamReader(socket.getInputStream(), Charsets.UTF_8))
        val output = socket.getOutputStream()
        while (true) {
            val line = input.readLine() ?: return
            val message = runCatching { JSONObject(line) }.getOrNull() ?: continue
            when (message.optString("type")) {
                "hello" -> {
                    if (message.optInt("protocol") != PROTOCOL) {
                        send(output, error("protocol", "Version du protocole non prise en charge."))
                        return
                    }
                    val computer = message.optString("computer").ifBlank { "un ordinateur" }
                    send(output, hello())
                    host.onEvent(ProbeEvent.Connected(computer))
                    if (host.askApproval(computer)) {
                        val token = newToken()
                        session.set(token)
                        send(output, JSONObject().put("type", "accepted").put("session", token))
                    } else {
                        send(output, JSONObject().put("type", "refused"))
                        return
                    }
                }
                "battery" -> send(output, battery())
                "status" -> host.onEvent(ProbeEvent.Status(message.optString("text")))
                "verdict" -> host.onEvent(
                    ProbeEvent.Verdict(
                        severity = message.optString("severity", "info"),
                        title = message.optString("title"),
                        detail = message.optString("detail"),
                        scores = scores(message),
                        facts = facts(message),
                        advice = message.optString("advice").takeIf { it.isNotBlank() && it != "null" },
                    ),
                )
                "watch" -> when (message.optString("action")) {
                    "start" -> host.startWatch()
                    "stop" -> send(output, watchReport(host.stopWatch()))
                    else -> send(output, error("unknown_type", "Action inconnue."))
                }
                "bye" -> return
                else -> send(output, error("unknown_type", "Message inconnu."))
            }
        }
    }

    private fun scores(message: JSONObject): List<VerdictScore> {
        val array = message.optJSONArray("scores") ?: return emptyList()
        return (0 until array.length()).mapNotNull { i ->
            val s = array.optJSONObject(i) ?: return@mapNotNull null
            VerdictScore(
                criterion = s.optString("criterion"),
                rating = s.optString("rating"),
                tone = s.optString("tone", "unknown"),
                value = s.optString("value").takeIf { it.isNotBlank() && it != "null" },
            )
        }
    }

    private fun facts(message: JSONObject): List<Pair<String, String>> {
        val array = message.optJSONArray("facts") ?: return emptyList()
        return (0 until array.length()).mapNotNull { i ->
            array.optJSONObject(i)?.let { it.optString("label") to it.optString("value") }
        }
    }

    private fun hello(): JSONObject {
        val device = host.device()
        return JSONObject()
            .put("type", "hello")
            .put("protocol", PROTOCOL)
            .put("probe", "CableScope Probe")
            .put("version", host.version())
            .put(
                "device",
                JSONObject()
                    .put("manufacturer", device.manufacturer)
                    .put("model", device.model)
                    .put("android", device.android),
            )
            .put("capabilities", org.json.JSONArray(listOf("battery", "throughput", "watch")))
    }

    private fun watchReport(report: WatchReport?): JSONObject = JSONObject()
        .put("type", "watch_report")
        .put("power_cuts", report?.powerCuts ?: JSONObject.NULL)
        .put("data_cuts", report?.dataCuts ?: JSONObject.NULL)
        .put("charge_drops", report?.chargeDrops ?: JSONObject.NULL)
        .put("median_current_ua", report?.medianCurrentUa ?: JSONObject.NULL)

    private fun battery(): JSONObject {
        val b = host.battery()
        return JSONObject()
            .put("type", "battery")
            .put("plugged", b.plugged ?: JSONObject.NULL)
            .put("charging", b.charging ?: JSONObject.NULL)
            .put("level_percent", b.levelPercent ?: JSONObject.NULL)
            .put("current_ua", b.currentUa ?: JSONObject.NULL)
            .put("voltage_mv", b.voltageMv ?: JSONObject.NULL)
            .put("temperature_c", b.temperatureC ?: JSONObject.NULL)
    }

    // --- Débit --------------------------------------------------------------

    private fun serveData(socket: Socket) {
        socket.use { measure(it) }
    }

    private fun measure(socket: Socket) {
        socket.soTimeout = 10_000
        val input = socket.getInputStream()
        val header = readLine(input) ?: return
        val request = runCatching { JSONObject(header) }.getOrNull() ?: return
        val expected = session.get()
        if (expected == null || request.optString("session") != expected) return
        val durationMs = request.optLong("duration_ms", 4000).coerceIn(500, 15_000)
        when (request.optString("direction")) {
            "to_computer" -> {
                host.onEvent(ProbeEvent.Measuring(toComputer = true))
                val chunk = ByteArray(CHUNK)
                val output = socket.getOutputStream()
                val end = System.nanoTime() + durationMs * 1_000_000
                while (System.nanoTime() < end) output.write(chunk)
                output.flush()
                socket.shutdownOutput()
            }
            "to_probe" -> {
                host.onEvent(ProbeEvent.Measuring(toComputer = false))
                val buffer = ByteArray(CHUNK)
                var received = 0L
                var first = 0L
                while (true) {
                    val n = input.read(buffer)
                    if (n < 0) break
                    if (first == 0L) first = System.nanoTime()
                    received += n
                }
                val elapsedMs = if (first == 0L) 0 else (System.nanoTime() - first) / 1_000_000
                send(
                    socket.getOutputStream(),
                    JSONObject().put("received", received).put("elapsed_ms", elapsedMs.coerceAtLeast(1)),
                )
            }
        }
    }

    /** Lit une ligne octet par octet, sans rien consommer au-delà. */
    private fun readLine(input: java.io.InputStream): String? {
        val bytes = java.io.ByteArrayOutputStream()
        while (bytes.size() < 4096) {
            val b = input.read()
            if (b < 0) return null
            if (b == '\n'.code) return bytes.toString(Charsets.UTF_8.name())
            bytes.write(b)
        }
        return null
    }

    private fun send(output: OutputStream, message: JSONObject) {
        synchronized(output) {
            output.write((message.toString() + "\n").toByteArray(Charsets.UTF_8))
            output.flush()
        }
    }

    private fun error(code: String, text: String) =
        JSONObject().put("type", "error").put("code", code).put("message", text)

    private fun newToken(): String {
        val bytes = ByteArray(16)
        SecureRandom().nextBytes(bytes)
        return bytes.joinToString("") { "%02x".format(it) }
    }

    private fun thread(name: String, body: () -> Unit) =
        Thread(body, name).apply { isDaemon = true }.start()

    companion object {
        const val PROTOCOL = 1
        const val CONTROL_PORT = 47820
        const val DATA_PORT = 47821
        private const val CHUNK = 256 * 1024
    }
}

/** Ce que l'application fournit au serveur. */
interface ProbeHost {
    fun device(): DeviceInfo
    fun version(): String
    fun battery(): BatteryReading
    /** Demande l'accord de l'utilisateur ; bloque jusqu'à sa réponse. */
    fun askApproval(computer: String): Boolean
    /** Test des faux contacts : surveillance des coupures pendant qu'on plie le câble. */
    fun startWatch()
    /** Fin de la surveillance ; `null` si elle n'avait pas commencé. */
    fun stopWatch(): WatchReport?
    fun onEvent(event: ProbeEvent)
}

/** Une note du verdict : « Transfert · Excellent · 5 Gb/s ». */
data class VerdictScore(val criterion: String, val rating: String, val tone: String, val value: String?)

data class DeviceInfo(val manufacturer: String, val model: String, val android: String)

data class BatteryReading(
    /** `usb`, `ac`, `wireless`, `dock` ou `none`. */
    val plugged: String?,
    val charging: Boolean?,
    val levelPercent: Int?,
    /** Courant entrant dans la batterie, positif en charge. */
    val currentUa: Long?,
    val voltageMv: Int?,
    val temperatureC: Double?,
)

sealed interface ProbeEvent {
    data class Connected(val computer: String) : ProbeEvent
    data class Status(val text: String) : ProbeEvent
    data class Measuring(val toComputer: Boolean) : ProbeEvent
    data class Verdict(
        val severity: String,
        val title: String,
        val detail: String,
        val scores: List<VerdictScore> = emptyList(),
        val facts: List<Pair<String, String>> = emptyList(),
        val advice: String? = null,
    ) : ProbeEvent
    data object Disconnected : ProbeEvent
}

/**
 * Courant de la batterie en microampères, positif en charge. Les appareils ne
 * respectent pas tous la convention : certains donnent des milliampères,
 * d'autres un signe inversé.
 */
fun normalizeCurrentUa(raw: Int, charging: Boolean): Long? {
    if (raw == Int.MIN_VALUE || raw == 0) return null
    var value = raw.toLong()
    // En charge, moins de 10 mA est invraisemblable : la valeur est en
    // milliampères. Batterie pleine, quelques milliampères sont normaux.
    if (charging && kotlin.math.abs(value) < 10_000) value *= 1000
    if (charging && value < 0) value = -value
    return value
}

/**
 * Interfaces réseau qui ont une adresse IPv4 (nom et adresse). Chaque
 * interface est lue à part : Android refuse certaines lectures aux
 * applications (état actif/inactif notamment), une erreur ne doit pas tout
 * faire échouer.
 */
fun ipv4Interfaces(): List<Pair<String, java.net.Inet4Address>> {
    val interfaces = runCatching { java.net.NetworkInterface.getNetworkInterfaces()?.toList().orEmpty() }
        .getOrDefault(emptyList())
    return interfaces.mapNotNull { nif ->
        runCatching {
            nif.inetAddresses.toList()
                .filterIsInstance<java.net.Inet4Address>()
                .firstOrNull { !it.isLoopbackAddress }
                ?.let { nif.name to it }
        }.getOrNull()
    }
}

/**
 * Adresse IPv4 de l'interface du partage de connexion USB (rndis0, ncm0,
 * usb0…), ou `null` si le partage n'est pas actif.
 */
fun usbTetherAddress(interfaces: List<Pair<String, InetAddress>> = ipv4Interfaces()): InetAddress? =
    interfaces.firstOrNull { isUsbTetherInterface(it.first) }?.second

fun isUsbTetherInterface(name: String): Boolean =
    listOf("rndis", "ncm", "usb").any { name.startsWith(it) }
