# Ce que Windows montre, et ce qu'on peut récupérer qu'il ne montre pas

Synthèse de recherche (octobre 2026) pour la collecte Windows de CableScope.
Sources principales : documentation Microsoft Learn (lue via ses dépôts GitHub
MicrosoftDocs), en-têtes du SDK/WDK (win32metadata), code source de l'exemple
USBView de Microsoft (MS-PL), code de nusb et cyme.

**Légende**

- **Non vérifié** : rien n'a pu être testé sur une vraie machine Windows. Tout ce
  qui est marqué *(à vérifier)* doit être confirmé sur du matériel réel.
- **Fiabilité** : *Mesuré* = état réel du lien ; *Déclaré* = annoncé par
  l'appareil, le port ou le câble ; *Déduit* = conclusion de CableScope.

## En bref

1. **Windows ne montre presque rien de ce qui compte pour un câble.**
   - Le Gestionnaire de périphériques n'affiche ni la vitesse du lien, ni ce que l'appareil sait faire, ni le type de port, ni les erreurs de lien.
   - Les Paramètres n'affichent que des notifications et, sur les PC USB4, une page « Concentrateurs et appareils USB4 ».
2. **L'essentiel est pourtant lisible sans droits administrateur**, via les commandes de hub (IOCTL) qu'utilise l'exemple USBView de Microsoft :
   - vitesse réelle ;
   - capacité SuperSpeed de l'appareil et du port ;
   - port USB-C ou non ;
   - descripteurs BOS et Billboard ;
   - état de connexion.
   Cela suffit pour la règle phare : **« appareil USB 3 qui tourne en USB 2 sur un port USB 3 → câble USB 2 probable »**.
