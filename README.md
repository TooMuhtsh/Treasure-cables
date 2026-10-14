<p align="center"><img src="docs/logo/cablescope.svg" width="112" alt="Logo CableScope : un stéthoscope terminé par une fiche RJ45"></p>

# CableScope

**Ce que votre câble USB limite vraiment.**

Un même connecteur USB-C peut cacher un câble à 480 Mb/s ou à 80 Gb/s, limité
à 60 W ou capable de 240 W. Les marquages sont rares, parfois faux, et les
outils existants (USB Device Tree Viewer, lsusb…) affichent des descripteurs
bruts réservés aux experts.

CableScope croise ce que le système sait sur vos ports, câbles et
appareils, et le traduit en diagnostic clair :

```
⚠ [1-2] SSD NVMe fonctionne en USB 2.0 alors qu'il supporte USB 3.2 Gen 2 (10 Gb/s) (probable)
    Vitesse actuelle : USB 2.0 High Speed (480 Mb/s). Le port et l'appareil savent faire mieux :
    le câble est très probablement un câble USB 2.0 (ou ses fils USB 3 sont abîmés).
```

## Principes

- **Honnête** : chaque information est *mesurée* (état réel du lien), *déclarée*
  (annoncée par l'appareil, le port ou l'e-marker du câble) ou *déduite*. Chaque
  constat indique son degré de certitude.
- **Sans installation** : un exécutable portable, utilisable hors ligne, sans
  droits administrateur pour l'essentiel.
- **Partageable** : un rapport JSON versionné, sans numéros de série par défaut,
  lisible par le futur site web.
- **Pour tout le monde** : un verdict simple d'abord, l'arbre détaillé ensuite.

## Architecture

```
crates/
├── cablescope-model    modèle de données + format de rapport   ┐
├── cablescope-audit    règles de diagnostic, test de câble     │ sans dépendance au système,
├── cablescope-present  présentation pour les interfaces        ┘ compilent en WebAssembly
├── cablescope-collect  collecte : Linux (sysfs, typec), Windows (IOCTL des hubs)
└── cablescope   version portable : fenêtre (serveur local), console, ligne de commande
web/                  interface Svelte 5 + Material Web (fenêtre de l'application et futur site)
android/              CableScope Probe : application Android qui sert de sonde (Kotlin, Compose)
```

Le futur site web réutilisera la même interface (`web/`) et `cablescope-present` compilé en
WebAssembly : il pourra lire un rapport produit par la version portable,
l'expliquer et le partager. Un navigateur ne peut pas inspecter lui-même l'arbre
USB ni les vitesses de lien (WebUSB ne le permet pas), d'où ce découpage.

## État

| Plateforme | Collecte |
|---|---|
| Linux | ✅ Arbre USB, vitesses négociées et voies, capacités (BOS, Linux 6.6+), ports compagnons USB 2/3, ports USB-C et e-marker des câbles (`/sys/class/typec`) |
| Windows | ✅ Testé sur Windows 11 (portable Intel + RTX 3060) : parcours des hubs sans droits administrateur (vitesse réelle, capacités SuperSpeed(Plus), voies, prises USB-C et leurs deux moitiés, BOS et modes alternatifs, consommation, codes de problème), écrans (liaison, mode natif, lien DisplayPort réel via NVIDIA NVAPI), diagnostic approfondi (écoute ETW, administrateur). 🧪 À valider : lien DisplayPort via Intel IGCL, USB4. Voir [la recherche](docs/recherche-sources-windows.md). |
| Web | 🔜 Après la version portable |

## Utilisation

**Windows** : téléchargez `cablescope.exe` (artefact `cablescope-windows-x64`
de la CI GitHub Actions) et double-cliquez dessus. L'interface s'ouvre dans une
fenêtre (Edge en mode application, Material Design 3, thème clair ou sombre) :

- **Tester un câble** : débranchez l'appareil, branchez-le avec le câble à
  tester ; verdict immédiat (charge seule, limité à l'USB 2.0, défaillant,
  correct) et comparaison des câbles testés.
- **Tester avec mon téléphone** : avec l'application Android **CableScope Probe**, le
  téléphone devient testeur. Le logiciel et l'application dialoguent à travers le
  câble (partage de connexion USB) : vitesse négociée, débit réel dans les deux
  sens, coupures, erreurs de lien et charge reçue par le téléphone. En option, le
  **test des faux contacts** : vous pliez le câble près de chaque fiche, le PC relève
  les chutes de débit et le téléphone les coupures de charge. L'application signale
  aussi d'elle-même un câble qui charge sans transporter de données.
