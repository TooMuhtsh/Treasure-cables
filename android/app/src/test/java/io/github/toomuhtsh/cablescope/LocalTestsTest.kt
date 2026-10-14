package io.github.toomuhtsh.cablescope

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CableWatchTest {
    @Test
    fun cutsAreCountedOncePerTransition() {
        var beeps = 0
        val watch = CableWatch { beeps++ }
        watch.sample(1_500_000, "ac")
        watch.power(false) // événement du système
        watch.sample(-200_000, "none") // même coupure, vue au relevé suivant
        watch.power(true)
        watch.sample(1_500_000, "ac")
        watch.data(true)
        watch.data(false)
        val report = watch.report()
        assertEquals(1, report.powerCuts)
        assertEquals(1, report.dataCuts)
        assertEquals(2, beeps)
        assertEquals(2, watch.cuts)
    }

    @Test
    fun chargeDropsNeedARealCharge() {
        val charging = CableWatch()
        listOf(1_500_000L, 1_520_000, 100_000, 80_000, 1_490_000, 1_510_000, 200_000, 1_500_000)
            .forEach { charging.sample(it, "ac") }
        assertEquals(2, charging.report().chargeDrops)

        // Batterie presque pleine : quelques dizaines de mA qui varient ne
        // sont pas des baisses.
        val trickle = CableWatch()
        listOf(60_000L, 10_000, 70_000, 5_000).forEach { trickle.sample(it, "ac") }
        assertEquals(0, trickle.report().chargeDrops)
    }

    @Test
    fun sourceChangeIsARenegotiation() {
        val watch = CableWatch()
        watch.sample(1_500_000, "ac")
        watch.sample(450_000, "usb")
        watch.sample(1_500_000, "ac")
        val report = watch.report()
        assertEquals(0, report.powerCuts)
        assertTrue(report.chargeDrops >= 2)
    }
}

class LocalTestsTest {
    private fun measure(ua: Long, level: Int = 40, plugged: String = "ac", usbData: Boolean = false, cuts: Int = 0) =
        ChargeMeasure(ua, ua - 100_000, ua + 50_000, 3900, level, 31.5, plugged, usbData, cuts)

    @Test
    fun fastCharge() {
        val v = chargeVerdict(measure(2_000_000), emptyList())
        assertEquals("ok", v.severity)
        assertEquals("Charge rapide", v.title)
        assertEquals(VerdictScore("charge", "Rapide", "good", "2,00 A · ≈ 7,8 W"), v.scores.single())
        assertTrue(v.facts.contains("Source" to "chargeur"))
        assertNull(v.advice)
    }

    @Test
    fun comparedWithTheBestCable() {
        val history = listOf(ChargeResult(0, 2_000_000, 40, "ac"), ChargeResult(1, 1_000_000, 40, "ac"))
        val v = chargeVerdict(measure(1_200_000), history)
        assertEquals("Normale", v.scores.single().rating)
        assertTrue(v.facts.contains("Par rapport au meilleur" to "-40 % (meilleur : 2,00 A)"))
        assertTrue(v.advice!!.contains("moins vite que votre meilleur"))
    }

    @Test
    fun caveatsComeFirst() {
        assertTrue(chargeVerdict(measure(800_000, level = 85), emptyList()).advice!!.contains("au-delà de 80 %"))
        val plain = chargeVerdict(measure(450_000, plugged = "usb"), emptyList())
        assertEquals("Lente", plain.scores.single().rating)
        assertTrue(plain.advice!!.contains("prise USB simple"))
        val cut = chargeVerdict(measure(1_500_000, cuts = 1), emptyList())
        assertEquals("problem", cut.severity)
        assertTrue(cut.advice!!.contains("faux contacts"))
    }

    private fun zone(name: String, power: Int = 0, drops: Int = 0, data: Int = 0) =
        ZoneResult(name, WatchReport(power, data, drops, 1_500_000, 20))

    @Test
    fun contacts() {
        val rest = zone("au repos")
        val solid = contactsVerdict(rest, listOf(zone("côté téléphone"), zone("côté chargeur")), "ac", false)
        assertEquals("Contacts solides", solid.title)
        assertEquals(listOf("charge", "contacts"), solid.scores.map { it.criterion })
        assertEquals("Solides", solid.scores.last().rating)
        assertTrue(solid.facts.contains("Pliage côté téléphone" to "aucune coupure"))

        val fragile = contactsVerdict(rest, listOf(zone("côté téléphone", drops = 1), zone("côté chargeur")), "ac", false)
        assertEquals("Câble fragile côté téléphone", fragile.title)
        assertEquals("warning", fragile.severity)

        val broken = contactsVerdict(
            rest,
            listOf(zone("côté téléphone", power = 2, drops = 1), zone("côté chargeur", data = 1)),
            "usb",
            true,
        )
        assertEquals("Faux contact côté téléphone et côté chargeur", broken.title)
        assertEquals(VerdictScore("contacts", "Faux contact", "bad", "coupe côté téléphone et côté chargeur"), broken.scores.last())
        assertTrue(broken.facts.contains("Pliage côté téléphone" to "2 coupures de charge, 1 baisse de charge"))
    }

    @Test
    fun historyRoundTrip() {
        val history = listOf(ChargeResult(1, 2_000_000, 40, "ac"), ChargeResult(2, 500_000, null, null))
        assertEquals(history, historyFromJson(historyToJson(history)))
        assertEquals(emptyList<ChargeResult>(), historyFromJson("pas du json"))
    }
}
