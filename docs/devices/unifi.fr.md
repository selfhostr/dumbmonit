# UniFi Network

Le réseau UniFi et ce qu'il gère : passerelles, commutateurs et points
d'accès hors ligne, isolés ou en attente d'adoption, mises à jour de
firmware en attente, les liens WAN et la connexion Internet telle que la
passerelle les voit, le nombre de clients, et les alarmes qu'UniFi a levées.
Ça fonctionne avec l'application UniFi Network d'une console UniFi OS
(Dream Machine, Cloud Gateway, Cloud Key) et avec un UniFi Network Server
auto-hébergé.

DumbMonit lit UniFi avec une clé d'API ou avec un compte local portant le
rôle **View Only**. Il ne redémarre, n'adopte, ne met à jour ni ne
provisionne jamais un appareil.

La panne à surveiller : **un point d'accès ou un commutateur qui se
déconnecte discrètement**. Les clients basculent vers le point d'accès
suivant, le réseau continue de fonctionner, et le trou de couverture ou le
lien montant perdu n'est remarqué que des jours plus tard. Pareil pour un
second lien WAN qui est tombé : rien ne casse jusqu'à ce que le premier
tombe aussi.

## Ports réseau à ouvrir

Du serveur DumbMonit (ou de son agent relais) vers l'équipement :

| Port | Protocole | Usage |
|---|---|---|
| 443 | TCP | API (HTTPS, par défaut) |
| 8443 | TCP | Seulement avec un UniFi Network Server auto-hébergé |

## Deux façons d'entrer

| Identifiant | API utilisée | Ce qu'elle lit |
|---|---|---|
| Clé d'API (console UniFi OS) | L'Integration API officielle, sur `/proxy/network/integration/v1` sur une console et `/integration/v1` sur un serveur auto-hébergé | Version, appareils et leur état, mises à jour de firmware, charge, mémoire et uptime de chaque appareil en ligne, nombre de clients |
| Compte View Only | L'API classique (`/api/s/<site>/stat/*`), sur `/proxy/network/api/…` sur une console | Tout ce qui précède, plus les clients par appareil et par type, les liens WAN, le statut WAN et Internet, et les alarmes |

Le statut WAN, Internet et des alarmes n'existe que dans l'API classique.
Avec une clé, DumbMonit demande aussi à l'API classique le statut WAN et
Internet avec la même clé ; si le contrôleur la refuse, ces séries sont
simplement absentes, sans erreur.

Les alarmes sont les entrées du journal système « critical » d'UniFi sur
les dernières 24 heures (UniFi Network 8 et plus récent), ou les alarmes
non archivées sur les versions plus anciennes. C'est le seul `POST` que
DumbMonit envoie, et il ne fait que lire.

Une session ouverte avec le compte View Only est gardée d'une sonde à
l'autre et rouverte quand elle expire : UniFi n'enregistre pas une nouvelle
connexion chaque minute.

## Ce qu'il surveille

Toutes les métriques sont préfixées `dumbmonit_unifi_`. Les séries
d'appareil portent `device` (son nom, ou son adresse MAC quand il n'en a
pas), `mac`, `model` et `type` (`gateway`, `switch`, `access_point`,
`other`).

