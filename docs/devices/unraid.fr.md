# Unraid

Un serveur Unraid via son API GraphQL : un array qui n'est pas démarré, des
disques désactivés, manquants ou rapportant des erreurs de lecture, des
vérifications de parité qui ont trouvé des erreurs ou n'ont pas tourné
depuis des semaines, des pools de cache qui se remplissent, des conteneurs
qui devraient tourner et ne tournent pas, et l'état des VM.

## Ports réseau à ouvrir

Du serveur DumbMonit (ou de son agent relais) vers l'équipement :

| Port | Protocole | Usage |
|---|---|---|
| 80 | TCP | Interface web et API GraphQL (HTTP, par défaut) |
| 443 | TCP | Seulement avec l'option de protocole HTTPS |

DumbMonit envoie une clé d'API avec le rôle Viewer dans l'en-tête
`x-api-key` à `/graphql`. Le rôle Viewer lit tout et ne peut rien changer.
L'API est intégrée à Unraid depuis la 7.2 ; sur les versions 6.12 à 7.1 elle
vient avec le plugin Unraid Connect.

Chaque minute, DumbMonit envoie une requête obligatoire (version, array,
parité, disques de données et de cache) et quatre optionnelles, chacune à
part pour qu'un champ qu'une version plus ancienne de l'API n'a pas, ou des
VM éteintes, ne laisse de côté que cette partie : la vérification de parité,
les conteneurs Docker, les VM et les notifications non lues.

!!! warning "Validé contre la documentation du fournisseur seulement — pas encore testé sur un système réel"

    Cette intégration a été construite à partir de la documentation de l'API
    d'Unraid et du schéma GraphQL publié de l'API Unraid (noms de champs,
    énumérations, tailles en kilooctets), pas contre un serveur en
    fonctionnement. Chaque requête est une lecture. Dites-nous ce qui casse.

## Ce qu'il surveille

Toutes les métriques sont préfixées `dumbmonit_unraid_`.

| Métrique | Labels | Signification |
|---|---|---|
| `version_info` | `version` | Valeur 1 : la version d'Unraid. |
| `array_started` | | 1 quand l'array est démarré. |
| `array_state_info` | `state` | Valeur 1 : `started`, `stopped`, `new_array`, `recon_disk`, `disable_disk`… |
| `array_size_bytes`, `array_used_bytes`, `array_free_bytes`, `array_used_percent` | | Capacité de l'array. |
| `disk_ok` | `disk`, `role` (`parity`, `data`, `cache`) | 1 quand le statut du disque est `DISK_OK`. Les emplacements vides sont laissés de côté. |
| `disk_status_info` | `disk`, `role`, `status` | Valeur 1 : le statut d'Unraid (`disk_ok`, `disk_dsbl`, `disk_np_missing`, `disk_invalid`…). |
| `disk_errors` | `disk`, `role` | Erreurs de lecture depuis le démarrage de l'array (la colonne Errors). |
| `disk_temperature_celsius` | `disk`, `role` | Seulement tant que le disque tourne : Unraid n'en rapporte aucune pour un disque endormi. |
| `disk_spinning` | `disk`, `role` | 1 quand le disque tourne. |
| `disk_size_bytes`, `disk_fs_size_bytes`, `disk_used_percent` | `disk`, `role` | Taille, taille du système de fichiers et remplissage, pour les disques avec un système de fichiers. |
| `disks`, `disks_problem`, `disks_with_errors` | | Compteurs. |
| `parity_check_status_info` | `status` | Valeur 1 : `completed`, `running`, `paused`, `cancelled`, `failed`, `never_run`. |
| `parity_check_running`, `parity_check_progress_percent` | | Une vérification en cours. |
| `parity_check_ok` | | Après une vérification terminée : 1 sans erreurs de synchro, 0 avec des erreurs ou en cas d'échec. Une vérification annulée ne prouve rien et ne fixe rien. |
| `parity_check_errors`, `parity_check_age_seconds`, `parity_check_duration_seconds` | | La dernière vérification. |
| `containers`, `containers_running`, `containers_autostart_stopped` | | Conteneurs Docker. |
| `container_running`, `container_autostart_stopped` | `container` | Par conteneur ; le second est 1 quand un conteneur réglé pour démarrer avec l'array ne tourne pas. |
| `vms`, `vms_running` | | VM. |
| `vm_state_info`, `vm_crashed` | `vm` (et `state`) | Par VM. |
| `notifications_unread` | `importance` : `alert`, `warning`, `info` | Notifications non lues. |
| `registration_ok` | | 1 quand Unraid ne signale aucun problème de licence, 0 quand l'état de la licence est une erreur (essai expiré, clé qui ne correspond pas à la clé USB, fichier de clé manquant…). |
| `registration_info` | `state`, `type` | Valeur 1 : l'état de la licence tel qu'Unraid le donne (`trial`, `pro`, `eexpired`, `eguid`…) et le type de licence. |
| `trial_expiry_seconds` | | Jusqu'à la fin de la licence d'essai ; seulement pendant un essai. |
| `os_updates_expiry_seconds` | | Jusqu'à la fin de l'éligibilité de la licence aux mises à jour d'Unraid OS ; absente quand la licence n'a pas de telle date. |
| `uptime_seconds` | | Depuis le démarrage du serveur. |
| `cpu_usage_percent` | | Charge du processeur, tous cœurs confondus. |
| `memory_total_bytes`, `memory_available_bytes`, `memory_used_percent` | | Mémoire. Est comptée comme utilisée la mémoire qui n'est pas disponible : le cache qu'Unraid peut rendre n'est pas compté. |
| `swap_used_percent` | | Seulement quand le serveur a du swap. |