3. **Les erreurs de lien** (réinitialisations, échecs d'énumération, erreurs de transaction) sont visibles dans les traces ETW de la pile USB. Ces traces exigent les droits administrateur ou l'appartenance au groupe « Utilisateurs du journal de performances ».
4. **L'e-marker du câble USB-C et le contrat de charge ne sont pas accessibles** par une API Windows standard.
   - Seule voie pour l'e-marker : l'interface de test UCSI. Elle demande les droits administrateur, une clé de registre et un redémarrage du périphérique, et ne marche que si le firmware la gère.
   - C'est un mode expert optionnel, pas le fonctionnement par défaut.
5. **USB4 / Thunderbolt** :
   - la vitesse du lien, le couplage des voies et des infos câble passent par des événements ETW (administrateur) ;
   - les échecs de tunnel DisplayPort sont dans le journal Système, lisible sans droits ;
   - le SDK déclare une IOCTL de débogage USB4 dont les droits d'accès sont inconnus.
6. **DisplayPort** : Windows ne donne que le type de connexion. Le débit du lien et le nombre de voies viennent des API des fabricants de GPU (NVIDIA NVAPI, AMD ADLX, Intel IGCL).

## 1. Lien USB et ports (sans droits administrateur)

Toutes ces IOCTL sont `FILE_ANY_ACCESS` (cf. `usbioctl.h`). USBView ouvre les hubs
avec `GENERIC_WRITE`, sans élévation.

| Information | Affichée par Windows ? | Source | Fiabilité |
|---|---|---|---|
| Vitesse réelle Low/Full/High | Non | `IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX.Speed` (ne dépasse jamais High) | Mesuré |
| Lien SuperSpeed / SuperSpeedPlus actif | Non | `IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2.Flags` (`DeviceIsOperatingAtSuperSpeed[Plus]OrHigher`) | Mesuré |
| L'appareil *sait* faire SuperSpeed / SuperSpeedPlus | Non (Windows 8 affichait « peut être plus rapide sur un port USB 3.0 ») | Même IOCTL V2 : `DeviceIsSuperSpeed[Plus]CapableOrHigher` | Déclaré |
| Protocoles supportés par le port (USB 1.1/2.0/3.x) | Non | V2 `SupportedUsbProtocols` (l'entrée doit avoir `Usb300=1`) | Déclaré |
| Débit par voie et nombre de voies (Gen 2x2) | Non | `IOCTL_USB_GET_NODE_CONNECTION_SUPERSPEEDPLUS_INFORMATION` (Windows 10 1709+). *Le nombre de voies réel n'est pas confirmé sur du matériel (à vérifier).* | Mesuré |
| Port USB-C, accessible à l'utilisateur, port compagnon USB 2 ↔ USB 3 | Non | `IOCTL_USB_GET_PORT_CONNECTOR_PROPERTIES` (`PortConnectorIsTypeC`, `PortIsUserConnectable`, `CompanionIndex`). Dépend des tables ACPI du fabricant. | Déclaré |
| Type de hub, nombre de ports, alimenté par le bus ou non | Non (ancien onglet « Alimentation » des hubs USB 2) | `IOCTL_USB_GET_HUB_INFORMATION_EX`, `IOCTL_USB_GET_NODE_INFORMATION` | Déclaré |
| État de connexion : échec d'énumération, surintensité, alimentation ou bande passante insuffisante, hubs trop imbriqués | Indirectement (« Périphérique USB inconnu », code 43, notification) | `USB_CONNECTION_STATUS` dans `…_INFORMATION_EX` | Mesuré |
| Descripteur d'appareil (VID, PID, bcdUSB), adresse, configuration active | En partie (ID matériels) | `…_INFORMATION_EX` (version en cache, sans trafic USB) | Déclaré |
| Contrôleur hôte : identifiants PCI, nombre de ports racine | En partie | `IOCTL_USB_USER_REQUEST` + `USBUSER_GET_CONTROLLER_INFO_0` (code 0x220438, absent de windows-sys) | Déclaré |

**À savoir :**

- **Ne jamais se fier à `Speed` seul**, comme le fait USBView : toujours le combiner avec les drapeaux V2.
- **Les IOCTL V2, de connecteur et SuperSpeedPlus exigent la pile USB 3 de Windows 8 ou plus** (USBHUB3). Il faut garder le repli d'USBView : `_EX`, puis l'IOCTL simple.
- **Ne pas sonder en boucle.** Chaque lecture de descripteur est un vrai échange USB avec le firmware de l'appareil.
- **Ne pas réveiller les appareils en veille.** USBView vérifie l'état d'alimentation avant de lire.

## 2. Descripteurs lisibles via le hub (sans changer de pilote)

`IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION` permet uniquement les requêtes
standard `GET_DESCRIPTOR`. nusb l'expose déjà (`Device::get_descriptor`) sans
exiger WinUSB.

| Descripteur | Ce qu'il apporte | Affiché par Windows ? |
|---|---|---|
| Configuration | `bMaxPower`, en unités de 2 mA en USB 2 et de 8 mA en SuperSpeed. Indique aussi si l'appareil est auto-alimenté. | Non |
| BOS 0x03 SuperSpeed | L'appareil sait faire 5 Gb/s, **même s'il tourne en USB 2** | Non |
| BOS 0x0A SuperSpeedPlus | Débits par voie (10 Gb/s…) et nombre minimal de voies | Non |
| BOS 0x0D **Billboard** | Modes alternatifs (DisplayPort 0xFF01, Thunderbolt 0x8087, USB4 0xFF00) et leur état : erreur, non tenté, **échec**, réussi. Indique aussi si le câble doit fournir VCONN (donc avoir un e-marker). | **Indirectement** : ce sont exactement les déclencheurs des notifications Windows « La connexion d'affichage peut être limitée », etc. |
| BOS 0x06 à 0x09 Power Delivery, batterie | Rôle PD de l'appareil, version PD | Non |
| BOS 0x04 Container ID | Regroupe les deux moitiés (USB 2 et USB 3) d'un même appareil | Via `DEVPKEY_Device_ContainerId` |
| Chaînes (produit, série) | Nom réel | En partie (« Description de périphérique signalée par le bus ») |

Impossible par ce biais : les requêtes propriétaires ou de classe (jeu de
descripteurs Microsoft OS 2.0, requêtes de hub, données PD et UCSI).

## 3. Propriétés PnP, registre et journaux (sans droits administrateur)

| Information | Affichée par Windows ? | Source | Intérêt pour le diagnostic |
|---|---|---|---|
| Chemin ACPI du port (`…#ACPI(HS03)` contre `ACPI(SS03)`) | Enfoui dans l'onglet Détails | `DEVPKEY_Device_LocationPaths` | Élevé : montre si l'appareil est sur la moitié USB 2 du port. *Convention de nommage du fabricant (à vérifier).* |
| Emplacement physique du port (`_PLD` : face, couleur, forme) | Non (hexadécimal brut) | `DEVPKEY_Device_PhysicalDeviceLocation` | Moyen : dire « port arrière gauche » plutôt que « Port_#0002 » |
| Code de problème et son statut NTSTATUS | Code seulement | `DEVPKEY_Device_ProblemCode` / `ProblemStatus` | Élevé (code 43) |
| Appareils « Périphérique USB inconnu (échec de la requête de descripteur / Link in Compliance Mode) » | Oui, sans explication | Identifiants factices (`USB\VID_0000&PID_0002`, `USB\PORT_LINK_COMPLIANCE_MODE`) et description | Élevé : signature typique d'un câble défectueux |
| Dates de première installation, de dernière arrivée et de dernier retrait | Enfoui | `DEVPKEY_Device_FirstInstallDate`, `LastArrivalDate`, `LastRemovalDate` | Moyen : branchements et débranchements à répétition |
| Historique des appareils déjà connectés | Avec « Afficher les périphériques cachés » | Registre `HKLM\SYSTEM\CurrentControlSet\Enum\USB` | Moyen |
| Cache du descripteur Microsoft OS, numéros de série ignorés | Non | Registre `…\Control\usbflags\VVVVPPPPRRRR` | Faible |
| Échecs de tunnel DisplayPort USB4 | Observateur d'événements | Journal Système, source `USB-USB4DeviceRouter-EventLogs`, événements 1 et 2 | Élevé pour les docks USB4 |
| Arrivées et retraits horodatés | Observateur d'événements | Microsoft-Windows-Kernel-PnP/Configuration *(identifiants d'événements à vérifier)* | Faible à moyen |

Toutes les propriétés d'un nœud se lisent avec `CM_Get_DevNode_Property_Keys`
puis `CM_Get_DevNode_PropertyW`. `pnputil /enum-devices /properties` fait la même chose en
ligne de commande.

WMI (`Win32_USBHub`, `Win32_USBController`…) n'apporte presque rien de plus et
reste lent. On l'écarte.

## 4. Traces ETW : les preuves d'un lien de mauvaise qualité (administrateur)

Une session ETW en temps réel exige les droits administrateur ou le groupe
« Utilisateurs du journal de performances ».

| Fournisseur | Événements utiles | Ce qu'ils révèlent |
|---|---|---|
| Microsoft-Windows-USB-USBHUB3 | Erreurs d'énumération (62 à 65, 160 à 197), changements d'état de port (100), erreurs de port (102, 103), réinitialisations de hub (120 à 122). Le « rundown » liste l'arbre, les vitesses et les types de connecteurs. | Tentatives répétées, réinitialisations, passage de SuperSpeed à High Speed, états de lien Inactive / Compliance / Recovery |
| Microsoft-Windows-USB-USBXHCI | Erreurs de transaction, « Babble », délais dépassés, réinitialisations du contrôleur | Symptômes classiques d'un problème d'intégrité du signal (câble) |
| Microsoft.Windows.USB.USB4.HostRouter / DeviceRouter (TraceLogging) | Vitesse actuelle, prise en charge et visée (`CurrentLinkSpeed`, `SupportedLinkSpeeds`), `LaneBonded`, `CableInfo`, `CableUsb4Version`, erreurs par voie, versions de firmware | **Limitation d'un câble USB4 / Thunderbolt** (Gen 2 au lieu de Gen 3, une seule voie). *Répartition des champs par événement à vérifier.* |

En Rust : `ferrisetw`, qui décode le schéma TDH mais dont la maintenance ralentit,
ou les API brutes `Win32_System_Diagnostics_Etw`.

USBPcap avec Wireshark exige d'installer un pilote filtre, et ne voit pas l'état du lien. On l'écarte.

## 5. USB-C et Power Delivery

| Information (équivalent Linux) | Disponible sous Windows ? | Comment |
|---|---|---|
| Port USB-C | ✅ Sans droits | `PortConnectorIsTypeC` (IOCTL du hub) |
| Rôles données et alimentation, type de partenaire, courant annoncé (Rp), contrat PD (RDO), PDO locaux et du partenaire | ⚠️ Peut-être sans droits | API Policy Manager `usbpmapi.h` (Windows 10 1809+). Microsoft la documente « mode utilisateur et noyau ». **Accès depuis une application ordinaire à prouver sur matériel réel.** Aucune liaison Rust : FFI à écrire. |
| Charge lente ou PC qui ne charge pas | ✅ Approximatif | Les notifications Windows ; `PowerManager.PowerSupplyStatus` (WinRT) *(correspondance à vérifier)* |
| Débit de charge de la batterie (mW) | ✅ Sans droits | `IOCTL_BATTERY_QUERY_STATUS` (`BATTERY_STATUS.Rate`), WMI `BatteryStatus.ChargeRate`. C'est ce qui entre dans la batterie, pas la puissance du chargeur. |
| Puissance du chargeur | ❌ | Pas d'API documentée. Certaines WMI de fabricants (Dell, Lenovo, HP) *(à vérifier)* |
| Modes alternatifs en échec | ✅ Sans droits | Descripteur Billboard |
| Modes alternatifs actifs | ❌ (pilote uniquement) | UCSI `GET_CURRENT_CAM`, via l'interface de test |
| **Câble : vitesse, courant 3 A / 5 A, actif ou passif, latence (e-marker)** | ❌ par défaut | Seulement via l'**interface de test UCSI** : clé `TestInterfaceEnabled=1`, droits administrateur, redémarrage du périphérique, IOCTL non documentée. Nécessite que le firmware gère `GET_CABLE_PROPERTY`. L'identité complète SOP′ exige UCSI 2.1 (`GET_PD_MESSAGE`). |
| Contrôleur PD en direct (tension, courant) | ❌ | Accès direct non documenté au contrôleur PD, propre à chaque modèle (fait par HWiNFO sur certaines cartes ASUS et Gigabyte ; peut figer le système) |

**Verdict e-marker** : aucune API Windows documentée, active par défaut et
utilisable par une application, ne donne les informations du câble. Les
machines à contrôleur TCPC n'ont aucune voie générique. Pour lire un e-marker de
façon fiable, il faut un testeur matériel.

## 6. USB4 et Thunderbolt

| Information | Affichée par Windows ? | Source | Droits |
|---|---|---|---|
| Arbre USB4, fabricant, firmware, version USB4 | Oui : Paramètres > Bluetooth et appareils > USB > Concentrateurs et appareils USB4 (si le gestionnaire de connexion est celui de Microsoft) | Interface privée, non documentée | — |
| Vitesse et voies du lien USB4, infos câble | Partiellement | ETW TraceLogging (voir section 4) | Administrateur |
| Espace de configuration USB4 brut (vitesse et largeur des voies) | Non | `IOCTL_USB4_HRD_DEBUG_READ_CONFIGURATION_SPACE` (`usb4dbgioctl.h`), à décoder avec la disposition de `tb_regs.h` de Linux | **Inconnu (à vérifier)** |
| Appareil PCIe tunnelisé | Gestionnaire de périphériques > Détails | `DEVPKEY_PciDevice_IsTunneledDevice`, `_UsbHostRouterName` | Sans droits |
| Contrôleurs Intel plus anciens (connexion gérée par le firmware Intel, ICM) | Thunderbolt Control Center (sécurité, approbation) | Rien de public sur le lien | — |

**Pièges :**

- Windows affiche toujours **20 Gb/s** pour l'adaptateur réseau USB4/Thunderbolt (source : Intel).
- Les vitesses PCIe tunnelisées sont virtuelles.
- Aucune de ces deux valeurs ne dit rien du câble.

## 7. DisplayPort sur USB-C

| Information | Source | Droits |
|---|---|---|
| Type de connexion (DP externe, HDMI, **DP tunnelisé en USB**) | `QueryDisplayConfig` → `outputTechnology` (valeur 18 = `DISPLAYPORT_USB_TUNNEL`) | Sans droits |
| Débit du lien négocié (HBR2/HBR3/UHBR) et nombre de voies, maximums de l'écran | NVIDIA `NvAPI_GetDisplayPortInfo` ; AMD ADLX `GetDPLinkRate` / `GetNumberOfActiveLanes` ; Intel IGCL (lecture DPCD par `ctlAUXAccess`) | Sans droits pour NVIDIA et AMD, *Intel à vérifier* |
| Capacités de l'écran | EDID dans le registre (`Enum\DISPLAY\…\Device Parameters\EDID`) | Sans droits |

## 8. Règles de diagnostic retenues

| # | Constat | Conclusion | Certitude |
|---|---|---|---|
| 1 | L'appareil sait faire SuperSpeed (drapeau V2 ou BOS), tourne en High Speed, et son port ou son compagnon supporte l'USB 3, sans hub intermédiaire | Câble USB 2.0 uniquement (ou fils USB 3 abîmés). Encore plus probable si ça persiste après avoir retourné la prise. | Probable |
| 2 | Même constat, mais le port ne supporte pas l'USB 3 | Le port est la limite | Certain |
| 3 | L'appareil sait faire 10 Gb/s, tout le chemin aussi, mais le lien est à 5 Gb/s | Câble 5 Gb/s ou hub Gen 1 caché | Possible |
| 4 | Appareil Gen 2x2 en une seule voie sur un port x2 | Câble sans seconde paire, ou négociation x2 en échec | Possible |
| 5 | Billboard : DisplayPort, Thunderbolt ou USB4 en échec ou non tenté sur un port USB-C | Câble sans voies rapides, sans VCONN ou sans e-marker, ou port incompatible. **Certain si combiné avec la règle 1.** | Possible |
| 6 | Moitié USB 3 en échec d'énumération ou en réinitialisations, moitié USB 2 qui fonctionne | Câble abîmé ou trop long | Possible |
| 7 | `DeviceCausedOvercurrent` | Court-circuit dans le câble ou l'appareil | Possible |
| 8 | `DeviceNotEnoughPower` | Budget électrique du hub, **pas le câble** | Certain |
| 9 | USB4 en Gen 2 alors que les deux routeurs supportent Gen 3, ou une seule voie (ETW) | Câble non certifié 40 Gb/s, ou trop long pour du passif | Probable |
| 10 | DisplayPort en HBR2 alors que GPU et écran supportent HBR3 | Câble, adaptateur ou retimer de qualité insuffisante | Possible |
| 11 | DisplayPort sur 2 voies via un dock | Mode qui partage les voies avec l'USB 3 (affectation D), **pas le câble** | Certain |
| 12 | Branchements et débranchements répétés sur le même port | Connecteur ou câble lâche | Possible |

Ne jamais conclure sur le câble à partir de : la vitesse PCIe tunnelisée, la
vitesse affichée de l'adaptateur réseau USB4, ou `bcdUSB` seul (un appareil USB 3
branché en USB 2 annonce souvent 2.10).

