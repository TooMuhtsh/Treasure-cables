# Protocole de sonde CableScope (version 1)

Une **sonde** est un appareil branché au bout du câble à tester, qui dialogue
avec le logiciel **à travers ce câble** : le logiciel mesure ce que le câble
laisse réellement passer (débit dans les deux sens, coupures), et la sonde
rapporte ce qu'elle voit de son côté (charge reçue). La première sonde est
l'application Android *CableScope Probe* ; n'importe quel appareil capable de
parler ce protocole peut en être une (autre application, montage à
microcontrôleur…).

Ce document est la référence : le logiciel (`crates/cablescope/src/probe.rs`)
et l'application (`android/`) l'implémentent.

## Transport

- **Liaison** : le téléphone active le *partage de connexion USB* (RNDIS ou
  NCM). Un réseau IP point à point s'établit **par le câble testé** ; aucun
  pilote à installer, aucune option développeur.
- **Adresses** : le téléphone est la passerelle (le routeur DHCP) de ce
  réseau. Le logiciel cherche la carte réseau du partage USB et se connecte
  à l'adresse de sa passerelle.
- **Ports TCP** de la sonde :
  - `47820` : contrôle, messages JSON ;
  - `47821` : mesures de débit, octets bruts.
- La sonde n'écoute **que** sur l'adresse de l'interface du partage USB, jamais
  sur le Wi-Fi ou les données mobiles.
- C'est le logiciel qui ouvre les connexions (une connexion sortante ne
  déclenche pas le pare-feu de Windows).

## Messages de contrôle

Une ligne de JSON par message (UTF-8, terminée par `\n`). Chaque message a un
champ `type`. Un champ inconnu est ignoré ; un `type` inconnu reçoit une
erreur `unknown_type` sans couper la session.

### 1. Présentation

Logiciel → sonde :

```json
{"type":"hello","protocol":1,"software":"CableScope","version":"0.1.0","computer":"PORTABLE-ALEX"}
```

Sonde → logiciel :

```json
{"type":"hello","protocol":1,"probe":"CableScope Probe","version":"0.1.0",
 "device":{"manufacturer":"samsung","model":"SM-S911B","android":"16"},
 "capabilities":["battery","throughput","watch"]}
```

`manufacturer` et `model` sont ceux que l'appareil annonce aussi en USB
(chaînes du descripteur) : le logiciel s'en sert pour retrouver la sonde dans
l'arbre USB et lire la vitesse négociée. Si `protocol` diffère, la sonde
répond `{"type":"error","code":"protocol",…}` et ferme.

### 2. Accord de l'utilisateur

La sonde demande l'accord sur son écran (« CableScope sur
PORTABLE-ALEX veut tester le câble »), puis :

```json
{"type":"accepted","session":"3f9c…"}
```

ou `{"type":"refused"}` suivi de la fermeture. `session` est un jeton
aléatoire (au moins 128 bits, en hexadécimal) exigé sur le port de débit.
Le logiciel attend l'accord au plus 60 s.

### 3. Charge

Logiciel → sonde : `{"type":"battery"}`

Sonde → logiciel :

```json
{"type":"battery","plugged":"usb","charging":true,"level_percent":64,
 "current_ua":1250000,"voltage_mv":4120,"temperature_c":31.5}
```

- `plugged` : `usb` (port d'ordinateur), `ac` (chargeur), `wireless`,
  `dock` ou `none`.
- `current_ua` : courant **entrant dans la batterie**, en microampères,
  positif en charge, `null` si inconnu. La sonde normalise l'unité et le signe
  (certains appareils rapportent des milliampères ou un signe inversé).
- `voltage_mv` : tension de la batterie (pas celle du câble).
- Chaque champ peut valoir `null`.

### 4. Messages d'état

Le logiciel tient la sonde au courant, pour l'affichage :

```json
{"type":"status","text":"Mesure du débit vers l'ordinateur…"}
```

### 5. Surveillance des coupures (capacité `watch`)

Pour le test des faux contacts, l'utilisateur plie le câble près de chaque
fiche pendant que le logiciel fait passer des données (port de débit, sens
`to_computer`) et relève les chutes de débit par tranches de 250 ms. La
sonde, elle, surveille ce que seul le téléphone voit :

```json
{"type":"watch","action":"start"}
```

puis, à la fin de la zone pliée :

```json
{"type":"watch","action":"stop"}
```

La sonde répond :

```json
{"type":"watch_report","power_cuts":0,"data_cuts":0,"charge_drops":1,"median_current_ua":1480000}
```

- `power_cuts` : coupures de l'alimentation (débranchement vu par le
  système) ;
