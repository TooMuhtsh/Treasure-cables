package io.github.toomuhtsh.cablescope

import org.json.JSONArray
import org.json.JSONObject

// Historique des mesures : chaque test garde son verdict complet (pour le
// relire après coup) et les noms donnés au chargeur et au câble (pour
// comparer les câbles sur un même chargeur).

enum class MeasurementKind(val key: String, val label: String) {
    Charge("charge", "Mesure de charge"),
    Contacts("contacts", "Faux contacts"),
    Computer("computer", "Test avec l'ordinateur"),
    ;

    companion object {
        fun of(key: String?) = entries.firstOrNull { it.key == key } ?: Charge
    }
}

data class Measurement(
    /** Heure de la mesure, qui sert aussi d'identifiant. */
    val timeMs: Long,
    val kind: MeasurementKind,
    /** Nom donné à la source de charge (« chargeur chambre », « batterie portable »). */
    val charger: String?,
    /** Nom donné au câble (« noir 2 m »). */
    val cable: String?,
    /** Ordinateur, pour un test lancé depuis CableScope. */
    val computer: String? = null,
    /** Courant de charge médian, pour comparer les câbles. */
    val medianUa: Long? = null,
    val levelPercent: Int? = null,
    val verdict: ProbeEvent.Verdict,
) {
    /** « chargeur chambre · noir 2 m », ou le type de mesure faute de nom. */
    val title: String
        get() = listOfNotNull(charger ?: computer?.let { "« $it »" }, cable).joinToString(" · ").ifEmpty { kind.label }

    /** La note qui résume la mesure. */
    val mainScore: VerdictScore?
        get() = verdict.scores.firstOrNull { it.criterion == "overall" }
            ?: verdict.scores.firstOrNull { it.criterion == "contacts" }
            ?: verdict.scores.firstOrNull()
}

/** Meilleure charge mesurée à laquelle se comparer. */
data class Comparison(val medianUa: Long, val label: String)

/**
 * Meilleure mesure de charge comparable : sur le même chargeur s'il est
 * nommé, sinon parmi les mesures sans nom de chargeur.
 */
fun bestCharge(history: List<Measurement>, charger: String?): Comparison? =
    history
        .filter { it.kind == MeasurementKind.Charge && (it.medianUa ?: 0) > 0 && sameName(it.charger, charger) }
        .maxByOrNull { it.medianUa!! }
        ?.let { Comparison(it.medianUa!!, it.cable?.let { c -> "câble « $c »" } ?: "meilleur") }

/**
 * Classement des câbles par chargeur nommé : pour chaque chargeur, la
 * meilleure charge de chaque câble, du plus rapide au plus lent. Seuls les
 * chargeurs mesurés avec au moins deux câbles différents apparaissent.
 */
fun cableRanking(history: List<Measurement>): List<Pair<String, List<Pair<String, Long>>>> =
    history
        .filter { it.kind == MeasurementKind.Charge && it.charger != null && (it.medianUa ?: 0) > 0 }
        .groupBy { it.charger!!.trim().lowercase() }
        .mapNotNull { (_, entries) ->
            val cables = entries
                .groupBy { it.cable?.trim()?.lowercase() }
                .map { (_, same) ->
                    val best = same.maxBy { it.medianUa!! }
                    (best.cable ?: "câble sans nom") to best.medianUa!!
                }
                .sortedByDescending { it.second }
            if (cables.size < 2) null else entries.last().charger!! to cables
        }

/** Noms déjà employés, du plus récent au plus ancien, sans doublon. */
fun usedNames(names: List<String?>): List<String> =
    names.asReversed().filterNotNull().map { it.trim() }.filter { it.isNotEmpty() }
        .distinctBy { it.lowercase() }

private fun sameName(a: String?, b: String?) =
    a?.trim()?.lowercase()?.ifEmpty { null } == b?.trim()?.lowercase()?.ifEmpty { null }

