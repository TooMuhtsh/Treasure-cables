package io.github.toomuhtsh.cablescope

/**
 * Surveille le câble pendant qu'on le plie : coupures de l'alimentation, de
 * la liaison de données, baisses de la charge. Sans dépendance à Android
 * (testé sur la JVM) : l'application lui transmet les événements du système
 * et les mesures de courant.
 */
class CableWatch(
    /** Appelé à chaque coupure franche, pour la signaler tout de suite. */
    private val onCut: () -> Unit = {},
) {
    private var powered: Boolean? = null
    private var data: Boolean? = null
    private var source: String? = null
    private var powerCuts = 0
    private var dataCuts = 0
    private var sourceChanges = 0
    private val currents = mutableListOf<Long>()

    /** L'alimentation est là ou non (événement du système ou relevé). */
    @Synchronized
    fun power(connected: Boolean) {
        if (powered == true && !connected) {
            powerCuts++
            onCut()
        }
        powered = connected
    }

    /** La liaison de données USB est là ou non. */
    @Synchronized
    fun data(connected: Boolean) {
        if (data == true && !connected) {
            dataCuts++
            onCut()
        }
        data = connected
    }

    /**
     * Relevé périodique : courant entrant dans la batterie et type de source
     * (`usb`, `ac`…). Un changement de type de source sans débranchement
     * trahit une renégociation de la charge.
     */
    @Synchronized
    fun sample(currentUa: Long?, plugged: String?) {
        if (currentUa != null) currents += currentUa
        when (plugged) {
            null -> {}
            "none" -> power(false)
            else -> {
                if (source != null && plugged != source) sourceChanges++
                source = plugged
                power(true)
            }
        }
    }

    /** Nombre de coupures franches jusqu'ici. */
    @get:Synchronized
    val cuts: Int get() = powerCuts + dataCuts

    @Synchronized
    fun report(): WatchReport {
        val median = currents.sorted().let { if (it.isEmpty()) null else it[it.size / 2] }
        return WatchReport(
            powerCuts = powerCuts,
            dataCuts = dataCuts,
            chargeDrops = chargeDrops(median) + sourceChanges,
            medianCurrentUa = median,
            samples = currents.size,
        )
    }

    /**
     * Baisses de charge : suites de relevés sous [DROP_RATIO] du courant
     * habituel. Seulement si la batterie se charge franchement : sinon, les
     * variations de consommation du téléphone suffisent à faire bouger le
     * courant.
     */
    private fun chargeDrops(median: Long?): Int {
        if (median == null || median < MIN_CHARGE_UA) return 0
        var count = 0
        var low = false
        for (ua in currents) {
            val dropped = ua < median * DROP_RATIO
            if (dropped && !low) count++
            low = dropped
        }
        return count
    }

    companion object {
        const val MIN_CHARGE_UA = 300_000L
        const val DROP_RATIO = 0.2
    }
}

/** Bilan d'une surveillance (message `watch_report` du protocole). */
data class WatchReport(
    val powerCuts: Int,
    val dataCuts: Int,
    val chargeDrops: Int,
    val medianCurrentUa: Long?,
    val samples: Int,
)