- `data_cuts` : coupures de la liaison de données USB ;
- `charge_drops` : baisses passagères de la charge (courant tombé sous 20 %
  de son niveau habituel, ou source renégociée) ; comptées seulement si la
  batterie se charge franchement (au moins 300 mA) ;
- chaque champ vaut `null` si la surveillance n'avait pas commencé.

Les consignes (« Pliez doucement le câble près de la fiche du téléphone… »)
passent par les messages `status`, affichés des deux côtés. Une sonde qui
n'annonce pas `watch` ne reçoit pas ces messages : seules les chutes de débit
sont alors relevées.

### 6. Verdict

À la fin, le logiciel transmet son verdict, que la sonde affiche :

```json
{"type":"verdict","severity":"ok","title":"Câble USB 3 en bon état",
 "detail":"5 Gb/s négociés, 410 Mb/s mesurés vers l'ordinateur…"}
```

`severity` : `ok`, `info`, `warning` ou `problem`. Le verdict porte aussi,
pour un affichage bref :

- `scores` : les notes, une par critère (`transfer`, `charge`, `overall`),
  avec `rating` (« Excellent »), `tone` (`good`, `fair`, `bad`, `unknown`) et
  `value` (« 5 Gb/s · 2,7 Gb/s mesurés ») ;
- `scores` peut aussi porter le critère `contacts` (test des faux contacts) ;
- `facts` : le détail rangé, `[{"label":"Débit vers le PC","value":"2,7 Gb/s"}, …]` ;
- `advice` : le conseil en une phrase, ou `null`.

`detail` reste l'explication complète, en texte.

### 7. Fin

`{"type":"bye"}`, dans un sens ou dans l'autre, puis fermeture.

### Erreurs

```json
{"type":"error","code":"busy","message":"Un test est déjà en cours."}
```

Codes : `protocol`, `busy`, `unauthorized`, `unknown_type`, `internal`.

## Mesure de débit (port 47821)

Le logiciel ouvre une connexion et envoie une ligne :

```json
{"session":"3f9c…","direction":"to_computer","duration_ms":4000}
```

- `to_computer` : la sonde envoie des octets en continu pendant
  `duration_ms`, puis ferme. Le logiciel compte et chronomètre.
- `to_probe` : le logiciel envoie pendant `duration_ms`, puis ferme son sens
  d'émission ; la sonde répond une ligne
  `{"received":123456789,"elapsed_ms":4003}` et ferme.

`duration_ms` est compris entre 500 et 15 000. Une session inconnue ferme
la connexion sans réponse. Les octets ne portent pas de sens (la liaison USB
ne compresse pas). Une connexion qui s'interrompt pendant la mesure est en soi
un résultat : la liaison a lâché.

## Sécurité

- La sonde n'écoute que sur l'interface du partage USB, et seulement tant que
  l'application est ouverte.
- Rien ne se passe sans l'accord explicite sur l'écran du téléphone.
- Le port de débit exige le jeton de session.
- Aucune donnée personnelle n'est échangée : fabricant, modèle, version
  d'Android et état de la batterie seulement.

## Développement

Le logiciel accepte la variable d'environnement `CABLESCOPE_PROBE`
(`adresse` ou `adresse:port`) pour joindre une sonde sans partage de connexion,
par exemple la sonde de développement de l'application
(`CABLESCOPE_PROBE_SERVE=60 ./gradlew :app:testDebugUnitTest --rerun` dans `android/`).

## Évolution

Le numéro `protocol` change à chaque modification incompatible. Les ajouts
compatibles (nouveaux champs, nouvelles capacités annoncées dans
`capabilities`) ne le changent pas.