## 9. Niveaux de collecte proposés

1. **Standard, sans droits, par défaut.** IOCTL des hubs, BOS et Billboard, propriétés PnP, journal Système, batterie, `QueryDisplayConfig`, API des GPU.
   Couvre les règles 1 à 8, 10 et 11.
2. **Diagnostic approfondi, administrateur, optionnel.** Session ETW pendant qu'on rebranche l'appareil (USBHUB3, USBXHCI, USB4).
   Couvre les règles 6, 9 et 12, avec des preuves.
3. **Expert, explicite.** Interface de test UCSI pour l'e-marker, uniquement si l'utilisateur l'active en connaissance de cause.
   Pas de pilote noyau maison : signature, distribution et risques sont disproportionnés.

## 10. Écosystème et projets voisins

**Bibliothèques :**

- **windows-sys** couvre 31 IOCTL USB et leurs structures. Il faut définir à la main :
  - `IOCTL_USB_USER_REQUEST` (0x220438) ;
  - `IOCTL_USB_GET_NODE_CONNECTION_SUPERSPEEDPLUS_INFORMATION` (0x220484) et sa structure ;
  - les bits `DEVICE_IS_*` des drapeaux V2.
- **nusb** (MIT/Apache-2.0) : son code Windows (`src/platform/windows_winusb/`) est un bon modèle. Il **jette** l'info « l'appareil sait faire SuperSpeed mais ne l'utilise pas », qui est précisément celle qui nous intéresse.
- **cyme** est sous **GPL-3.0** : on peut s'en inspirer, mais pas copier son code dans un projet sous licence permissive. Sous Windows, il ne fait que s'appuyer sur nusb.

