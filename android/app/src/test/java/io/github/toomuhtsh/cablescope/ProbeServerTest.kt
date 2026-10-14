package io.github.toomuhtsh.cablescope

import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.BufferedReader
import java.io.InputStreamReader
import java.net.InetAddress
import java.net.Socket
import java.util.concurrent.CopyOnWriteArrayList

class ProbeServerTest {
    private val events = CopyOnWriteArrayList<ProbeEvent>()
    private var approve = true
    @Volatile private var watching = false
    private val host = object : ProbeHost {
        override fun device() = DeviceInfo("samsung", "SM-S911B", "16")
        override fun version() = "test"
        override fun battery() = BatteryReading("usb", true, 64, 1_250_000, 4120, 31.5)
        override fun askApproval(computer: String) = approve
        override fun startWatch() {
            watching = true
        }
        override fun stopWatch() = if (watching) WatchReport(1, 0, 2, 1_250_000, 40) else null
        override fun onEvent(event: ProbeEvent) {
            events += event
        }
    }
    private val loopback = InetAddress.getLoopbackAddress()
    private val port = 48000 + (System.nanoTime() % 1000).toInt() * 2
    private lateinit var server: ProbeServer

    @Before
    fun start() {
        server = ProbeServer(host, loopback, port, port + 1)
        server.start()
    }

    @After
    fun stop() = server.close()

    private class Client(socket: Socket) {
        val input = BufferedReader(InputStreamReader(socket.getInputStream()))
        val output = socket.getOutputStream()
        fun send(json: String) = output.write((json + "\n").toByteArray()).also { output.flush() }
        fun receive() = JSONObject(input.readLine())
    }

    private fun handshake(client: Client): JSONObject {
        client.send("""{"type":"hello","protocol":1,"software":"CableScope","version":"0","computer":"PC-TEST"}""")
        val hello = client.receive()
        assertEquals("hello", hello.getString("type"))
        assertEquals("SM-S911B", hello.getJSONObject("device").getString("model"))
        assertTrue(hello.getJSONArray("capabilities").toString().contains("\"watch\""))
        return client.receive()
    }

    @Test
    fun fullSession() {
        Socket(loopback, port).use { socket ->
            val client = Client(socket)
            val accepted = handshake(client)
            assertEquals("accepted", accepted.getString("type"))
            val session = accepted.getString("session")
            assertEquals(32, session.length)

            client.send("""{"type":"battery"}""")
            val battery = client.receive()
            assertEquals(1_250_000, battery.getLong("current_ua"))
            assertEquals("usb", battery.getString("plugged"))

            // Débit vers l'ordinateur : la sonde envoie pendant la durée demandée.
            Socket(loopback, port + 1).use { data ->
                data.getOutputStream().write(
                    """{"session":"$session","direction":"to_computer","duration_ms":500}""".plus("\n").toByteArray(),
                )
                // Compté au fil de l'eau : en 500 ms, une boucle locale rapide
                // transporte plusieurs gigaoctets.
                val input = data.getInputStream()
                val buffer = ByteArray(1 shl 16)
                var received = 0L
                while (true) {
                    val n = input.read(buffer)
                    if (n < 0) break
                    received += n
                }
                assertTrue("reçu $received octets", received > 1_000_000)
            }
            // Débit vers la sonde : elle compte et répond.
            Socket(loopback, port + 1).use { data ->
                val out = data.getOutputStream()
                out.write("""{"session":"$session","direction":"to_probe","duration_ms":500}""".plus("\n").toByteArray())
                val chunk = ByteArray(1 shl 20)
                repeat(8) { out.write(chunk) }
                data.shutdownOutput()
                val reply = JSONObject(BufferedReader(InputStreamReader(data.getInputStream())).readLine())
                assertEquals(8L shl 20, reply.getLong("received"))
            }

            // Test des faux contacts : la sonde relève les coupures entre start et stop.
            client.send("""{"type":"watch","action":"start"}""")
            client.send("""{"type":"watch","action":"stop"}""")
            val report = client.receive()
            assertEquals("watch_report", report.getString("type"))
            assertEquals(1, report.getInt("power_cuts"))
            assertEquals(0, report.getInt("data_cuts"))
            assertEquals(2, report.getInt("charge_drops"))

            client.send(
                """{"type":"verdict","severity":"ok","title":"Câble USB 3","detail":"5 Gb/s",""" +
                    """"scores":[{"criterion":"transfer","rating":"Excellent","tone":"good","value":"5 Gb/s"}],""" +
                    """"facts":[{"label":"Débit vers le PC","value":"2,7 Gb/s"}],"advice":null}""",
            )
            client.send("""{"type":"bye"}""")
        }
        Thread.sleep(200)
        assertTrue(events.any { it is ProbeEvent.Connected && it.computer == "PC-TEST" })
        val verdict = events.filterIsInstance<ProbeEvent.Verdict>().single()
        assertEquals("Câble USB 3", verdict.title)
        assertEquals(VerdictScore("transfer", "Excellent", "good", "5 Gb/s"), verdict.scores.single())
        assertEquals("Débit vers le PC" to "2,7 Gb/s", verdict.facts.single())
        assertNull(verdict.advice)
        assertTrue(events.last() is ProbeEvent.Disconnected)
    }