| Métrique | Quoi | Labels |
|---|---|---|
| `info` | valeur 1 | `version`, `api` (`integration` ou `classic`) |
| `devices` | appareils par état : `online`, `offline`, `pending`, `updating`, `adopting`, `adoption_failed`, `isolated`, `other` | `state` |
| `devices_upgradable` | appareils adoptés avec une mise à jour de firmware disponible | |
| `device_state` | 0 en ligne, 1 hors ligne, 2 en attente d'adoption, 3 en mise à jour ou provisionnement, 4 en cours d'adoption, 5 adoption échouée, 6 isolé, 7 autre | labels d'appareil |
| `device_up` | 1 en ligne ou en mise à jour, 0 hors ligne, isolé ou adoption échouée ; absent pour un appareil pas encore adopté | labels d'appareil |
| `device_upgradable` | 1 quand une mise à jour de firmware est disponible | labels d'appareil, `firmware` |
| `device_uptime_seconds`, `device_cpu_percent`, `device_memory_percent` | appareils en ligne seulement | labels d'appareil |
| `device_clients` | clients connectés à l'appareil (compte View Only) | labels d'appareil |
| `wan_link_up` | 1 quand un port WAN de la passerelle est actif ; un port désactivé n'a aucune série | labels d'appareil, `wan` (`wan1`, `wan2`) |
| `wan_up`, `internet_up` | le statut WAN et Internet de la passerelle ; absent sans passerelle UniFi | |
| `internet_latency_seconds` | latence mesurée par la passerelle | |
| `wan_availability_percent` | disponibilité de chaque WAN sur les dernières 24 heures | `wan` |
| `subsystem_status` | 0 ok, 1 avertissement, 2 erreur, 3 inconnu, pour `wlan`, `wan`, `www`, `lan`, `vpn` | `subsystem` |
| `clients` | clients sur le site | |
| `clients_by_type` | `wireless`, `wired`, `guest`, `vpn` (compte View Only) | `type` |
| `alarms` | entrées du journal système critique des dernières 24 heures, ou alarmes non archivées | |
| `scrape_errors` | appels qui ont échoué à la dernière sonde | |
| `port_enabled`, `port_up`, `port_speed_bps`, `port_full_duplex`, `port_uplink` | ports de chaque commutateur ou passerelle en ligne (compte View Only, `port_table`) ; `port_up` est absent pour un port désactivé | `device`, `mac`, `port` (numéro en façade), `port_name` |
| `port_poe_enabled`, `port_poe_watts` | PoE de chaque port | idem |
| `port_rx_bytes_total`, `port_tx_bytes_total`, `port_rx_errors_total`, `port_tx_errors_total` | compteurs par port, tracés par le panneau de commutateur | idem |
| `port_neighbor_info` | valeur 1, voisin LLDP du port (`lldp_table`) | idem, `remote_chassis`, `remote_port` |

