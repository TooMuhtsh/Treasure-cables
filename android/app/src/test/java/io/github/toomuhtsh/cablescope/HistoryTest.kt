package io.github.toomuhtsh.cablescope

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class HistoryTest {
    private fun charge(time: Long, ua: Long, charger: String?, cable: String?) = Measurement(
        timeMs = time,
        kind = MeasurementKind.Charge,
        charger = charger,
        cable = cable,
        medianUa = ua,
        levelPercent = 40,
        verdict = ProbeEvent.Verdict(
            "ok",
            "Charge rapide",
            "détail",
            listOf(VerdictScore("charge", "Rapide", "good", formatAmps(ua))),
            listOf("Courant reçu" to formatAmps(ua)),
            null,
        ),
    )

    @Test
    fun roundTrip() {
        val history = listOf(
            charge(1, 2_000_000, "chargeur chambre", "noir 2 m"),
            charge(2, 900_000, null, null).copy(kind = MeasurementKind.Contacts),
            charge(3, 0, null, "blanc").copy(kind = MeasurementKind.Computer, computer = "PC-SALON", medianUa = null),
        )
        assertEquals(history, measurementsFromJson(measurementsToJson(history)))
        assertEquals(emptyList<Measurement>(), measurementsFromJson("pas du json"))
    }

    @Test
    fun firstVersionHistoryIsKept() {
        val old = """[{"time_ms":5,"median_ua":4290000,"level_percent":41,"plugged":"ac"}]"""
        val m = measurementsFromJson(old).single()
        assertEquals(MeasurementKind.Charge, m.kind)
        assertEquals(4_290_000L, m.medianUa)
        assertEquals("Charge rapide", m.verdict.title)
        assertEquals("Rapide", m.mainScore!!.rating)
        assertEquals("Mesure de charge", m.title)
    }

    @Test
    fun comparisonStaysOnTheSameCharger() {
        val history = listOf(
            charge(1, 4_000_000, "Chargeur chambre", "noir"),
            charge(2, 2_000_000, "batterie portable", "noir"),
            charge(3, 3_000_000, null, null),
        )
        assertEquals(Comparison(4_000_000, "câble « noir »"), bestCharge(history, "chargeur chambre "))
        assertEquals(Comparison(2_000_000, "câble « noir »"), bestCharge(history, "Batterie portable"))
        assertEquals(Comparison(3_000_000, "meilleur"), bestCharge(history, null))
        assertNull(bestCharge(history, "voiture"))
    }

    @Test
    fun cablesRankedPerCharger() {
        val history = listOf(
            charge(1, 3_000_000, "chambre", "noir"),
            charge(2, 3_500_000, "chambre", "Noir"),
            charge(3, 1_200_000, "chambre", "blanc"),
            charge(4, 2_000_000, "voiture", "noir"), // un seul câble : pas de classement
        )
        assertEquals(
            listOf("chambre" to listOf("Noir" to 3_500_000L, "blanc" to 1_200_000L)),
            cableRanking(history),
        )
        assertEquals(listOf("voiture", "chambre"), usedNames(history.map { it.charger }))
    }
}
