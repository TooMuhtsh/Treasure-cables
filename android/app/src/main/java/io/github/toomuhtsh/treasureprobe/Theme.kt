package io.github.toomuhtsh.treasureprobe

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// Couleurs Material 3 du logiciel Treasure Cables (couleur de base : le bleu
// des prises USB 3.0, #005EB8), reprises de web/src/theme.css.

val Light = lightColorScheme(
    primary = Color(0xFF00478D),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFF005EB8),
    onPrimaryContainer = Color(0xFFC8DAFF),
    inversePrimary = Color(0xFFA9C7FF),
    secondary = Color(0xFF4A5F83),
    onSecondary = Color(0xFFFFFFFF),
    secondaryContainer = Color(0xFFC0D5FF),
    onSecondaryContainer = Color(0xFF475C80),
    tertiary = Color(0xFF793100),
    onTertiary = Color(0xFFFFFFFF),
    tertiaryContainer = Color(0xFF9F4300),
    onTertiaryContainer = Color(0xFFFFCFB9),
    background = Color(0xFFF9F9FF),
    onBackground = Color(0xFF191C21),
    surface = Color(0xFFF9F9FF),
    onSurface = Color(0xFF191C21),
    surfaceVariant = Color(0xFFDEE2F0),
    onSurfaceVariant = Color(0xFF424752),
    inverseSurface = Color(0xFF2E3037),
    inverseOnSurface = Color(0xFFEFF0F8),
    error = Color(0xFFBA1A1A),
    onError = Color(0xFFFFFFFF),
    errorContainer = Color(0xFFFFDAD6),
    onErrorContainer = Color(0xFF93000A),
    outline = Color(0xFF727783),
    outlineVariant = Color(0xFFC2C6D4),
    scrim = Color(0xFF000000),
    surfaceBright = Color(0xFFF9F9FF),
    surfaceContainer = Color(0xFFECEDF6),
    surfaceContainerHigh = Color(0xFFE7E8F0),
    surfaceContainerHighest = Color(0xFFE1E2EA),
    surfaceContainerLow = Color(0xFFF2F3FB),
    surfaceContainerLowest = Color(0xFFFFFFFF),
    surfaceDim = Color(0xFFD8DAE2),
    surfaceTint = Color(0xFF00478D),
)

val Dark = darkColorScheme(
    primary = Color(0xFFA9C7FF),
    onPrimary = Color(0xFF003063),
    primaryContainer = Color(0xFF005EB8),
    onPrimaryContainer = Color(0xFFC8DAFF),
    inversePrimary = Color(0xFF005DB6),
    secondary = Color(0xFFB2C7F0),
    onSecondary = Color(0xFF1B3152),
    secondaryContainer = Color(0xFF32476A),
    onSecondaryContainer = Color(0xFFA1B6DE),
    tertiary = Color(0xFFFFB691),
    onTertiary = Color(0xFF552000),
    tertiaryContainer = Color(0xFF9F4300),
    onTertiaryContainer = Color(0xFFFFCFB9),
    background = Color(0xFF111319),
    onBackground = Color(0xFFE1E2EA),
    surface = Color(0xFF111319),
    onSurface = Color(0xFFE1E2EA),
    surfaceVariant = Color(0xFF424752),
    onSurfaceVariant = Color(0xFFC2C6D4),
    inverseSurface = Color(0xFFE1E2EA),
    inverseOnSurface = Color(0xFF2E3037),
    error = Color(0xFFFFB4AB),
    onError = Color(0xFF690005),
    errorContainer = Color(0xFF93000A),
    onErrorContainer = Color(0xFFFFDAD6),
    outline = Color(0xFF8C919D),
    outlineVariant = Color(0xFF424752),
    scrim = Color(0xFF000000),
    surfaceBright = Color(0xFF37393F),
    surfaceContainer = Color(0xFF1D2025),
    surfaceContainerHigh = Color(0xFF272A30),
    surfaceContainerHighest = Color(0xFF32353B),
    surfaceContainerLow = Color(0xFF191C21),
    surfaceContainerLowest = Color(0xFF0B0E14),
    surfaceDim = Color(0xFF111319),
    surfaceTint = Color(0xFFA9C7FF),
)

/** Couleurs de verdict (Material ne définit que l'erreur), comme le logiciel. */
data class VerdictColors(val container: Color, val onContainer: Color)

@Composable
fun verdictColors(severity: String): VerdictColors {
    val dark = isSystemInDarkTheme()
    val scheme = MaterialTheme.colorScheme
    return when (severity) {
        "ok" -> if (dark) VerdictColors(Color(0xFF00531F), Color(0xFFA6F5B4)) else VerdictColors(Color(0xFFC4EFCB), Color(0xFF002109))
        "warning" -> if (dark) VerdictColors(Color(0xFF693C00), Color(0xFFFFDCBE)) else VerdictColors(Color(0xFFFFDCBE), Color(0xFF2C1600))
        "problem" -> VerdictColors(scheme.errorContainer, scheme.onErrorContainer)
        else -> VerdictColors(scheme.secondaryContainer, scheme.onSecondaryContainer)
    }
}

@Composable
fun TreasureTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, content = content)
}
