package io.github.toomuhtsh.cablescope

import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale

// Tests faits par le téléphone seul, sans ordinateur : mesure de la charge
// et test des faux contacts. Verdicts présentés comme ceux du logiciel
// (mêmes notes, mêmes seuils).

/** Ce qu'a mesuré le test de charge. */
data class ChargeMeasure(
    /** Courant médian entrant dans la batterie, en microampères. */
    val medianUa: Long,
    val minUa: Long,
    val maxUa: Long,
    /** Tension de la batterie, pour estimer la puissance reçue. */
    val voltageMv: Int?,
    val levelPercent: Int?,
    val temperatureC: Double?,
    /** Type de source vu par Android (`usb`, `ac`…). */
    val plugged: String?,
    /** Un ordinateur échange des données avec le téléphone. */
    val usbData: Boolean,
    /** Coupures de l'alimentation pendant la mesure. */
    val powerCuts: Int,
)

/** Un test de charge gardé pour comparer les câbles entre eux. */
data class ChargeResult(
    val timeMs: Long,
    val medianUa: Long,
    val levelPercent: Int?,
    val plugged: String?,
)

/** Une zone du câble pendant le test des faux contacts. */
data class ZoneResult(val zone: String, val report: WatchReport) {
    val cuts get() = report.powerCuts + report.dataCuts
    val wobbles get() = report.chargeDrops

    fun summary(): String {
        val parts = listOf(
            Triple(report.powerCuts, "coupure de charge", "coupures de charge"),
            Triple(report.dataCuts, "coupure des données", "coupures des données"),
            Triple(report.chargeDrops, "baisse de charge", "baisses de charge"),
        ).filter { it.first > 0 }.map { (n, one, many) -> "$n ${if (n == 1) one else many}" }
        return parts.ifEmpty { listOf("aucune coupure") }.joinToString(", ")
    }
}

/**
 * Le courant de charge s'est stabilisé : la charge rapide monte par paliers
 * pendant plusieurs secondes après le branchement. Stable quand les 2
 * dernières secondes ne dépassent plus de 5 % les 2 précédentes (au moins
 * 5 s de relevés), ou que la batterie ne se charge pas.
 */
fun chargeSettled(currents: List<Long>, sampleMs: Long): Boolean {
    val window = (2_000 / sampleMs).toInt().coerceAtLeast(1)
    if (currents.size * sampleMs < 5_000 || currents.size < 2 * window) return false
    val median = { values: List<Long> -> values.sorted()[values.size / 2] }
    val recent = median(currents.takeLast(window))
    val before = median(currents.dropLast(window).takeLast(window))
    return recent <= 0 || recent <= before * 105 / 100
}

fun chargeRating(ua: Long): Pair<String, String> = when {
    ua >= 1_500_000 -> "Rapide" to "good"
    ua >= 500_000 -> "Normale" to "good"
    ua > 0 -> "Lente" to "fair"
    else -> "Nulle" to "bad"
}

fun chargeVerdict(m: ChargeMeasure, history: List<ChargeResult>): ProbeEvent.Verdict {
    val (rating, tone) = chargeRating(m.medianUa)
    val full = (m.levelPercent ?: 0) >= 95
    val watts = m.voltageMv?.takeIf { m.medianUa > 0 }?.let { m.medianUa / 1e6 * it / 1000.0 }
    val best = history.maxByOrNull { it.medianUa }?.takeIf { it.medianUa > 0 }

    val scoreValue = if (m.medianUa > 0) {
        listOfNotNull(formatAmps(m.medianUa), watts?.let { "≈ ${formatWatts(it)}" }).joinToString(" · ")
    } else {
        "aucun courant"
    }
    val scores = listOf(
        VerdictScore("charge", rating, if (rating == "Nulle" && full) "unknown" else tone, scoreValue),
    )

    val facts = buildList {
        add("Source" to sourceLabel(m.plugged, m.usbData))
        add("Courant reçu" to if (m.medianUa > 0) formatAmps(m.medianUa) else "aucun")
        if (m.maxUa > 0) add("Variation" to "${formatAmps(m.minUa.coerceAtLeast(0))} à ${formatAmps(m.maxUa)}")
        watts?.let { add("Puissance reçue par la batterie" to "≈ ${formatWatts(it)}") }
        m.levelPercent?.let { add("Batterie" to "$it %") }
        m.temperatureC?.let { add("Température" to String.format(Locale.FRANCE, "%.1f °C", it)) }
        if (m.powerCuts > 0) add("Coupures" to "${m.powerCuts}")
        if (best != null && m.medianUa > 0) {
            val diff = ((m.medianUa - best.medianUa) * 100 / best.medianUa).toInt()
            add(
                "Par rapport au meilleur" to when {
                    m.medianUa >= best.medianUa -> "meilleur résultat"
                    else -> "$diff % (meilleur : ${formatAmps(best.medianUa)})"
                },
            )
        }
    }

    val advice = when {
        m.powerCuts > 0 ->
            "La charge s'est coupée pendant la mesure : faux contact probable. Lancez le test des faux contacts."
        full && m.medianUa <= 0 ->
            "La batterie est pleine : refaites la mesure quand elle sera descendue sous 50 %."
        (m.levelPercent ?: 0) >= 80 ->
            "Batterie à ${m.levelPercent} % : le téléphone ralentit la charge au-delà de 80 %. Pour comparer des câbles, mesurez sous 50 %."
        (m.temperatureC ?: 0.0) >= 40 ->
            "Le téléphone est chaud : il ralentit la charge pour se protéger. Laissez-le refroidir avant de comparer."
        m.plugged == "usb" && !m.usbData ->
            "Le téléphone ne voit qu'une prise USB simple (0,5 A au plus) : chargeur basique, ou câble qui bloque la charge rapide. Essayez un autre câble sur ce chargeur pour les départager."
        m.plugged == "usb" ->
            "Un port d'ordinateur limite la charge : mesurez sur un chargeur pour juger le câble."
        best != null && m.medianUa < best.medianUa * 7 / 10 ->
            "Ce câble charge nettement moins vite que votre meilleur résultat : avec le même chargeur, c'est le câble qui limite."
        else -> null
    }

    val (severity, title) = when {
        m.powerCuts > 0 -> "problem" to "La charge s'est coupée pendant la mesure"
        rating == "Nulle" -> (if (full) "info" else "problem") to "Le téléphone ne se charge pas"
        rating == "Lente" -> "warning" to "Charge lente"
        rating == "Normale" -> "ok" to "Charge normale"
        else -> "ok" to "Charge rapide"
    }
    return ProbeEvent.Verdict(
        severity = severity,
        title = title,
        detail = facts.joinToString(" · ") { (label, value) -> "$label : $value" },
        scores = scores,
        facts = facts,
        advice = advice,
    )
}

