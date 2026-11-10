# pfSense

Un pare-feu pfSense via le paquet communautaire d'API REST
(`pfSense-pkg-RESTAPI`, version 2) : passerelles en panne, qui perdent des
paquets ou répondent lentement, interfaces activées sans lien, services
activés qui se sont arrêtés, et CPU, mémoire, disque et température.

DumbMonit envoie une clé d'API dans l'en-tête `X-API-Key`. Une clé porte
exactement les privilèges de l'utilisateur pfSense auquel elle appartient :
l'utilisateur `dumbmonit` ci-dessous détient seulement les privilèges `GET`
des cinq pages que DumbMonit lit, la clé peut donc lire ces pages et ne rien
changer. Chaque minute, DumbMonit lit, en parallèle,
`/api/v2/status/system`, `/api/v2/status/gateways`,
`/api/v2/status/interfaces`, `/api/v2/status/services` et
`/api/v2/system/version`, et, si le privilège a été donné,
`/api/v2/system/restapi/version`.

!!! warning "Validé contre la documentation du fournisseur seulement — pas encore testé sur un système réel"

    Cette intégration a été construite à partir de la documentation
    publiée du paquet d'API REST et du code source de ses modèles (noms de
    champs, unités, l'enveloppe de réponse et ses identifiants d'erreur),
    pas contre un pfSense en fonctionnement. Chaque appel est une lecture.
    Dites-nous ce qui casse.

## Ports réseau à ouvrir

Du serveur DumbMonit (ou de son agent relais) vers l'équipement :

| Port | Protocole | Usage |
|---|---|---|
| 443 | TCP | API (HTTPS, par défaut) |
| 80 | TCP | Seulement avec l'option de protocole HTTP |

## Ce qu'il surveille

Toutes les métriques sont préfixées `dumbmonit_pfsense_`.

| Métrique | Labels | Signification |
|---|---|---|
| `cpu_usage_percent`, `memory_used_percent`, `swap_used_percent`, `disk_used_percent`, `mbuf_used_percent` | | Comme le tableau de bord les montre, en pourcentage. Le chiffre de CPU est la propre estimation de pfSense depuis la moyenne de charge. |
| `temperature_celsius` | | Quand pfSense peut lire un capteur. |
| `load_average` | `period` : `1m`, `5m`, `15m` | Moyenne de charge. |
| `cpu_count` | | Cœurs de processeur. |
| `version_info` | `version` | Valeur 1 : la version de pfSense, `2.7.2-RELEASE`. |
| `gateway_up` | `gateway` | 0 quand pfSense marque la passerelle en panne, 1 sinon. |
| `gateway_status_info` | `gateway`, `status` | Valeur 1 : le propre mot de pfSense (`none` quand tout va bien, `down`, `loss`, `delay`). |
| `gateway_loss_percent` | `gateway` | Perte de paquets vers l'adresse de surveillance. |
| `gateway_delay_milliseconds`, `gateway_stddev_milliseconds` | `gateway` | Aller-retour vers l'adresse de surveillance et son écart ; absent tant que la passerelle est en panne. |
| `gateways`, `gateways_down` | | Compteurs. |
| `interface_enabled`, `interface_up` | `interface`, `descr` | Activée dans la configuration ; lien actif. |
| `interface_down` | `interface`, `descr` | 1 quand l'interface est activée et n'a pas de lien. Une interface désactivée n'est jamais en panne. |
| `interface_bytes_in_total`, `interface_bytes_out_total`, `interface_errors_in_total`, `interface_errors_out_total` | `interface`, `descr` | Compteurs. |
| `interfaces_down` | | Compteur. |
| `service_running`, `service_enabled` | `service`, `description` | Comme Status > Services les montre. |
| `service_stopped` | `service`, `description` | 1 quand le service est activé et ne tourne pas. |
| `services_stopped` | | Compteur. |
| `states`, `states_max`, `states_used_percent` | | Connexions que pf suit, la limite, et la part utilisée. Demande le privilège `/api/v2/firewall/states/size GET`. |
| `carp_enabled`, `carp_maintenance_mode` | | 1 quand CARP est activé ; 1 quand il a été mis en mode maintenance persistant. Demande le privilège `/api/v2/status/carp GET`. |
| `restapi_update_available` | | Avec le privilège `/api/v2/system/restapi/version` : 1 quand le paquet d'API REST a une release plus récente. |
| `restapi_version_info` | `version`, `latest` | Valeur 1 : release installée et dernière release du paquet. |

pfSense ne dit pas via cette API si une mise à jour de pfSense est
disponible : la seule route qui touche aux mises à jour système
(`/api/v2/system/update`) en démarre une. DumbMonit rapporte seulement les
propres mises à jour du paquet.

