# Treasure Cables

**Ce que votre câble USB limite vraiment.**

Un même connecteur USB-C peut cacher un câble à 480 Mb/s ou à 80 Gb/s, limité
à 60 W ou capable de 240 W. Les marquages sont rares, parfois faux, et les
outils existants (USB Device Tree Viewer, lsusb…) affichent des descripteurs
bruts réservés aux experts.

Treasure Cables croise ce que le système sait sur vos ports, câbles et
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
├── treasure-model    modèle de données + format de rapport   ┐ sans dépendance au système,
├── treasure-audit    règles de diagnostic                    ┘ compilent en WebAssembly
├── treasure-collect  collecte : Linux (sysfs, typec), Windows (IOCTL des hubs)
└── treasure-cables   version portable (ligne de commande)
```

Le futur site web réutilisera `treasure-model` et `treasure-audit` compilés en
WebAssembly : il pourra lire un rapport produit par la version portable,
l'expliquer et le partager. Un navigateur ne peut pas inspecter lui-même l'arbre
USB ni les vitesses de lien (WebUSB ne le permet pas), d'où ce découpage.

## État

| Plateforme | Collecte |
|---|---|
| Linux | ✅ Arbre USB, vitesses négociées et voies, capacités (BOS, Linux 6.6+), ports compagnons USB 2/3, ports USB-C et e-marker des câbles (`/sys/class/typec`) |
| Windows | ✅ Testé sur Windows 11 : parcours des hubs sans droits administrateur (vitesse réelle, capacités SuperSpeed(Plus), voies, prises USB-C et leurs deux moitiés, BOS et modes alternatifs, consommation, codes de problème), écrans (liaison, mode natif). 🧪 À valider : lien DisplayPort réel via les pilotes Intel (IGCL) et NVIDIA (NVAPI), diagnostic approfondi (écoute ETW, administrateur). Voir [la recherche](docs/recherche-sources-windows.md). |
| Web | 🔜 Après la version portable |

## Utilisation

**Windows** : téléchargez `treasure-cables.exe` (artefact `treasure-cables-windows-x64`
de la CI GitHub Actions) et double-cliquez dessus. Le diagnostic s'affiche avec
un menu :

```
[R] Rafraîchir   [V] Vue détaillée   [D] Diagnostic approfondi   [E] Exporter (JSON)   [Q] Quitter
```

- **Vue par prise** (par défaut) : chaque prise physique (USB-C, USB-A) avec ce
  qu'elle sait faire et ce qui y est branché, hubs et docks compris ; les
  appareils internes à part.
- **Vue détaillée** (V) : arbre par contrôleur, avec pour chaque appareil
  vitesse et capacité (mesurée, déclarée ou déduite), version USB,
  consommation, voies, port et port jumeau, modes alternatifs.
- **Rafraîchir** (R ou F5) : relance l'analyse, pratique pour comparer deux câbles.
- **Diagnostic approfondi** (D, administrateur) : écoute la pile USB pendant
  que vous débranchez et rebranchez ; les erreurs de lien sont la preuve la
  plus directe d'un câble ou d'un connecteur défaillant.
- **Exporter** (E) : enregistre `rapport-treasure-cables-AAAAMMJJ-HHMMSS.json`
  (sans numéros de série) à côté de l'exécutable, à partager ou à ouvrir sur le site.

Pour les scripts, les options restent disponibles (`--json`, `--input`, `--details`, `--deep`, `--help`).

**Depuis les sources** :

```sh
cargo run --release -p treasure-cables                 # diagnostic de la machine
cargo run --release -p treasure-cables -- --json > rapport.json
cargo run --release -p treasure-cables -- --input rapport.json
```

## Documentation

- [Ce que Windows montre, et ce qu'on peut récupérer qu'il ne montre pas](docs/recherche-sources-windows.md)

## FAQ

**Pourquoi pas WSL sous Windows ?** WSL2 est une machine virtuelle : elle ne voit
pas le matériel USB réel. Le pont usbipd-win retire l'appareil à Windows et le
présente sur un contrôleur virtuel, ce qui fausse précisément la vitesse de lien
et la topologie qu'on veut mesurer. Le code Linux est réutilisé autrement : le
modèle, l'audit et le format de rapport sont communs.

**Un logiciel peut-il tester un câble seul ?** Non. Il voit ce qui a été négocié
avec l'appareil au bout du câble. Pour certifier un câble isolé (continuité des
fils, e-marker sous Windows), il faut un testeur matériel.