**Projets similaires à étudier pour se positionner :**

- **UsbTreeView** : la référence experte, freeware fermé.
- **whatcable-windows** : sonde déjà l'interface de test UCSI.
- **UsbAtlas** : sans droits ; détecte les routeurs USB4 et le Billboard, et lit le journal UCMUCSICX.
- **usbscope** : SMBIOS, WMI, Billboard.

**Ce que CableScope apporte** : le **diagnostic en langage clair**
(mesuré / déclaré / déduit), un rapport partageable et un même moteur pour
l'application portable et le site web.

## 11. À vérifier sur une vraie machine Windows

- [ ] Nombre de voies réel rapporté pour un lien Gen 2x2 (IOCTL SuperSpeedPlus)
- [ ] API Policy Manager (`UsbPm_Register`) depuis une application ordinaire, sans droits
- [ ] Droits d'accès et activation de l'IOCTL de débogage USB4 sur une version commerciale de Windows
- [ ] Champs exacts des événements ETW USB4 (24H2 / 25H2)
- [ ] Comportement des lectures de descripteurs sur un appareil en veille (USBHUB3)
- [ ] Valeur `outputTechnology` pour du DisplayPort en mode alternatif (non tunnelisé)
- [ ] Persistance des nœuds « Périphérique USB inconnu » comme historique d'échecs par port