- **Diagnostic approfondi** (administrateur) : écoute la pile USB pendant que
  vous débranchez et rebranchez ; les erreurs de lien sont la preuve la plus
  directe d'un câble ou d'un connecteur défaillant.
- Onglets **Prises** (chaque prise physique, sa façade et ses appareils),
  **Écrans** (liaison, mode natif, part du lien DisplayPort utilisée),
  **Diagnostic** et **Détails** ; rafraîchir et exporter le rapport JSON.
- Mise à jour automatique : brancher ou débrancher un appareil USB ou un
  écran relance l'analyse et l'annonce (« Branché : … »), sans bouton.
- Thème clair, sombre ou automatique (suit Windows), mémorisé d'un lancement
  à l'autre (`%APPDATA%\CableScope\preferences.json`).

L'interface est servie par l'exécutable lui-même, uniquement sur 127.0.0.1 et
protégée par un jeton aléatoire ; l'outil s'arrête quand la fenêtre est fermée.
Sous Windows, l'exécutable est une application fenêtrée : un double-clic
n'ouvre aucune console. `--tui` ouvre à la place une interface plein écran
dans sa propre console.

Pour les scripts, les options restent disponibles (`--json`, `--input`, `--details`,
`--deep`, `--tui`, `--gui`, `--no-window`, `--help`). Lancées depuis un
terminal, elles y écrivent leur résultat ; sous PowerShell, la redirection
fonctionne comme d'habitude : `.\cablescope.exe --json > rapport.json`.

**Depuis les sources** (l'interface web se compile d'abord, elle est ensuite
embarquée dans l'exécutable) :

```sh
(cd web && npm ci && npm run build)
cargo run --release -p cablescope                 # diagnostic de la machine
cargo run --release -p cablescope -- --json > rapport.json
cargo run --release -p cablescope -- --input rapport.json
```

## Documentation

- [Ce que Windows montre, et ce qu'on peut récupérer qu'il ne montre pas](docs/recherche-sources-windows.md)
- [Feuille de route](docs/feuille-de-route.md) : diagnostic Ethernet et Wi-Fi, publication
  (versions GitHub, Play Store, F-Droid)
- [Protocole de sonde](docs/protocole-sonde.md) : comment le logiciel et une sonde (l'application
  Android, ou tout autre appareil) testent un câble ensemble

## CableScope Probe (Android)

L'APK est produit par la CI (artefact `cablescope-probe-apk`) ; Android 8 ou plus récent.

Sans ordinateur, l'application teste aussi seule, branchée sur un chargeur :

- **Mesurer la charge** (30 s) : courant reçu, puissance estimée, note Rapide /
  Normale / Lente, et comparaison avec les câbles déjà mesurés. Les pièges sont
  signalés (batterie au-delà de 80 %, téléphone chaud, prise USB simple).
- **Faux contacts** (30 s) : câble au repos, puis plié près de chaque fiche ; un bip
  signale chaque coupure, le verdict dit de quel côté le câble lâche.

Pour le compiler :

```sh
cd android
./gradlew :app:testDebugUnitTest :app:assembleDebug   # SDK Android requis (ANDROID_HOME)
```

Sans téléphone, `CABLESCOPE_PROBE_SERVE=60 ./gradlew :app:testDebugUnitTest --rerun` sert une sonde
de développement sur 127.0.0.1 ; lancez alors le logiciel avec `CABLESCOPE_PROBE=127.0.0.1`.

## FAQ

**Pourquoi pas WSL sous Windows ?** WSL2 est une machine virtuelle : elle ne voit
pas le matériel USB réel. Le pont usbipd-win retire l'appareil à Windows et le
présente sur un contrôleur virtuel, ce qui fausse précisément la vitesse de lien
et la topologie qu'on veut mesurer. Le code Linux est réutilisé autrement : le
modèle, l'audit et le format de rapport sont communs.

**Un logiciel peut-il tester un câble seul ?** Non. Il voit ce qui a été négocié
avec l'appareil au bout du câble. Pour certifier un câble isolé (continuité des
fils, e-marker sous Windows), il faut un testeur matériel.

## Licence

Au choix, sous licence [MIT](LICENSE-MIT) ou [Apache 2.0](LICENSE-APACHE),
comme la plupart des projets Rust. Sauf mention contraire, toute contribution
proposée au projet est placée sous ces deux mêmes licences.