Les [règles intégrées](../alerting/rules.md#unifi-network) qui s'appliquent :

| Règle | Quoi | Seuil par défaut | Maintien | Gravité | Rappel |
|---|---|---|---|---|---|
| Appareil UniFi hors ligne | Un appareil UniFi adopté par le contrôleur est hors ligne, isolé ou l'adoption a échoué (`dumbmonit_unifi_device_up`). | < 1 | 5 min | Avis (`warning`), escalade après 1 h | 6 h |
| Internet UniFi en panne | La passerelle UniFi rapporte aucune connexion Internet (`dumbmonit_unifi_internet_up`). Sans passerelle UniFi, la série n'existe pas et la règle reste silencieuse. | < 1 | 3 min | Avertissement (`critical`) | 1 h |
| Lien WAN UniFi en panne | Un lien WAN de la passerelle UniFi est en panne (`dumbmonit_unifi_wan_link_up`) : avec deux liens, la redondance est perdue tandis que rien ne semble cassé. Un port désactivé n'a aucune série. | < 1 | 5 min | Avis (`warning`) | 6 h |
| CPU de passerelle UniFi élevé | Le processeur de la passerelle UniFi est au-dessus de 90 % depuis quinze minutes (`dumbmonit_unifi_device_cpu_percent{type="gateway"}`) ; se résout sous 80 %. | > 90 % | 15 min | Avis (`warning`) | 6 h |
| Appareil UniFi en attente d'adoption | Un appareil est apparu sur le réseau et attend d'être adopté dans UniFi (`dumbmonit_unifi_devices{state="pending"}`). | > 0 | 30 min | Info (`info`) | 24 h |
| Mise à jour de firmware UniFi disponible | Une mise à jour de firmware est disponible pour des appareils UniFi (`dumbmonit_unifi_devices_upgradable`). | > 0 | 1 h | Info (`info`) | 7 j |
| Alarmes UniFi | UniFi a journalisé des événements critiques ou levé des alarmes dans les dernières 24 heures (`dumbmonit_unifi_alarms`). Lu seulement avec un compte View Only. | > 0 | 5 min | Avis (`warning`) | 24 h |
| Sous-système UniFi en erreur | Un sous-système UniFi (Wi-Fi, LAN ou VPN) signale une erreur dans la vue de santé du contrôleur. Internet et les liens WAN ont leurs propres règles (`dumbmonit_unifi_subsystem_status`: 2). | ≥ 2 | 10 min | Avis (`warning`) | 6 h |
| Appareil UniFi en surchauffe | Un appareil UniFi signale qu'il surchauffe (`dumbmonit_unifi_device_overheating`). | > 0 | 5 min | Avis (`warning`) | 6 h |
| Budget PoE UniFi presque épuisé | Un commutateur consomme plus de 90 % de son budget PoE : le prochain appareil branché risque de ne pas être alimenté. Se lève sous 80 % (`dumbmonit_unifi_device_poe_used_watts` / `dumbmonit_unifi_device_poe_budget_watts`). | > 90 % | 15 min | Avis (`warning`) | 24 h |
| Canal Wi-Fi UniFi saturé | Le canal d'une radio de point d'accès est occupé plus de 80 % du temps depuis trente minutes : le Wi-Fi est lent pour tous ceux qui l'utilisent. Se lève sous 65 % (`dumbmonit_unifi_radio_channel_utilization_percent`). | > 80 % | 30 min | Avis (`warning`) | 24 h |
| Clients Wi-Fi UniFi à signal faible | Plus de 40 % des clients Wi-Fi, avec au moins cinq clients connectés, ont un signal plus faible que -75 dBm : les points d'accès sont mal placés ou trop peu nombreux (`dumbmonit_unifi_wifi_clients_weak_signal` / `dumbmonit_unifi_wifi_clients`). | > 40 % | 1 h | Info (`info`) | 7 j |
| Erreurs de port UniFi | Un port de commutateur a enregistré plus de 100 erreurs de réception ou d'émission en quinze minutes : vérifiez le câble et les connecteurs (`dumbmonit_unifi_port_rx_errors_total` + `dumbmonit_unifi_port_tx_errors_total`). | > 100 | 15 min | Avis (`warning`) | 24 h |

## Appareils

`/rest/user` (l'API classique, lue avec l'un ou l'autre identifiant) liste
chaque client que le contrôleur a jamais vu — la plupart d'un réseau
domestique, en pratique : téléphones qui passent, Wi-Fi invité, appareils
connectés qui ne reviennent jamais. Suivre tous ces appareils pour la
péremption enterrait l'alerte sous des appareils qui ne reviennent
simplement jamais, pas des appareils devenus silencieux. Par défaut, seuls
les clients que l'utilisateur a marqués d'une façon ou d'une autre
comptent : nommés dans UniFi, réservés sur une IP fixe, ou notés. L'option
**Clients à surveiller en plus des marqués** ajoute des adresses MAC ou
noms de client spécifiques, ou `all` pour suivre chaque client (non
recommandé sur autre chose qu'un petit réseau largement statique).

Les clients suivis sont publiés sous la famille générique partagée par
chaque intégration DumbMonit qui suit les appareils clients —
`dumbmonit_client_device_*`, préfixée sans `unifi`, documentée en entier
sous [Appareils clients](../alerting/rules.md#client-devices) :

| Métrique | Quoi | Labels |
|---|---|---|
| `last_seen_timestamp_seconds` | Dernière connexion, comme `/rest/user` la rapporte | `device`, `type` (`Wired`, `Wireless` ou `Unknown`), `os` (vide), `user` (vide), `kind` |
| `stale_seconds` | Positif une fois qu'un client suivi a dépassé `device_stale_days` (3 par défaut) sans se connecter ; alimente la règle [Appareil client périmé](../alerting/rules.md#client-devices) | `device`, `type`, `os`, `user`, `kind`, `signal` (toujours `connection` : UniFi n'a pas de sauvegarde à rapporter) |

## Aller plus loin

En plus de l'état des équipements, du WAN et des clients ci-dessus, DumbMonit lit, avec un compte local ou une clé d'API que l'API classique accepte :

- **Santé des équipements** : score de satisfaction, température et indicateur de surchauffe de chaque équipement en ligne.
- **PoE** : le budget PoE de chaque commutateur et ce que ses ports consomment.
- **Wi-Fi** : occupation du canal et nombre de clients de chaque radio (2,4, 5 et 6 GHz), nombre de clients Wi-Fi et combien ont un signal plus faible que -75 dBm.
- **Test de débit de la passerelle** : débit descendant, montant et latence du dernier test.

Rien n'envoie de commande ; si l'API classique refuse un appel, les séries correspondantes manquent simplement et la surveillance n'est pas en erreur.

## La page de la surveillance

Le panneau au-dessus des graphiques lit ce que la sonde a stocké ; ouvrir la
page n'interroge jamais UniFi. Il dit d'abord ce qui ne va pas (appareils
hors ligne, Internet ou un lien WAN en panne, alarmes), puis les appareils
par type avec leur état, firmware et charge, et les clients, puis le
tableau des clients suivis avec leur dernière connexion.

## Comment cela a été validé

La connexion, le rôle View Only, les réponses d'erreur (mot de passe
refusé, site inconnu, clé refusée, session expirée) et les réponses d'un
contrôleur sans appareils ont été vérifiées contre UniFi Network 10.6.106,
auto-hébergé. Aucun appareil n'a pu y être adopté : la liste d'appareils, la
santé d'un site équipé, les clients connus (`/rest/user`) et les charges
utiles de l'Integration API sont validées contre la documentation
officielle.

## Créer un accès en lecture seule dans UniFi Network

1. Pour une clé d'API sur une console UniFi OS : UniFi Network → Settings → Control Plane → Integrations → Create API Key. Nommez-la DumbMonit et copiez-la : elle n'est affichée qu'une fois.

2. Ou créez un compte local (requis pour l'état du WAN et des alarmes, et sur un serveur auto-hébergé) : Settings → Admins & Users → Create New, accès local uniquement.

    ```
    dumbmonit
    ```

3. Réglez le rôle UniFi Network de ce compte sur View Only.

4. Dans DumbMonit, saisissez l'adresse (unifi.lan, ou unifi.lan:8443 pour un serveur auto-hébergé) et collez la clé ou le compte.

5. Si le réseau n'est pas le site par défaut, réglez l'option Site sur le nom court affiché après /manage/ ou /network/ dans la barre d'adresse.

### Droits requis

- **View Only (rôle UniFi Network)** — Lit tout et ne change rien

!!! warning
    Utilisez un compte local, jamais le propriétaire ni un compte cloud UI.com, et acceptez le certificat auto-signé de la console dans les options sauf si vous avez installé le vôtre.

### Notes

Gardez l'authentification à deux facteurs désactivée pour ce seul compte local
View Only, car un serveur de supervision ne peut pas saisir de code. Un mot de
passe de propriétaire ou UI.com divulgué ouvrirait toutes les consoles qui lui
sont liées.

UniFi connaît chaque client qui s'est un jour connecté, la plupart des
téléphones de passage : seuls ceux nommés, réservés sur une IP fixe ou annotés
dans UniFi sont surveillés par défaut, pour éviter une alerte par téléphone de
visiteur. Listez-en d'autres dans l'option Clients to watch, ou saisissez all.

## Identifiants

| Identifiant | Champs |
|---|---|
| Clé d'API (console UniFi OS) | Une clé créée dans Settings → Control Plane → Integrations, envoyée comme `X-API-KEY`. |
| Compte View Only | Le nom d'utilisateur et le mot de passe du compte local View Only. |

Adresse : un nom d'hôte ou une IP (`unifi.lan`), `host:port`
(`unifi.lan:8443` pour un serveur auto-hébergé), ou une URL complète ; une
adresse copiée depuis le navigateur, avec `/manage/…` ou `/network/…`, est
acceptée. Options : site (`default`), port (443), vérification du
certificat, délai de requête (15 s), clients à surveiller en plus des
marqués (vide, ou `all`), et péremption d'appareil (`device_stale_days`,
3 jours par défaut).

## Limites connues

- L'état d'Internet et du WAN, les alarmes, le PoE et le détail du Wi-Fi n'existent que dans l'API classique. Certains contrôleurs la refusent à une clé d'API : ces séries sont alors absentes, sans erreur. Un compte View Only lit tout.
- Les alarmes sont les entrées critiques du journal système sur les dernières 24 heures à partir de UniFi Network 8, les alarmes non archivées avant.
- Seuls les clients que vous avez marqués dans UniFi (nommés, à IP fixe ou annotés) sont suivis un par un, sauf si l'option `watched_clients` en décide autrement.
- Aucune commande n'est jamais envoyée : ni redémarrage, ni adoption, ni mise à jour de firmware.