## Sources principales

- IOCTL USB : <https://learn.microsoft.com/windows-hardware/drivers/ddi/usbioctl/>
- Exemple USBView (MS-PL) : <https://github.com/microsoft/Windows-driver-samples/tree/main/usb/usbview>
- En-têtes SDK : <https://github.com/microsoft/win32metadata/tree/main/generation/WinSDK/RecompiledIdlHeaders>
- Notifications USB-C : <https://learn.microsoft.com/windows-hardware/drivers/usbcon/usb-type-c-troubleshooting-notifications>
- UCSI sous Windows : <https://learn.microsoft.com/windows-hardware/drivers/usbcon/ucsi>
- Policy Manager : <https://learn.microsoft.com/windows-hardware/drivers/usbcon/policy-manager-client>
- Capture de traces USB : <https://learn.microsoft.com/windows-hardware/drivers/usbcon/how-to-capture-a-usb-event-trace>
- Événements ETW USB4 : <https://learn.microsoft.com/windows-hardware/design/component-guidelines/usb4-tracelogging-rundown-events>
- Journal Système des tunnels DisplayPort USB4 : <https://learn.microsoft.com/windows-hardware/design/component-guidelines/usb4-system-event-log-events-for-display-tunnels>
- Propriétés PnP : <https://learn.microsoft.com/windows-hardware/drivers/install/devpkey-device-locationpaths>
- Registres USB4 (Linux, pour le décodage) : <https://github.com/torvalds/linux/blob/master/drivers/thunderbolt/tb_regs.h>
- NVAPI : <https://github.com/NVIDIA/nvapi> ; Intel IGCL : <https://github.com/intel/drivers.gpu.control-library> ; AMD ADLX : <https://gpuopen.com/adlx/>
- nusb : <https://github.com/kevinmehall/nusb> ; cyme : <https://github.com/tuna-f1sh/cyme>
- Projets voisins : <https://github.com/darrylmorley/whatcable-windows>, <https://github.com/HowdyMoto/UsbAtlas>
