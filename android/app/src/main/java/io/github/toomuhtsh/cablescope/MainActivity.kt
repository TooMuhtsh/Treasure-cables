package io.github.toomuhtsh.cablescope

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Surface
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.res.painterResource
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
            CableScopeTheme { ProbeScreen() }
        }
    }
}

/** Le logo de l'application, tel qu'il apparaît sur l'écran d'accueil. */
@Composable
fun Logo(modifier: Modifier = Modifier) {
    Box(
        modifier
            .size(36.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(Color(0xFF005EB8)),
    ) {
        // Le dessin de l'icône adaptative laisse une marge pour les masques
        // ronds du lanceur : on l'agrandit pour remplir la vignette.
        Image(
            painter = painterResource(R.drawable.ic_launcher_foreground),
            contentDescription = null,
            modifier = Modifier.fillMaxSize().scale(1.4f),
        )
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
                title = { Text("CableScope Probe") },
                navigationIcon = { Logo(Modifier.padding(start = 12.dp, end = 4.dp)) },
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
            if (state.computer == null) {
                state.localTest?.let { LocalTestCard(it, onCancel = model::cancelLocal) }
                    ?: LocalTestsCard(state, onCharge = model::startCharge, onContacts = model::startContacts)
                state.localVerdict?.let { VerdictCard(it, onClose = model::dismissLocal) }
            }
            if (state.chargeHistory.isNotEmpty()) {
                ChargeHistoryCard(state.chargeHistory, onClear = model::clearChargeHistory)
            }
            state.battery?.let { BatteryCard(it, computer = state.usbConnected) }
            Text(
                "Avec le logiciel CableScope, l'ordinateur fait passer des données par le câble dans les deux sens " +
                    "et le téléphone rapporte la charge qu'il reçoit. Sans ordinateur, le téléphone mesure seul la " +
                    "charge et repère les faux contacts.",
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
                    "CableScope sur « $computer » veut tester le câble : des données vont transiter " +
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
                    Text(
                        state.status ?: "En attente de l'ordinateur…",
                        style = if (state.watchCuts != null) MaterialTheme.typography.titleLarge else MaterialTheme.typography.bodyLarge,
                    )
                    state.watchCuts?.let { CutsLine(it) }
                    if (state.testing) LinearProgressIndicator(Modifier.fillMaxWidth())
                }
                state.tetherAddress != null -> {
                    Title("Prêt à tester le câble")
                    Text("Sur l'ordinateur, lancez « Tester avec mon téléphone » dans CableScope.")
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

/** Coupures relevées, en direct, pendant que l'utilisateur plie le câble. */
@Composable
private fun CutsLine(cuts: Int) {
    Text(
        if (cuts == 0) "Aucune coupure pour l'instant" else "$cuts coupure${if (cuts > 1) "s" else ""} relevée${if (cuts > 1) "s" else ""}",
        color = if (cuts == 0) Color.Unspecified else MaterialTheme.colorScheme.error,
        fontWeight = FontWeight.Medium,
    )
}

@Composable
private fun LocalTestsCard(state: UiState, onCharge: () -> Unit, onContacts: () -> Unit) {
    val plugged = state.battery?.plugged.let { it != null && it != "none" }
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Title("Tester sans ordinateur")
            Text(
                if (plugged) {
                    "Le téléphone mesure seul ce que le câble laisse passer, branché sur un chargeur ou un ordinateur."
                } else {
                    "Branchez le téléphone sur un chargeur (ou un ordinateur) avec le câble à tester."
                },
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilledTonalButton(onClick = onCharge, enabled = plugged) { Text("Mesurer la charge") }
                OutlinedButton(onClick = onContacts, enabled = plugged) { Text("Faux contacts") }
            }
            Text(
                "Charge : 30 s, à comparer entre câbles avec le même chargeur. Faux contacts : 30 s, vous pliez le " +
                    "câble près de chaque fiche en suivant les consignes ; un bip signale chaque coupure.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            state.localError?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }
}

@Composable
private fun LocalTestCard(test: LocalTest, onCancel: () -> Unit) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer),
    ) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(
                if (test.kind == LocalKind.Charge) "Mesure de la charge" else "Test des faux contacts",
                style = MaterialTheme.typography.labelLarge,
            )
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                Text(test.step, modifier = Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
                if (test.secondsLeft > 0) {
                    Text("${test.secondsLeft} s", style = MaterialTheme.typography.headlineMedium)
                }
            }
            LinearProgressIndicator(progress = { test.progress }, modifier = Modifier.fillMaxWidth())
            test.currentUa?.let { ua ->
                Fact(if (ua >= 0) "Courant entrant" else "Courant sortant", formatAmps(kotlin.math.abs(ua)))
            }
            if (test.kind == LocalKind.Contacts) CutsLine(test.cuts)
            TextButton(onClick = onCancel, contentPadding = PaddingValues(0.dp)) { Text("Annuler") }
        }
    }
}

