package io.github.toomuhtsh.treasureprobe

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.util.Locale

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // L'écran reste allumé pendant que l'application sert de sonde.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContent {
            TreasureTheme { ProbeScreen() }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProbeScreen(model: ProbeViewModel = viewModel()) {
    val state by model.state.collectAsStateWithLifecycle()
    val context = LocalContext.current

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Treasure Probe") },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surfaceContainer,
                ),
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            StatusCard(state, onOpenSettings = { model.openTetherSettings(context) })
            state.verdict?.let { VerdictCard(it) }
            state.battery?.let { BatteryCard(it, computer = state.usbConnected) }
            Text(
                "Treasure Probe sert de testeur au logiciel Treasure Cables : l'ordinateur fait passer des données " +
                    "par le câble dans les deux sens, et le téléphone rapporte la charge qu'il reçoit.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }

    state.approvalFor?.let { computer ->
        AlertDialog(
            onDismissRequest = { model.answer(false) },
            title = { Text("Tester le câble ?") },
            text = {
                Text(
                    "Treasure Cables sur « $computer » veut tester le câble : des données vont transiter " +
                        "dans les deux sens pendant une dizaine de secondes. Aucune donnée personnelle n'est échangée.",
                )
            },
            confirmButton = { Button(onClick = { model.answer(true) }) { Text("Accepter") } },
            dismissButton = { TextButton(onClick = { model.answer(false) }) { Text("Refuser") } },
        )
    }
}

@Composable
private fun StatusCard(state: UiState, onOpenSettings: () -> Unit) {
    val plugged = state.battery?.plugged.let { it != null && it != "none" }
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
    ) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            when {
                state.computer != null -> {
                    Title("Test en cours avec « ${state.computer} »")
                    Text(state.status ?: "En attente de l'ordinateur…")
                    if (state.testing) LinearProgressIndicator(Modifier.fillMaxWidth())
                }
                state.tetherAddress != null -> {
                    Title("Prêt à tester le câble")
                    Text("Sur l'ordinateur, lancez « Tester avec mon téléphone » dans Treasure Cables.")
                    Text(
                        "Partage de connexion USB actif · ${state.tetherAddress}",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                state.usbConnected -> {
                    Title("Activez le partage de connexion USB")
                    Text(
                        "Il fait passer le test par le câble. Il se trouve dans les réglages, rubrique " +
                            "« Partage de connexion » (ou dans la notification USB).",
                    )
                    Button(onClick = onOpenSettings) { Text("Ouvrir les réglages") }
                    Text(
                        "S'il est grisé ou absent, le câble ne transporte peut-être pas les données.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Text(
                        "Réseaux vus par l'application : " +
                            state.interfaces.ifEmpty { listOf("aucun") }.joinToString(", "),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                plugged -> {
                    Title("Le téléphone se charge, sans échange de données")
                    Text(
                        "Si le téléphone est branché sur un ordinateur, ce câble ne transporte pas les données : " +
                            "c'est un câble de charge seule, ou ses fils de données sont coupés. Sur un chargeur, " +
                            "c'est normal.",
                    )
                }
                else -> {
                    Title("Branchez le téléphone à l'ordinateur")
                    Text("Utilisez le câble à tester. Gardez cette application ouverte.")
                }
            }
        }
    }
}

@Composable
private fun VerdictCard(verdict: ProbeEvent.Verdict) {
    val colors = verdictColors(verdict.severity)
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = colors.container, contentColor = colors.onContainer),
    ) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Title(verdict.title)
            Text(verdict.detail)
        }
    }
}

@Composable
private fun BatteryCard(battery: BatteryReading, computer: Boolean) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Title("Charge")
            Fact(
                "Source",
                when (battery.plugged) {
                    "usb" -> "port USB d'ordinateur"
                    // Un port USB-C d'ordinateur qui annonce 1,5 ou 3 A est
                    // classé « chargeur » par Android.
                    "ac" -> if (computer) "port USB-C d'ordinateur (charge renforcée)" else "chargeur"
                    "wireless" -> "sans fil"
                    "dock" -> "station d'accueil"
                    "none" -> "non branché"
                    else -> "inconnue"
                },
            )
            battery.currentUa?.let { ua ->
                val label = if (ua >= 0) "Courant entrant" else "Courant sortant"
                Fact(label, amps(kotlin.math.abs(ua)))
            }
            battery.levelPercent?.let { Fact("Niveau", "$it %") }
            battery.temperatureC?.let { Fact("Température", String.format(Locale.FRANCE, "%.1f °C", it)) }
        }
    }
}

private fun amps(ua: Long): String =
    if (ua >= 1_000_000) String.format(Locale.FRANCE, "%.2f A", ua / 1_000_000.0) else "${ua / 1000} mA"

@Composable
private fun Title(text: String) {
    Text(text, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Medium)
}

@Composable
private fun Fact(label: String, value: String) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, color = Color.Unspecified)
    }
}
