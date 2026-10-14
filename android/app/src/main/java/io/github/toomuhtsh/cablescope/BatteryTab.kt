package io.github.toomuhtsh.cablescope

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.util.Locale

/** Onglet Batterie : le branchement en direct, puis l'état de la batterie. */
@Composable
fun BatteryTab(state: UiState) {
    state.battery?.let { BatteryCard(it, computer = state.usbConnected) }
    val info = state.batteryInfo
    val battery = state.battery
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Title("État de la batterie")
            info?.status?.let { Fact("État", it) }
            info?.health?.let { Fact("Santé", it) }
            info?.cycleCount?.let { Fact("Cycles de charge", "$it") }
            info?.chargeCounterUah?.let { Fact("Charge restante", "${it / 1000} mAh") }
            battery?.voltageMv?.let { Fact("Tension", String.format(Locale.FRANCE, "%.2f V", it / 1000.0)) }
            info?.technology?.let { Fact("Technologie", it) }
            Text(
                "Données fournies par Android ; certains téléphones n'en communiquent qu'une partie (le nombre de " +
                    "cycles, à partir d'Android 14).",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