@Composable
private fun ChargeHistoryCard(history: List<ChargeResult>, onClear: () -> Unit) {
    val best = history.maxOf { it.medianUa }
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Title("Charges mesurées")
            history.withIndex().reversed().forEach { (i, r) ->
                val details = listOfNotNull(
                    r.levelPercent?.let { "batterie $it %" },
                    sourceLabel(r.plugged, false).takeIf { r.plugged != null },
                ).joinToString(" · ")
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Column(Modifier.weight(1f)) {
                        Text("Mesure ${i + 1}${if (r.medianUa == best) " · la meilleure" else ""}")
                        if (details.isNotEmpty()) {
                            Text(details, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                    Text(formatAmps(r.medianUa), fontWeight = FontWeight.Medium)
                }
            }
            TextButton(onClick = onClear, contentPadding = PaddingValues(0.dp)) { Text("Effacer") }
        }
    }
}

@Composable
private fun VerdictCard(verdict: ProbeEvent.Verdict, onClose: (() -> Unit)? = null) {
    var details by rememberSaveable(verdict) { mutableStateOf(false) }
    val title = when (verdict.severity) {
        "ok" -> MaterialTheme.colorScheme.primary
        "warning" -> verdictColors("warning").onContainer
        "problem" -> MaterialTheme.colorScheme.error
        else -> MaterialTheme.colorScheme.onSurface
    }
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(verdict.title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Medium, color = title)
            if (verdict.scores.isEmpty()) {
                Text(verdict.detail)
                return@Column
            }
            verdict.scores.forEach { ScoreRow(it) }
            verdict.advice?.let {
                Text(it, style = MaterialTheme.typography.bodyMedium)
            }
            TextButton(onClick = { details = !details }, contentPadding = PaddingValues(0.dp)) {
                Text(if (details) "Masquer les détails" else "Détails")
            }
            if (details) {
                verdict.facts.forEach { (label, value) -> Fact(label, value) }
            }
            onClose?.let { TextButton(onClick = it, contentPadding = PaddingValues(0.dp)) { Text("Fermer") } }
        }
    }
}

@Composable
private fun ScoreRow(score: VerdictScore) {
    val colors = toneColors(score.tone)
    val criterion = when (score.criterion) {
        "transfer" -> "Transfert"
        "charge" -> "Charge"
        "contacts" -> "Contacts"
        "overall" -> "Général"
        else -> score.criterion
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            criterion,
            modifier = Modifier.width(80.dp),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Surface(shape = RoundedCornerShape(50), color = colors.container, contentColor = colors.onContainer) {
            Text(
                score.rating,
                modifier = Modifier.padding(horizontal = 10.dp, vertical = 3.dp),
                style = MaterialTheme.typography.labelLarge,
            )
        }
        score.value?.let { Text(it, style = MaterialTheme.typography.bodyMedium) }
    }
}

@Composable
private fun BatteryCard(battery: BatteryReading, computer: Boolean) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Title("Charge")
            Fact("Source", sourceLabel(battery.plugged, computer))
            battery.currentUa?.let { ua ->
                val label = if (ua >= 0) "Courant entrant" else "Courant sortant"
                Fact(label, formatAmps(kotlin.math.abs(ua)))
            }
            battery.levelPercent?.let { Fact("Niveau", "$it %") }
            battery.temperatureC?.let { Fact("Température", String.format(Locale.FRANCE, "%.1f °C", it)) }
        }
    }
}

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
