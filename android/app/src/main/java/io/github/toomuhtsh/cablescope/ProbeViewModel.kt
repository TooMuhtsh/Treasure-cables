package io.github.toomuhtsh.cablescope

import android.app.Application
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioManager
import android.media.ToneGenerator
import android.os.BatteryManager
import android.os.Build
import android.provider.Settings
import androidx.core.content.ContextCompat
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.net.InetAddress
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/** État affiché par l'écran. */
data class UiState(
    /** Un ordinateur est relié en USB (données). */
    val usbConnected: Boolean = false,
    /** Adresse du partage de connexion USB, s'il est actif. */
    val tetherAddress: String? = null,
    val battery: BatteryReading? = null,
    /** Ordinateur connecté à la sonde. */
    val computer: String? = null,
    /** Ordinateur qui demande l'accord de l'utilisateur. */
    val approvalFor: String? = null,
    val status: String? = null,
    val testing: Boolean = false,
    val verdict: ProbeEvent.Verdict? = null,
    /** Interfaces réseau vues (« rndis0 192.168.42.129 »), pour le diagnostic. */
    val interfaces: List<String> = emptyList(),
    /** Coupures relevées pendant le test des faux contacts lancé par l'ordinateur. */
    val watchCuts: Int? = null,
    /** Test fait par le téléphone seul, en cours. */
    val localTest: LocalTest? = null,
    val localVerdict: ProbeEvent.Verdict? = null,
    val localError: String? = null,
    /** Mesures de charge gardées pour comparer les câbles. */
    val chargeHistory: List<ChargeResult> = emptyList(),
)

enum class LocalKind { Charge, Contacts }

/** Avancement d'un test fait par le téléphone seul. */
data class LocalTest(
    val kind: LocalKind,
    /** Consigne ou étape en cours. */
    val step: String,
    val secondsLeft: Int,
    val progress: Float,
    val currentUa: Long?,
    val cuts: Int,
)

/** Fin d'un test sans verdict, avec l'explication pour l'utilisateur. */
private class LocalTestFailed(message: String) : Exception(message)

/**
 * Relie le serveur de la sonde au téléphone : il démarre quand le partage de
 * connexion USB apparaît et s'arrête quand il disparaît.
 */
class ProbeViewModel(application: Application) : AndroidViewModel(application), ProbeHost {
    private val context: Context get() = getApplication()
    private val _state = MutableStateFlow(UiState())
    val state = _state.asStateFlow()

    private var server: ProbeServer? = null
    private var serverAddress: InetAddress? = null
    @Volatile private var approval: CompletableFuture<Boolean>? = null

    /** Surveillance des coupures en cours (test des faux contacts). */
    @Volatile private var watch: CableWatch? = null
    private var sampler: Job? = null
    private var localJob: Job? = null
    private val prefs = application.getSharedPreferences("cablescope", Context.MODE_PRIVATE)
    private val beeper = runCatching { ToneGenerator(AudioManager.STREAM_NOTIFICATION, 80) }.getOrNull()

