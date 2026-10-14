// Génère src/theme.css : les rôles de couleur Material 3 (clair et sombre,
// schéma « fidelity » qui conserve la couleur de base)
// calculés à partir d'une couleur de base, avec l'outil officiel de Google.
// Usage : npm run theme
import { writeFileSync } from 'node:fs';
import {
  argbFromHex,
  hexFromArgb,
  Hct,
  MaterialDynamicColors,
  SchemeFidelity,
} from '@material/material-color-utilities';

// Bleu des prises USB 3.0 (Pantone 300 C, recommandé par l'USB-IF pour SuperSpeed).
const SEED = '#005eb8';

const roles = [
  'primary', 'onPrimary', 'primaryContainer', 'onPrimaryContainer',
  'secondary', 'onSecondary', 'secondaryContainer', 'onSecondaryContainer',
  'tertiary', 'onTertiary', 'tertiaryContainer', 'onTertiaryContainer',
  'error', 'onError', 'errorContainer', 'onErrorContainer',
  'background', 'onBackground', 'surface', 'onSurface', 'surfaceVariant', 'onSurfaceVariant',
  'surfaceDim', 'surfaceBright', 'surfaceContainerLowest', 'surfaceContainerLow',
  'surfaceContainer', 'surfaceContainerHigh', 'surfaceContainerHighest',
  'outline', 'outlineVariant', 'shadow', 'scrim',
  'inverseSurface', 'inverseOnSurface', 'inversePrimary',
];

const kebab = (s) => s.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase());

function block(dark) {
  const scheme = new SchemeFidelity(Hct.fromInt(argbFromHex(SEED)), dark, 0);
  return roles
    .map((role) => `  --md-sys-color-${kebab(role)}: ${hexFromArgb(MaterialDynamicColors[role].getArgb(scheme))};`)
    .join('\n');
}

// Clair par défaut ; sombre si le système le demande (choix « automatique »)
// ou si l'utilisateur l'a choisi (attribut data-theme posé par l'application).
const dark = block(true);
const css = `/* Généré par scripts/theme.mjs (couleur de base ${SEED}) : ne pas modifier à la main. */
:root {
  color-scheme: light;
${block(false)}
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme='light']) {
    color-scheme: dark;
${dark.replace(/^/gm, '  ')}
  }
}

:root[data-theme='dark'] {
  color-scheme: dark;
${dark}
}
`;
writeFileSync(new URL('../src/theme.css', import.meta.url), css);
console.log('src/theme.css généré');
