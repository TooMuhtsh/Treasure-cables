package io.github.toomuhtsh.treasureprobe

import android.app.Application
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build
import android.provider.Settings
import androidx.core.content.ContextCompat
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
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
)

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

    init {
        // Hors du fil de l'interface : ouverture des ports du serveur.
        viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                refresh()
                delay(1000)
            }
        }
    }

    private fun refresh() {
        val address = usbTetherAddress()
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
    }

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
                ProbeEvent.Disconnected -> it.copy(computer = null, testing = false, status = null, approvalFor = null)
            }
        }
    }

    // --- Système ------------------------------------------------------------

    /** Un ordinateur échange des données avec le téléphone par USB. */
    private fun usbConnected(): Boolean =
        sticky("android.hardware.usb.action.USB_STATE")?.getBooleanExtra("connected", false) == true

    private fun sticky(action: String): Intent? = runCatching {
        ContextCompat.registerReceiver(context, null, IntentFilter(action), ContextCompat.RECEIVER_NOT_EXPORTED)
    }.getOrNull()
}