La licence et la charge du système (durée de fonctionnement, processeur, mémoire) viennent de deux requêtes facultatives de plus. Une version plus ancienne de l'API qui ne les connaît pas laisse seulement ces lignes de côté : le reste de la surveillance n'est pas touché.

Les [règles intégrées](../alerting/rules.md#unraid) qui s'appliquent :

| Règle | Quoi | Seuil par défaut | Maintien | Gravité | Rappel |
|---|---|---|---|---|---|
| Array Unraid arrêté | L'array Unraid n'est pas démarré : partages, conteneurs et VM qui y vivent sont indisponibles (`dumbmonit_unraid_array_started`). | < 1 | 10 min | Avertissement (`critical`) | 6 h |
| Disque Unraid désactivé ou manquant | Un disque Unraid est désactivé, manquant ou invalide : l'array tourne en mode dégradé, et une panne de plus perd des données (`dumbmonit_unraid_disk_ok`). | < 1 | 5 min | Avertissement (`critical`) | 6 h |
| Erreurs de lecture disque Unraid | Un disque Unraid a rapporté des erreurs de lecture depuis le démarrage de l'array : il sera peut-être désactivé à la prochaine (`dumbmonit_unraid_disk_errors`). | > 0 | 5 min | Avis (`warning`) | 1 j |
| Disque Unraid trop chaud | Un disque de données ou de parité Unraid est resté au-dessus de 55 °C pendant quinze minutes (`dumbmonit_unraid_disk_temperature_celsius{role=~"data\|parity"}`). | > 55 °C | 15 min | Avis (`warning`) | 6 h |
| Vérification de parité Unraid a trouvé des erreurs | La dernière vérification de parité Unraid a échoué ou a trouvé des erreurs de synchro : la parité ne correspond plus aux données (`dumbmonit_unraid_parity_check_ok`). | < 1 | 5 min | Avis (`warning`) | 1 j |
| Vérification de parité Unraid en retard | La dernière vérification de parité Unraid a plus de 40 jours : une vérification mensuelle a été manquée (`dumbmonit_unraid_parity_check_age_seconds`). | > 40 j | 1 h | Avis (`warning`) | 1 j |
| Cache Unraid presque plein | Un pool de cache Unraid est plein à plus de 90 % : les écritures se replient sur l'array lent, ou échouent (`dumbmonit_unraid_disk_used_percent{role="cache"}`). | > 90 % | 30 min | Avis (`warning`) | 1 j |
| Conteneur Unraid arrêté | Un conteneur Docker réglé pour démarrer avec l'array ne tourne pas (`dumbmonit_unraid_container_autostart_stopped`). | > 0 | 10 min | Avis (`warning`) | 6 h |
| VM Unraid plantée | Unraid signale une machine virtuelle comme plantée (`dumbmonit_unraid_vm_crashed`). | > 0 | 5 min | Avis (`warning`) | 6 h |
| Alerte Unraid non lue | Unraid a levé une notification de la plus haute importance (alert) que personne n'a lue : disque en panne, surchauffe, erreur de l'array… (`dumbmonit_unraid_notifications_unread{importance="alert"}`). | > 0 | 10 min | Avis (`warning`) | 24 h |
| Array Unraid presque plein | L'array est plein à plus de 95 % : les écritures sur les partages vont bientôt échouer. Se lève sous 93 % (`dumbmonit_unraid_array_used_percent`). | > 95 % | 1 h | Avis (`warning`) | 24 h |
| Mémoire Unraid presque épuisée | Plus de 95 % de la mémoire est utilisée depuis quinze minutes : Unraid fonctionne en mémoire, et des conteneurs ou des VM sont tués quand elle vient à manquer. Se lève sous 90 % (`dumbmonit_unraid_memory_used_percent`). | > 95 % | 15 min | Avis (`warning`) | 6 h |
| Problème de licence Unraid | Unraid signale un problème de licence (essai expiré, clé qui ne correspond pas à la clé USB, fichier de clé manquant…) : l'array risque de refuser de démarrer après le prochain arrêt (`dumbmonit_unraid_registration_ok`). | < 1 | 30 min | Avis (`warning`) | 24 h |
| Essai Unraid bientôt terminé | La licence d'essai se termine dans moins de trois jours : après, l'array ne démarre plus (`dumbmonit_unraid_trial_expiry_seconds`). | < 3 j | 1 h | Avis (`warning`) | 24 h |
| Fin d'éligibilité aux mises à jour d'Unraid OS | La licence cesse d'être éligible aux mises à jour d'Unraid OS dans moins de 30 jours. Le serveur continue de fonctionner ; il ne reçoit simplement plus les nouvelles versions (`dumbmonit_unraid_os_updates_expiry_seconds`). | < 30 j | 1 h | Info (`info`) | 7 j |

## La page de la surveillance

Le panneau Array dit ce qui ne va pas en une phrase, montre le remplissage de
l'array, la dernière vérification de parité, les conteneurs et VM en cours,
puis liste chaque disque — parité d'abord, puis données, puis cache — avec
son statut, sa température et son remplissage, et les conteneurs qui
devraient tourner.

## Créer une clé d'API en lecture seule pour DumbMonit

1. Sur Unraid 6.12 à 7.1, installez d'abord le plugin Unraid Connect depuis l'onglet Apps. Unraid 7.2 ou plus récent intègre l'API.
2. Ouvrez Settings → Management Access → API Keys et créez une clé nommée comme suit, avec le rôle Viewer.

    ```
    dumbmonit
    ```

3. Copiez la clé : Unraid ne la montre qu'une fois.
4. Dans DumbMonit, saisissez l'adresse du serveur Unraid (par exemple tower.lan), collez la clé comme clé d'API et choisissez https si l'interface web est servie en HTTPS.

### Droits requis

- **Rôle Viewer** — Lit tout, ne change rien

!!! warning
    Unraid ne donne aucune température pour un disque en veille, et DumbMonit n'en réveille jamais un : un disque endormi n'a simplement pas de température avant de tourner.

Documentation du fournisseur : [Using the Unraid API](https://docs.unraid.net/API/how-to-use-the-api/).

### Pourquoi ces permissions

Le rôle Viewer lit tout et ne peut rien changer. Aucune connexion à Unraid
Connect n'est nécessaire en usage local.

## Identifiants

| Identifiant | Champs |
|---|---|
| Clé d'API | Une clé avec le rôle Viewer, envoyée comme `x-api-key`. |

Adresse : le nom d'hôte ou l'IP du serveur Unraid, `host:port`, ou une URL.
Options :

- **Protocole** (`http`) et **Port** (80) : ceux de l'interface web. Avec
  SSL réglé sur Strict sous Settings > Management Access, utilisez `https`,
  et l'adresse `myunraid.net` si c'est celle du certificat.
- **Accepter un certificat non vérifiable** : pour un certificat autosigné.
- **Délai par requête** (15 s).

## Limites connues

- L'onduleur n'est pas lu : branchez-le plutôt via [NUT](nut.md).
- Pas de détail SMART : DumbMonit lit l'état, les erreurs et la température qu'Unraid donne pour chaque disque, pas les attributs SMART.
- DumbMonit ne lit pas si un conteneur a une mise à jour d'image en attente.
- Les chiffres de licence, de processeur et de mémoire demandent une version récente de l'API Unraid. Sur une version qui ne les a pas, ces lignes et leurs règles sont simplement absentes.

## Dépannage

« Unraid refused the API key (401) » : la clé est incorrecte ou a été
supprimée.

« Unraid accepted the API key but refused to answer (403) » : la clé n'a
aucun rôle, ou un rôle plus restreint que Viewer.

Aucune donnée de conteneur, VM ou parité : cette requête optionnelle a
échoué (VM éteintes, une version d'API plus ancienne sans le champ) ; le
reste est quand même lu. La raison est dans le journal du serveur au niveau
debug.