La table d'états et CARP sont des lectures facultatives. Chacune demande un privilège de plus sur l'utilisateur `dumbmonit` : `/api/v2/firewall/states/size GET` et `/api/v2/status/carp GET`. Sans le privilège, pfSense refuse cette lecture et les chiffres correspondants sont simplement absents : ni erreur, ni alerte.

Les [règles intégrées](../alerting/rules.md#pfsense) qui s'appliquent :

| Règle | Quoi | Seuil par défaut | Maintien | Gravité | Rappel |
|---|---|---|---|---|---|
| Passerelle pfSense en panne | Une passerelle pfSense est en panne : son adresse de surveillance ne répond plus, et le trafic qu'elle porte s'arrête ou bascule (`dumbmonit_pfsense_gateway_up`). | < 1 | 3 min | Avertissement (`critical`) | 6 h |
| Passerelle pfSense perd des paquets | Une passerelle pfSense perd plus de 10 % des sondes vers son adresse de surveillance : la ligne est dégradée (`dumbmonit_pfsense_gateway_loss_percent unless on (target, gateway) dumbmonit_pfsense_gateway_up == 0`). | > 10 % | 10 min | Avis (`warning`) | 6 h |
| Passerelle pfSense lente | L'aller-retour vers l'adresse de surveillance d'une passerelle pfSense est resté au-dessus de 500 ms pendant quinze minutes (`dumbmonit_pfsense_gateway_delay_milliseconds`). | > 500 ms | 15 min | Avis (`warning`) | 6 h |
| Interface pfSense sans lien | Une interface pfSense activée n'a pas de lien : câble, port de commutateur ou carte réseau (`dumbmonit_pfsense_interface_down`). | > 0 | 5 min | Avis (`warning`) | 6 h |
| Service pfSense arrêté | Un service pfSense activé ne tourne pas : résolveur DNS, VPN, NTP… (`dumbmonit_pfsense_service_stopped`). | > 0 | 5 min | Avis (`warning`) | 6 h |
| Disque pfSense presque plein | Le disque pfSense est plein à plus de 90 % : les logs, rapports et mises à jour de paquet arrêtent de s'écrire (`dumbmonit_pfsense_disk_used_percent`). | > 90 % | 30 min | Avis (`warning`) | 1 j |
| Table d'états pfSense presque pleine | pf suit plus de 80 % des connexions auxquelles il a droit : passé la limite, le pare-feu rejette les nouvelles connexions sans autre symptôme. Se lève sous 70 % (`dumbmonit_pfsense_states_used_percent`). | > 80 % | 10 min | Avis (`warning`) | 6 h |
| Buffers réseau pfSense presque épuisés | Plus de 90 % des buffers réseau sont utilisés : un pare-feu qui n'en a plus cesse de transmettre alors que son processeur est au repos. Se lève sous 80 % (`dumbmonit_pfsense_mbuf_used_percent`). | > 90 % | 10 min | Avis (`warning`) | 6 h |
| Température pfSense élevée | La température que pfSense lit est restée au-dessus de 85 °C pendant dix minutes : le boîtier va se brider lui-même. Se lève à 78 °C (`dumbmonit_pfsense_temperature_celsius`). | > 85 °C | 10 min | Avis (`warning`) | 6 h |
| CPU pfSense élevé | Le processeur est resté au-dessus de 90 % pendant quinze minutes. Se lève sous 80 % (`dumbmonit_pfsense_cpu_usage_percent`). | > 90 % | 15 min | Avis (`warning`) | 6 h |
| Swap pfSense utilisé | pfSense utilise plus de la moitié de son swap depuis une heure : il manque de mémoire (`dumbmonit_pfsense_swap_used_percent`). | > 50 % | 1 h | Avis (`warning`) | 24 h |
| Erreurs d'interface pfSense | Une interface a enregistré plus de 100 erreurs de réception ou d'émission en quinze minutes : vérifiez le câble, le module SFP et le réglage duplex (`dumbmonit_pfsense_interface_errors_in_total` + `dumbmonit_pfsense_interface_errors_out_total`). | > 100 | 15 min | Avis (`warning`) | 24 h |
| Mode maintenance CARP pfSense laissé actif | CARP est resté en mode maintenance persistant pendant une heure : ce pare-feu a cédé ses adresses virtuelles à son partenaire et ne les reprendra pas de lui-même (`dumbmonit_pfsense_carp_maintenance_mode`). | > 0 | 1 h | Info (`info`) | 24 h |

## La page de la surveillance

Le panneau Firewall dit ce qui ne va pas en une phrase, montre le CPU, la
mémoire, le disque et la température, puis liste chaque passerelle avec sa
latence et sa perte, les interfaces sans lien, les services arrêtés, et une
mise à jour disponible du paquet d'API REST.

## Créer une clé d'API REST en lecture seule pour DumbMonit

1. Installez le paquet pfSense-pkg-RESTAPI (version 2) depuis Diagnostics → Command Prompt, avec le fichier qui correspond à votre version de pfSense (2.7.2 ci-dessous).

    ```
    pkg-static add https://github.com/pfrest/pfSense-pkg-RESTAPI/releases/latest/download/pfSense-2.7.2-pkg-RESTAPI.pkg
    ```

2. System → REST API → Settings : cochez BasicAuth et Key parmi les méthodes d'authentification.

3. System → User Manager → Users → Add : créez l'utilisateur dumbmonit avec un long mot de passe aléatoire.

4. Modifiez l'utilisateur et, sous Assigned Privileges, ajoutez les privilèges listés ci-dessous.

5. Créez la clé en tant que dumbmonit, copiez la valeur key de la réponse, puis retirez de nouveau le privilège POST.

    ```
    curl -k -u dumbmonit -X POST -H 'Content-Type: application/json' -d '{"descr":"DumbMonit"}' https://pfsense.lan/api/v2/auth/key
    ```

6. Dans DumbMonit, saisissez l'adresse de pfSense (pfsense.lan) et collez la clé comme clé d'API.

### Droits requis

- **/api/v2/status/system GET** — CPU, mémoire, disque, température
- **/api/v2/status/gateways GET** — État, délai et perte des passerelles
- **/api/v2/status/interfaces GET** — Lien et compteurs des interfaces
- **/api/v2/status/services GET** — Services arrêtés
- **/api/v2/system/version GET** — Version de pfSense
- **/api/v2/system/restapi/version GET** — Facultatif : mises à jour du paquet REST API
- **/api/v2/firewall/states/size GET** — Facultatif : remplissage de la table d'états
- **/api/v2/status/carp GET** — Facultatif : mode maintenance CARP
- **/api/v2/auth/key POST** — Seulement pour créer la clé, puis retirez-le

!!! warning
    Le paquet REST API est un projet communautaire, pas une partie de pfSense : réinstallez-le après chaque mise à niveau de pfSense. pfSense livre un certificat auto-signé : cochez « Accept an unverifiable certificate » sauf si vous l'avez remplacé.

Documentation de l'éditeur : https://pfrest.org/AUTHENTICATION_AND_AUTHORIZATION/

### Pourquoi ces droits et cette commande

Une clé porte exactement les privilèges de l'utilisateur pfSense auquel elle
appartient : l'utilisateur `dumbmonit` ne détient que les privilèges `GET` des
pages que DumbMonit lit, donc la clé peut les lire et ne rien changer. Le
paquet n'offre pas de menu pour créer une clé au nom d'un autre utilisateur :
la clé est donc créée par un seul appel `POST` fait en tant que `dumbmonit` (le
privilège supplémentaire est retiré juste après). Le paquet s'installe par une
commande car le Package Manager de pfSense ne liste que les paquets officiels.

L'API REST ne dit pas si une mise à jour de pfSense est disponible, seulement
si le paquet lui-même en a une.

## Identifiants

| Identifiant | Champs |
|---|---|
| Clé d'API REST | La clé créée pour `dumbmonit` à l'étape 3, envoyée comme `X-API-Key`. |

Adresse : le nom d'hôte ou l'IP de pfSense, `host:port`, ou une URL.
Options :

- **Protocole** (`https`) et **Port** (443) : ceux du webConfigurator,
  réglés sous System > Advanced > Admin Access.
- **Accepter un certificat non vérifiable** : le propre certificat de
  pfSense est autosigné.
- **Délai par requête** (15 s).

## Limites connues

- Les certificats ne sont pas lus : le paquet renvoie leurs clés privées avec eux, et DumbMonit refuse de les demander. Suivez une date d'expiration avec une [surveillance TLS](services.md#tls) sur l'adresse qui sert le certificat.
- Les tunnels VPN (IPsec, OpenVPN, WireGuard) ne sont pas suivis un par un. Leurs services le sont, par « Service pfSense arrêté ».
- Les mises à jour de pfSense ne sont pas signalées : la seule route de l'API qui y touche en lance une.
- La mémoire n'a pas de règle : pfSense compte le cache ZFS comme mémoire utilisée, un chiffre élevé est donc normal. Le swap utilisé est le signe fiable d'un pare-feu à court de mémoire.

## Dépannage

« pfSense refused the API key (401) » : la clé est incorrecte, ou Key n'est
pas cochée parmi les méthodes d'authentification sous System > REST API >
Settings.

« pfSense accepted the API key but refused … (403) » : l'utilisateur
`dumbmonit` n'a pas le privilège de cette page ; le message le nomme.

« … not found: is the REST API package installed? » : le paquet est
manquant, typiquement après une mise à jour de pfSense, qui le supprime.
Réinstallez-le comme à l'étape 1.