    @Test
    fun refusalAndWrongSession() {
        approve = false
        Socket(loopback, port).use { socket ->
            assertEquals("refused", handshake(Client(socket)).getString("type"))
        }
        // Sans session acceptée, le port de débit ne répond rien.
        Socket(loopback, port + 1).use { data ->
            data.getOutputStream().write("""{"session":"faux","direction":"to_computer","duration_ms":500}""".plus("\n").toByteArray())
            assertEquals(-1, data.getInputStream().read())
        }
    }

    /**
     * Sonde de développement : `CABLESCOPE_PROBE_SERVE=60 ./gradlew :app:testDebugUnitTest --rerun`
     * sert la sonde sur 127.0.0.1:47820 pendant 60 s, pour essayer le logiciel
     * sans téléphone (`CABLESCOPE_PROBE=127.0.0.1 cablescope`).
     */
    @Test
    fun serveForDevelopment() {
        val seconds = System.getenv("CABLESCOPE_PROBE_SERVE")?.toLongOrNull() ?: return
        ProbeServer(host, loopback).use {
            it.start()
            Thread.sleep(seconds * 1000)
        }
        println("Événements : $events")
    }

    @Test
    fun currentNormalization() {
        assertEquals(1_250_000L, normalizeCurrentUa(1_250_000, charging = true))
        assertEquals(1_250_000L, normalizeCurrentUa(1250, charging = true)) // milliampères
        assertEquals(1_250_000L, normalizeCurrentUa(-1_250_000, charging = true)) // signe inversé
        assertEquals(-300_000L, normalizeCurrentUa(-300_000, charging = false)) // décharge
        assertEquals(4_000L, normalizeCurrentUa(4_000, charging = false)) // batterie pleine
        assertNull(normalizeCurrentUa(Int.MIN_VALUE, charging = true))
    }

    @Test
    fun tetherInterfaceNames() {
        assertTrue(isUsbTetherInterface("rndis0"))
        assertTrue(isUsbTetherInterface("ncm0"))
        assertTrue(!isUsbTetherInterface("wlan0"))
        assertTrue(!isUsbTetherInterface("rmnet_data0"))
    }

    @Test
    fun tetherAddressIsPickedAmongInterfaces() {
        val wifi = java.net.InetAddress.getByName("192.168.1.20") as java.net.Inet4Address
        val usb = java.net.InetAddress.getByName("192.168.42.129") as java.net.Inet4Address
        assertEquals(usb, usbTetherAddress(listOf("wlan0" to wifi, "rndis0" to usb)))
        assertNull(usbTetherAddress(listOf("wlan0" to wifi)))
    }
}