fun measurementsToJson(history: List<Measurement>): String = JSONArray(
    history.map { m ->
        JSONObject()
            .put("time_ms", m.timeMs)
            .put("kind", m.kind.key)
            .put("charger", m.charger ?: JSONObject.NULL)
            .put("cable", m.cable ?: JSONObject.NULL)
            .put("computer", m.computer ?: JSONObject.NULL)
            .put("median_ua", m.medianUa ?: JSONObject.NULL)
            .put("level_percent", m.levelPercent ?: JSONObject.NULL)
            .put("verdict", verdictToJson(m.verdict))
    },
).toString()

/**
 * Relit l'historique. Les mesures de la première version (sans verdict
 * enregistré) sont reprises avec un verdict reconstitué.
 */
fun measurementsFromJson(json: String?): List<Measurement> {
    val array = runCatching { JSONArray(json ?: return emptyList()) }.getOrNull() ?: return emptyList()
    return (0 until array.length()).mapNotNull { i ->
        val o = array.optJSONObject(i) ?: return@mapNotNull null
        val median = o.optLongOrNull("median_ua")
        val level = o.optIntOrNull("level_percent")
        val verdict = o.optJSONObject("verdict")?.let(::verdictFromJson)
            ?: legacyVerdict(median ?: return@mapNotNull null, level, o.optStringOrNull("plugged"))
        Measurement(
            timeMs = o.optLong("time_ms"),
            kind = MeasurementKind.of(o.optStringOrNull("kind")),
            charger = o.optStringOrNull("charger"),
            cable = o.optStringOrNull("cable"),
            computer = o.optStringOrNull("computer"),
            medianUa = median,
            levelPercent = level,
            verdict = verdict,
        )
    }
}

private fun legacyVerdict(medianUa: Long, level: Int?, plugged: String?): ProbeEvent.Verdict {
    val (rating, tone) = chargeRating(medianUa)
    val facts = listOfNotNull(
        "Source" to sourceLabel(plugged, false),
        "Courant reçu" to formatAmps(medianUa),
        level?.let { "Batterie" to "$it %" },
    )
    return ProbeEvent.Verdict(
        severity = if (tone == "good") "ok" else "warning",
        title = "Charge ${rating.lowercase()}",
        detail = facts.joinToString(" · ") { (label, value) -> "$label : $value" },
        scores = listOf(VerdictScore("charge", rating, tone, formatAmps(medianUa))),
        facts = facts,
    )
}

fun verdictToJson(v: ProbeEvent.Verdict): JSONObject = JSONObject()
    .put("severity", v.severity)
    .put("title", v.title)
    .put("detail", v.detail)
    .put(
        "scores",
        JSONArray(
            v.scores.map {
                JSONObject()
                    .put("criterion", it.criterion)
                    .put("rating", it.rating)
                    .put("tone", it.tone)
                    .put("value", it.value ?: JSONObject.NULL)
            },
        ),
    )
    .put("facts", JSONArray(v.facts.map { (label, value) -> JSONObject().put("label", label).put("value", value) }))
    .put("advice", v.advice ?: JSONObject.NULL)

fun verdictFromJson(o: JSONObject): ProbeEvent.Verdict {
    val scores = o.optJSONArray("scores")
    val facts = o.optJSONArray("facts")
    return ProbeEvent.Verdict(
        severity = o.optString("severity", "info"),
        title = o.optString("title"),
        detail = o.optString("detail"),
        scores = (0 until (scores?.length() ?: 0)).mapNotNull { i ->
            scores!!.optJSONObject(i)?.let {
                VerdictScore(it.optString("criterion"), it.optString("rating"), it.optString("tone", "unknown"), it.optStringOrNull("value"))
            }
        },
        facts = (0 until (facts?.length() ?: 0)).mapNotNull { i ->
            facts!!.optJSONObject(i)?.let { it.optString("label") to it.optString("value") }
        },
        advice = o.optStringOrNull("advice"),
    )
}

private fun JSONObject.optStringOrNull(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf { it.isNotBlank() }

private fun JSONObject.optLongOrNull(key: String): Long? = if (isNull(key)) null else optLong(key)

private fun JSONObject.optIntOrNull(key: String): Int? = if (isNull(key)) null else optInt(key)
