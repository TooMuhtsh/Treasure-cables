package io.github.toomuhtsh.cablescope

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Onglet Mesures : classement des câbles par chargeur, puis toutes les mesures. */
@Composable
fun MeasuresTab(history: List<Measurement>, model: ProbeViewModel) {
    var selected by rememberSaveable { mutableStateOf<Long?>(null) }
    var confirmClear by rememberSaveable { mutableStateOf(false) }

    if (history.isEmpty()) {
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Title("Aucune mesure pour l'instant")
                Text(
                    "Chaque test est gardé ici, avec son détail. Nommez le chargeur et le câble dans l'onglet " +
                        "Tester : les câbles mesurés sur un même chargeur sont alors classés.",
                )
            }
        }
        return
    }

    cableRanking(history).forEach { (charger, cables) -> RankingCard(charger, cables) }

    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(vertical = 8.dp)) {
            Text(
                "Toutes les mesures",
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Medium,
            )
            history.asReversed().forEachIndexed { i, m ->
                if (i > 0) HorizontalDivider(Modifier.padding(horizontal = 16.dp))
                MeasurementRow(m, onClick = { selected = m.timeMs })
            }
            TextButton(onClick = { confirmClear = true }, modifier = Modifier.padding(horizontal = 8.dp)) {
                Text("Tout effacer")
            }
        }
    }

    history.firstOrNull { it.timeMs == selected }?.let { m ->
        MeasurementSheet(
            m,
            chargers = usedNames(history.map { it.charger }),
            cables = usedNames(history.map { it.cable }),
            onRename = { charger, cable -> model.rename(m.timeMs, charger, cable) },
            onDelete = {
                model.delete(m.timeMs)
                selected = null
            },
            onDismiss = { selected = null },
        )
    }

    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text("Effacer toutes les mesures ?") },
            text = { Text("Les ${history.size} mesures et leur détail seront supprimés de ce téléphone.") },
            confirmButton = {
                Button(onClick = {
                    model.clearHistory()
                    confirmClear = false
                }) { Text("Effacer") }
            },
            dismissButton = { TextButton(onClick = { confirmClear = false }) { Text("Annuler") } },
        )
    }
}

/** Les câbles d'un même chargeur, du plus rapide au plus lent. */
@Composable
private fun RankingCard(charger: String, cables: List<Pair<String, Long>>) {
    val best = cables.first().second
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Title("Câbles sur « $charger »")
            cables.forEach { (cable, ua) ->
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text(cable)
                        Text(formatAmps(ua), fontWeight = FontWeight.Medium)
                    }
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .height(8.dp)
                            .clip(RoundedCornerShape(4.dp))
                            .background(MaterialTheme.colorScheme.surfaceContainerHighest),
                    ) {
                        Box(
                            Modifier
                                .fillMaxWidth(ua.toFloat() / best)
                                .fillMaxHeight()
                                .clip(RoundedCornerShape(4.dp))
                                .background(MaterialTheme.colorScheme.primary),
                        )
                    }
                }
            }
            Text(
                "Meilleure charge de chaque câble sur ce chargeur. Comparez à niveau de batterie proche (sous 80 %).",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun MeasurementRow(m: Measurement, onClick: () -> Unit) {
    ListItem(
        modifier = Modifier.clickable(onClick = onClick),
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        headlineContent = { Text(m.title) },
        supportingContent = {
            Text(
                listOfNotNull(
                    m.kind.label.takeIf { m.title != m.kind.label },
                    formatDate(m.timeMs),
                    m.levelPercent?.let { "batterie $it %" },
                ).joinToString(" · "),
            )
        },
        trailingContent = {
            Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                m.mainScore?.let { Pill(it) }
                m.medianUa?.let { Text(formatAmps(it), style = MaterialTheme.typography.labelMedium) }
            }
        },
    )
}

@Composable
private fun Pill(score: VerdictScore) {
    val colors = toneColors(score.tone)
    Surface(shape = RoundedCornerShape(50), color = colors.container, contentColor = colors.onContainer) {
        Text(
            score.rating,
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 3.dp),
            style = MaterialTheme.typography.labelLarge,
        )
    }
}

/** Fiche d'une mesure : son verdict complet, ses noms modifiables. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MeasurementSheet(
    m: Measurement,
    chargers: List<String>,
    cables: List<String>,
    onRename: (String, String) -> Unit,
    onDelete: () -> Unit,
    onDismiss: () -> Unit,
) {
    var charger by rememberSaveable(m.timeMs) { mutableStateOf(m.charger.orEmpty()) }
    var cable by rememberSaveable(m.timeMs) { mutableStateOf(m.cable.orEmpty()) }
    val changed = charger.trim() != m.charger.orEmpty() || cable.trim() != m.cable.orEmpty()
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 32.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(m.title, style = MaterialTheme.typography.titleLarge)
            Text(
                listOfNotNull(m.kind.label, formatDate(m.timeMs), m.computer?.let { "avec « $it »" }).joinToString(" · "),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            VerdictCard(m.verdict, expanded = true)
            if (m.kind != MeasurementKind.Computer) {
                NameField("Chargeur ou source", "chargeur chambre, batterie portable…", charger, chargers) { charger = it }
            }
            NameField("Câble", "noir 2 m, câble du bureau…", cable, cables) { cable = it }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilledTonalButton(onClick = { onRename(charger, cable) }, enabled = changed) { Text("Enregistrer les noms") }
                OutlinedButton(onClick = onDelete) { Text("Supprimer") }
            }
        }
    }
}

private fun formatDate(ms: Long): String = SimpleDateFormat("d MMM yyyy, HH:mm", Locale.FRANCE).format(Date(ms))