    /** Débranchements et coupures de la liaison USB, signalés par le système. */
    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val w = watch ?: return
            when (intent.action) {
                Intent.ACTION_POWER_CONNECTED -> w.power(true)
                Intent.ACTION_POWER_DISCONNECTED -> w.power(false)
                USB_STATE -> w.data(intent.getBooleanExtra("connected", false))
            }
        }
    }

    init {
        ContextCompat.registerReceiver(
            context,
            receiver,
            IntentFilter().apply {
                addAction(Intent.ACTION_POWER_CONNECTED)
                addAction(Intent.ACTION_POWER_DISCONNECTED)
                addAction(USB_STATE)
            },
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        _state.update { it.copy(chargeHistory = historyFromJson(prefs.getString(HISTORY, null))) }

        // Hors du fil de l'interface : ouverture des ports du serveur.
        viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                refresh()
                delay(1000)
            }
        }
    }

    private fun refresh() {
        val interfaces = ipv4Interfaces()
        val address = usbTetherAddress(interfaces)
        if (address != serverAddress) {
            server?.close()
            server = null
            serverAddress = address
            if (address != null) {
                server = runCatching { ProbeServer(this, address).also { it.start() } }.getOrNull()
            }
        }
        _state.update {
            it.copy(
                tetherAddress = address?.hostAddress,
                battery = battery(),
                usbConnected = usbConnected(),
                interfaces = interfaces.map { (name, ip) -> "$name ${ip.hostAddress}" },
            )
        }
    }

    /** Réponse de l'utilisateur à la demande d'accord. */
    fun answer(accept: Boolean) {
        approval?.complete(accept)
    }

    fun openTetherSettings(context: Context) {
        val candidates = listOf(
            Intent().setClassName("com.android.settings", "com.android.settings.TetherSettings"),
            Intent().setClassName("com.android.settings", "com.android.settings.Settings\$TetherSettingsActivity"),
            Intent(Settings.ACTION_WIRELESS_SETTINGS),
            Intent(Settings.ACTION_SETTINGS),
        )
        for (intent in candidates) {
            val opened = runCatching {
                context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            }.isSuccess
            if (opened) return
        }
    }

    override fun onCleared() {
        server?.close()
        approval?.complete(false)
        runCatching { context.unregisterReceiver(receiver) }
        beeper?.release()
    }

    // --- Tests sans ordinateur ------------------------------------------------

    /** Mesure de la charge : 5 s de mise en route, puis 25 s de relevés. */
    fun startCharge() = launchLocal(LocalKind.Charge) {
        val w = newWatch()
        val currents = mutableListOf<Long>()
        val voltages = mutableListOf<Int>()
        val sources = mutableListOf<String>()
        val plan = Plan(LocalKind.Charge, CHARGE_SETTLE_MS + CHARGE_MEASURE_MS)
        plan.step("La charge se met en route…", CHARGE_SETTLE_MS, w)
        val last = plan.step("Mesure du courant de charge…", CHARGE_MEASURE_MS, w) { b ->
            b.currentUa?.let { currents += it }
            b.voltageMv?.let { voltages += it }
            b.plugged?.takeIf { it != "none" }?.let { sources += it }
        }
        if (last.plugged == "none") throw LocalTestFailed("Le téléphone a été débranché pendant la mesure.")
        if (currents.isEmpty()) {
            throw LocalTestFailed("Ce téléphone ne donne pas son courant de charge : la mesure est impossible.")
        }
        val measure = ChargeMeasure(
            medianUa = median(currents),
            minUa = currents.min(),
            maxUa = currents.max(),
            voltageMv = voltages.takeIf { it.isNotEmpty() }?.let { median(it.map(Int::toLong)).toInt() },
            levelPercent = last.levelPercent,
            temperatureC = last.temperatureC,
            plugged = sources.groupingBy { it }.eachCount().maxByOrNull { it.value }?.key,
            usbData = usbConnected(),
            powerCuts = w.report().powerCuts,
        )
        val history = _state.value.chargeHistory
        val verdict = chargeVerdict(measure, history)
        if (measure.powerCuts == 0 && measure.medianUa > 0) {
            val updated = (history + ChargeResult(System.currentTimeMillis(), measure.medianUa, measure.levelPercent, measure.plugged))
                .takeLast(HISTORY_SIZE)
            prefs.edit().putString(HISTORY, historyToJson(updated)).apply()
            _state.update { it.copy(chargeHistory = updated) }
        }
        verdict
    }

    /**
     * Test des faux contacts : câble au repos (référence), puis plié près de
     * chaque fiche, en surveillant les coupures.
     */
    fun startContacts() = launchLocal(LocalKind.Contacts) {
        val data = usbConnected()
        val plugged = battery().plugged
        val other = if (data) "côté ordinateur" else "côté chargeur"
        val zones = listOf(
            "côté téléphone" to "Pliez doucement le câble près de la fiche du téléphone, dans tous les sens.",
            other to "Pliez doucement le câble près de l'autre fiche (${other.removePrefix("côté ")}), dans tous les sens.",
        )
        val plan = Plan(LocalKind.Contacts, REST_MS + zones.size * (GET_READY_MS + BEND_MS))
        val restWatch = newWatch()
        plan.step("Ne touchez pas le câble : mesure de référence.", REST_MS, restWatch)
        val rest = ZoneResult("au repos", restWatch.report())
        val results = zones.map { (zone, instruction) ->
            watch = null
            plan.step("Préparez-vous. $instruction", GET_READY_MS, null)
            val w = newWatch()
            plan.step(instruction, BEND_MS, w)
            ZoneResult(zone, w.report())
        }
        contactsVerdict(rest, results, plugged, data)
    }

    fun cancelLocal() {
        localJob?.cancel()
    }

    fun dismissLocal() {
        _state.update { it.copy(localVerdict = null, localError = null) }
    }

    fun clearChargeHistory() {
        prefs.edit().remove(HISTORY).apply()
        _state.update { it.copy(chargeHistory = emptyList()) }
    }

    private fun launchLocal(kind: LocalKind, body: suspend () -> ProbeEvent.Verdict) {
        if (localJob?.isActive == true || _state.value.computer != null) return
        val plugged = battery().plugged
        if (plugged == null || plugged == "none") {
            _state.update {
                it.copy(localVerdict = null, localError = "Branchez d'abord le téléphone avec le câble à tester.")
            }
            return
        }
        _state.update {
            it.copy(
                localVerdict = null,
                localError = null,
                localTest = LocalTest(kind, "Préparation…", 0, 0f, null, 0),
            )
        }
        localJob = viewModelScope.launch(Dispatchers.Default) {
            try {
                val verdict = body()
                _state.update { it.copy(localVerdict = verdict) }
            } catch (e: LocalTestFailed) {
                _state.update { it.copy(localError = e.message) }
            } finally {
                watch = null
                _state.update { it.copy(localTest = null) }
            }
        }
    }

    /** Déroulé d'un test : des étapes chronométrées, une barre d'avancement commune. */
    private inner class Plan(val kind: LocalKind, val totalMs: Long) {
        private var doneMs = 0L

        /**
         * Une étape de `durationMs`, avec un relevé toutes les 250 ms transmis
         * à la surveillance `w` et à `sample`. Renvoie le dernier relevé.
         */
        suspend fun step(
            text: String,
            durationMs: Long,
            w: CableWatch?,
            sample: (BatteryReading) -> Unit = {},
        ): BatteryReading {
            val start = System.currentTimeMillis()
            var last = battery()
            while (true) {
                val elapsed = System.currentTimeMillis() - start
                if (elapsed >= durationMs) break
                last = battery()
                w?.sample(last.currentUa, last.plugged)
                sample(last)
                _state.update {
                    it.copy(
                        localTest = LocalTest(
                            kind = kind,
                            step = text,
                            secondsLeft = ((durationMs - elapsed + 999) / 1000).toInt(),
                            progress = (doneMs + elapsed).toFloat() / totalMs,
                            currentUa = last.currentUa,
                            cuts = w?.cuts ?: 0,
                        ),
                    )
                }
                delay(SAMPLE_MS)
            }
            doneMs += durationMs
            return last
        }
    }

    /** Nouvelle surveillance, partant de l'état actuel du branchement. */
    private fun newWatch(): CableWatch = CableWatch(onCut = ::beep).also { w ->
        val plugged = battery().plugged
        w.power(plugged != null && plugged != "none")
        w.data(usbConnected())
        watch = w
    }

    private fun beep() {
        runCatching { beeper?.startTone(ToneGenerator.TONE_PROP_BEEP, 150) }
    }

    private fun median(values: List<Long>): Long = values.sorted()[values.size / 2]

    // --- ProbeHost ----------------------------------------------------------

    override fun device() = DeviceInfo(Build.MANUFACTURER, Build.MODEL, Build.VERSION.RELEASE)

    override fun version(): String =
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }
            .getOrNull() ?: "?"

    override fun battery(): BatteryReading {
        val intent = sticky(Intent.ACTION_BATTERY_CHANGED)
        val manager = context.getSystemService(BatteryManager::class.java)
        val status = intent?.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
        val charging = status == BatteryManager.BATTERY_STATUS_CHARGING
        val plugged = when (intent?.getIntExtra(BatteryManager.EXTRA_PLUGGED, -1)) {
            0 -> "none"
            BatteryManager.BATTERY_PLUGGED_AC -> "ac"
            BatteryManager.BATTERY_PLUGGED_USB -> "usb"
            BatteryManager.BATTERY_PLUGGED_WIRELESS -> "wireless"
            8 -> "dock" // BATTERY_PLUGGED_DOCK (Android 13)
            else -> null
        }
        val level = intent?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val scale = intent?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
        val raw = manager?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW) ?: Int.MIN_VALUE
        return BatteryReading(
            plugged = plugged,
            charging = status?.takeIf { it >= 0 }?.let { charging },
            levelPercent = if (level >= 0 && scale > 0) level * 100 / scale else null,
            currentUa = normalizeCurrentUa(raw, charging),
            voltageMv = intent?.getIntExtra(BatteryManager.EXTRA_VOLTAGE, -1)?.takeIf { it > 0 },
            temperatureC = intent?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, Int.MIN_VALUE)
                ?.takeIf { it != Int.MIN_VALUE }?.let { it / 10.0 },
        )
    }

    override fun askApproval(computer: String): Boolean {
        val future = CompletableFuture<Boolean>()
        approval = future
        _state.update { it.copy(approvalFor = computer) }
        val accepted = runCatching { future.get(60, TimeUnit.SECONDS) }.getOrDefault(false)
        approval = null
        _state.update { it.copy(approvalFor = null) }
        return accepted
    }

    override fun startWatch() {
        val w = newWatch()
        sampler?.cancel()
        sampler = viewModelScope.launch(Dispatchers.IO) {
            while (isActive && watch === w) {
                val b = battery()
                w.sample(b.currentUa, b.plugged)
                _state.update { it.copy(watchCuts = w.cuts) }
                delay(SAMPLE_MS)
            }
        }
    }

    override fun stopWatch(): WatchReport? {
        val w = watch ?: return null
        watch = null
        sampler?.cancel()
        _state.update { it.copy(watchCuts = null) }
        return w.report()
    }

    override fun onEvent(event: ProbeEvent) {
        _state.update {
            when (event) {
                is ProbeEvent.Connected -> it.copy(computer = event.computer, verdict = null, status = null)
                is ProbeEvent.Status -> it.copy(status = event.text, testing = true)
                is ProbeEvent.Measuring -> it.copy(
                    testing = true,
                    status = if (event.toComputer) "Envoi de données vers l'ordinateur…" else "Réception des données de l'ordinateur…",
                )
                is ProbeEvent.Verdict -> it.copy(verdict = event, testing = false, status = null)
                ProbeEvent.Disconnected -> {
                    watch = null
                    it.copy(computer = null, testing = false, status = null, approvalFor = null, watchCuts = null)
                }
            }
        }
    }

    // --- Système ------------------------------------------------------------

    /** Un ordinateur échange des données avec le téléphone par USB. */
    private fun usbConnected(): Boolean =
        sticky(USB_STATE)?.getBooleanExtra("connected", false) == true

    private fun sticky(action: String): Intent? = runCatching {
        ContextCompat.registerReceiver(context, null, IntentFilter(action), ContextCompat.RECEIVER_NOT_EXPORTED)
    }.getOrNull()
}

private const val USB_STATE = "android.hardware.usb.action.USB_STATE"
private const val HISTORY = "charge_history"
private const val HISTORY_SIZE = 20
private const val SAMPLE_MS = 250L
private const val CHARGE_SETTLE_MS = 5_000L
private const val CHARGE_MEASURE_MS = 25_000L
private const val REST_MS = 5_000L
private const val GET_READY_MS = 3_000L
private const val BEND_MS = 10_000L
