# Feuille de route

Ce qui est prévu pour les prochaines versions, par ordre de priorité.

## Diagnostic réseau

- **Ethernet** : vitesse négociée comparée à la capacité de la carte (une carte
  1 Gb/s connectée à 100 Mb/s trahit un câble abîmé ou à 4 fils), duplex,
  compteurs d'erreurs ; notes Transfert / Général comme pour l'USB.
- **Wi-Fi** : norme et bande obtenues comparées aux capacités de la carte
  (Wi-Fi 6 capable mais connecté en Wi-Fi 4 sur 2,4 GHz), signal, canal,
  largeur, débits négociés ; débit réel mesurable avec le téléphone (même
  protocole de sonde).

## Publication

### Versions publiées (GitHub Releases)

- Une étiquette (`v0.2.0`…) produit l'exécutable Windows, l'APK signé et
  l'AAB, joints à la version.
- Clé de signature de l'application conservée dans les secrets du dépôt.

### CableScope Probe sur le Play Store

- Compte développeur Google Play (25 $, vérification d'identité).
- Test fermé obligatoire pour un compte personnel : 12 testeurs inscrits
  pendant 14 jours d'affilée avant la publication.
- À préparer : AAB signé, fiche (description, captures, bannière ; l'icône
  512 px est prête : `docs/logo/cablescope-play-store-512.png`), questionnaire de classification, formulaire « Sécurité des
  données » (aucune donnée collectée), page de politique de confidentialité.
- Version Android visée : à relever chaque année (API 36 exigée depuis le
  31 août 2026).

### CableScope Probe sur F-Droid

- Métadonnées (fastlane) et compilation depuis les sources ; aucune
  bibliothèque propriétaire.

### Dans le logiciel

- **QR code vers la fiche Play Store** dans l'assistant « Tester avec mon
  téléphone » (`https://play.google.com/store/apps/details?id=io.github.toomuhtsh.cablescope`),
  à activer dès la publication ; lien F-Droid et APK en second choix.
- Généré hors ligne (bibliothèque embarquée), comme le reste de l'interface.

## Plus tard

- Site web qui lit et explique un rapport JSON (même interface, présentation
  compilée en WebAssembly).
- Validation sur matériel : lien DisplayPort via Intel IGCL, USB4.