/**
 * Verdict du test des faux contacts. `rest` est la mesure câble au repos
 * (référence de la charge) ; `zones` les zones pliées tour à tour.
 */
fun contactsVerdict(rest: ZoneResult, zones: List<ZoneResult>, plugged: String?, usbData: Boolean): ProbeEvent.Verdict {
    val all = listOf(rest) + zones
    val broken = all.filter { it.cuts > 0 }.map { it.zone }
    val fragile = all.filter { it.wobbles > 0 }.map { it.zone }
    val where = { list: List<String> -> list.joinToString(" et ") }

    val (rating, tone, value) = when {
        broken.isNotEmpty() -> Triple("Faux contact", "bad", "coupe ${where(broken)}")
        fragile.isNotEmpty() -> Triple("Fragiles", "fair", "faiblit ${where(fragile)}")
        else -> Triple("Solides", "good", "aucune coupure")
    }
    val scores = buildList {
        rest.report.medianCurrentUa?.let { ua ->
            val (r, t) = chargeRating(ua)
            add(VerdictScore("charge", r, t, if (ua > 0) formatAmps(ua) else "aucun courant"))
        }
        add(VerdictScore("contacts", rating, tone, value))
    }
    val facts = buildList {
        add("Source" to sourceLabel(plugged, usbData))
        all.forEach { add((if (it === rest) "Câble au repos" else "Pliage ${it.zone}") to it.summary()) }
    }
    val (severity, title, advice) = when {
        broken.isNotEmpty() -> Triple(
            "problem",
            "Faux contact ${where(broken)}",
            "Remplacez ce câble : il coupe quand on le plie ${where(broken)}. Si tous vos câbles coupent du même côté, " +
                "c'est la prise qui est usée ou encrassée.",
        )
        fragile.isNotEmpty() -> Triple(
            "warning",
            "Câble fragile ${where(fragile)}",
            "La charge faiblit un instant quand on plie le câble ${where(fragile)} : ménagez-le, et remplacez-le s'il se met à couper.",
        )
        else -> Triple(
            "ok",
            "Contacts solides",
            if (usbData) null else "Sans ordinateur, seule la charge est surveillée : avec CableScope sur PC, les données le sont aussi.",
        )
    }
    return ProbeEvent.Verdict(
        severity = severity,
        title = title,
        detail = facts.joinToString(" · ") { (label, v) -> "$label : $v" },
        scores = scores,
        facts = facts,
        advice = advice,
    )
}

fun sourceLabel(plugged: String?, usbData: Boolean): String = when (plugged) {
    "usb" -> if (usbData) "port USB d'ordinateur" else "prise USB simple"
    // Un port USB-C d'ordinateur qui annonce 1,5 ou 3 A est classé
    // « chargeur » par Android.
    "ac" -> if (usbData) "port USB-C d'ordinateur (charge renforcée)" else "chargeur"
    "wireless" -> "sans fil"
    "dock" -> "station d'accueil"
    "none" -> "non branché"
    else -> "inconnue"
}

fun formatAmps(ua: Long): String =
    if (ua >= 1_000_000) String.format(Locale.FRANCE, "%.2f A", ua / 1_000_000.0) else "${ua / 1000} mA"

fun formatWatts(w: Double): String = String.format(Locale.FRANCE, "%.1f W", w)

fun historyToJson(history: List<ChargeResult>): String = JSONArray(
    history.map {
        JSONObject()
            .put("time_ms", it.timeMs)
            .put("median_ua", it.medianUa)
            .put("level_percent", it.levelPercent ?: JSONObject.NULL)
            .put("plugged", it.plugged ?: JSONObject.NULL)
    },
).toString()

fun historyFromJson(json: String?): List<ChargeResult> {
    val array = runCatching { JSONArray(json ?: return emptyList()) }.getOrNull() ?: return emptyList()
    return (0 until array.length()).mapNotNull { i ->
        val o = array.optJSONObject(i) ?: return@mapNotNull null
        ChargeResult(
            timeMs = o.optLong("time_ms"),
            medianUa = o.optLong("median_ua"),
            levelPercent = if (o.isNull("level_percent")) null else o.optInt("level_percent"),
            plugged = if (o.isNull("plugged")) null else o.optString("plugged"),
        )
    }
}
