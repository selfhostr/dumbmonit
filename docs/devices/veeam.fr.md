# Veeam Backup & Replication

Un serveur Veeam Backup & Replication via son API REST : jobs dont la
dernière exécution a échoué ou s'est terminée avec un avertissement,
sessions en échec des dernières 24 heures, dépôts qui se remplissent, la
licence, et — par ordinateur ou VM protégé — sa dernière sauvegarde
réussie.

## Ports réseau à ouvrir

Du serveur DumbMonit (ou de son agent relais) vers l'équipement :

| Port | Protocole | Usage |
|---|---|---|
| 9419 | TCP | API REST (HTTPS, par défaut) |

DumbMonit se connecte avec un compte Windows qui détient seulement le rôle
Veeam Backup Viewer (OAuth2 password grant sur `/api/oauth2/token`, port
9419) et garde le jeton pour sa durée de vie de quinze minutes. Chaque
appel porte l'en-tête obligatoire `x-api-version`, `1.1-rev0` par défaut :
la révision qui est arrivée avec la version 12.0.

Les états de job (`/api/v1/jobs/states`) sont la seule lecture requise. Les
autres — `/api/v1/serverInfo`, `/api/v1/sessions` (créées dans les
dernières 24 heures), `/api/v1/backupInfrastructure/repositories/states`,
`/api/v1/license` et `/api/v1/restorePoints` (les points de restauration de
chaque objet protégé, à l'échelle du serveur, les plus récents d'abord) —
sont lues quand le rôle les permet. Veeam réserve certaines routes au rôle
Backup Administrator, la licence parmi elles sur les versions actuelles :
un refus là est montré comme « non lisible », jamais comme une erreur. Un
serveur trop ancien pour connaître `/api/v1/restorePoints` — la route est
apparue après 12.0 — est traité de la même façon : pas d'appareil, pas
d'erreur.

!!! warning "Validé contre la documentation du fournisseur seulement — pas encore testé sur un système réel"

    Cette intégration a été construite à partir de la référence de l'API
    REST de Veeam et du centre d'aide (authentification, versionnage, les
    modèles de job, session, dépôt, licence et point de restauration), pas
    contre un serveur de sauvegarde en fonctionnement. Les routes que le
    rôle Viewer peut lire varient selon la version : dites-nous ce que
    votre serveur refuse.

## Ce qu'il surveille

Toutes les métriques sont préfixées `dumbmonit_veeam_`.

| Métrique | Labels | Signification |
|---|---|---|
| `version_info` | `version` | Valeur 1 : le build, `12.3.1.1139`. |
| `job_enabled`, `job_running` | `job`, `type` | Par job. |
| `job_failed`, `job_warning` | `job`, `type` | 1 quand la dernière exécution d'un job activé a échoué, ou s'est terminée avec un avertissement. Un job désactivé garde son dernier résultat, qui ne dit plus rien : il ne compte pour aucun des deux. |
| `job_result_info` | `job`, `type`, `result` | Valeur 1 : `success`, `warning`, `failed`, `none`. |
| `job_last_run_age_seconds` | `job`, `type` | Depuis la dernière exécution ; absent pour un job qui n'a jamais tourné. |
| `job_progress_percent` | `job`, `type` | Pendant que le job tourne. |
| `jobs`, `jobs_failed`, `jobs_warning`, `jobs_running`, `jobs_disabled` | | Compteurs ; 200 jobs au plus sont décrits un par un. |
| `sessions_24h`, `sessions_failed_24h`, `sessions_warning_24h` | | Sessions créées dans les dernières 24 heures, par résultat. |
| `repository_capacity_bytes`, `repository_free_bytes`, `repository_used_bytes`, `repository_used_percent` | `repository`, `type` | Par dépôt. Un dépôt de stockage objet n'a pas de capacité : seul son espace utilisé est publié. |
| `repository_online` | `repository`, `type` | 1 quand le dépôt est en ligne, 0 quand Veeam ne peut pas le joindre. Seulement à partir de la révision d'API `1.2-rev0` : voir l'option de version d'API. |
| `license_valid` | | 1 quand le statut de licence est `Valid`. |
| `license_info` | `status`, `type`, `edition` | Valeur 1. |
| `license_expiry_seconds`, `support_expiry_seconds` | | Jusqu'à l'expiration de la licence et du contrat de support ; absent pour une licence perpétuelle. |
| `license_instances_licensed`, `license_instances_used` | | Usage de licence par instance. |
| `config_backup_enabled`, `config_backup_encrypted` | | 1 quand la sauvegarde de la configuration de Veeam lui-même est activée ; 1 quand elle est chiffrée. Rôle Backup Administrator seulement. |
| `config_backup_last_success_age_seconds` | | Depuis la dernière sauvegarde de configuration réussie. Rôle Backup Administrator seulement. |
| `section_readable` | `section` : `sessions`, `repositories`, `license`, `restore_points`, `config_backup` | 1 quand le compte peut la lire, 0 quand Veeam a refusé. |

Deux lectures dépendent du serveur et du compte. L'état en ligne d'un dépôt n'existe qu'à partir de la révision d'API `1.2-rev0` : réglez l'option de version d'API sur cette révision, ou sur une plus récente que votre serveur annonce ; sinon la série est absente et « Dépôt Veeam hors ligne » ne peut pas se déclencher. La sauvegarde de configuration, comme la licence, est réservée au rôle Backup Administrator : avec le rôle Viewer, elle apparaît comme « non lisible » et ses deux règles restent muettes.

Les [règles intégrées](../alerting/rules.md#veeam-backup-replication) qui s'appliquent :

| Règle | Quoi | Seuil par défaut | Maintien | Gravité | Rappel |
|---|---|---|---|---|---|
| Job Veeam en échec | La dernière exécution d'un job Veeam activé a échoué : ce qu'il protège n'a pas de nouveau point de restauration (`dumbmonit_veeam_job_failed`). | > 0 | 5 min | Avertissement (`critical`) | 1 j |
| Job Veeam terminé avec un avertissement | La dernière exécution d'un job Veeam activé s'est terminée avec un avertissement : certains objets ne sont peut-être pas protégés (`dumbmonit_veeam_job_warning`). | > 0 | 5 min | Avis (`warning`) | 1 j |
| Sauvegarde Veeam trop ancienne | Aucune exécution réussie d'un job planifié au moins chaque jour depuis plus de deux jours (`dumbmonit_veeam_job_last_success_age_seconds`). | > 2 j | 1 h | Avis (`warning`) | 1 j |
| Dépôt Veeam presque plein | Un dépôt de sauvegarde Veeam est plein à plus de 90 % : les prochains jobs échoueront par manque d'espace (`dumbmonit_veeam_repository_used_percent`). | > 90 % | 30 min | Avis (`warning`) | 1 j |
| Licence Veeam arrivant à expiration | La licence Veeam expire dans moins de 30 jours : les jobs s'arrêtent à la fin de la période de grâce (`dumbmonit_veeam_license_expiry_seconds`). | < 30 j | 1 h | Avis (`warning`) | 1 j |
| Support Veeam arrivant à expiration | Le contrat de support Veeam expire dans moins de 30 jours : après cette date, le serveur ne reçoit plus ni mises à jour ni assistance de l'éditeur (`dumbmonit_veeam_support_expiry_seconds`). | < 30 j | 1 h | Info (`info`) | 24 h |
| Dépôt Veeam hors ligne | Un dépôt de sauvegarde est hors ligne : les jobs qui y écrivent échouent, et ses points de restauration ne peuvent pas être lus. Demande la révision d'API `1.2-rev0` ou une plus récente (`dumbmonit_veeam_repository_online`). | < 1 | 10 min | Avertissement (`critical`) | 6 h |
| Licence Veeam expirée ou invalide | La licence Veeam n'est plus valide : les jobs s'arrêtent une fois la période de grâce terminée. Seulement quand le compte peut lire la licence (`dumbmonit_veeam_license_valid`). | < 1 | 30 min | Avertissement (`critical`) | 24 h |
| Sauvegarde de configuration Veeam désactivée | La sauvegarde de la configuration de Veeam lui-même est désactivée : sans elle, un serveur de sauvegarde perdu doit être reconstruit à la main (`dumbmonit_veeam_config_backup_enabled`). | < 1 | 24 h | Info (`info`) | 7 j |
| Sauvegarde de configuration Veeam trop ancienne | La sauvegarde de configuration est activée mais n'a pas réussi depuis plus de sept jours (`dumbmonit_veeam_config_backup_last_success_age_seconds`). | > 7 j | 1 h | Avis (`warning`) | 24 h |

## Appareils {#devices}

Chaque objet avec au moins un point de restauration — une VM, un
ordinateur physique ou virtuel protégé par agent, un partage de fichiers —
est publié sous la famille générique partagée par chaque intégration
DumbMonit qui suit les appareils protégés —
`dumbmonit_client_device_*`, préfixée sans `veeam`, documentée en entier
sous [Appareils clients](../alerting/rules.md#client-devices) :

| Métrique | Quoi | Labels |
|---|---|---|
| `last_backup_timestamp_seconds` | Le point de restauration le plus récent de l'objet | `device`, `type`, `os`, `user`, `kind` |
| `stale_seconds` | Positif une fois qu'un objet a dépassé `device_stale_days` (3 par défaut) sans nouveau point de restauration ; alimente la règle [Appareil client périmé](../alerting/rules.md#client-devices) | `device`, `type`, `os`, `user`, `kind`, `signal` (toujours `backup`) |

**Limites à connaître** : un job protège ses objets en groupe
(`jobs/states` donne seulement un compteur, `objectsCount`) ; la date par
objet vient de `/api/v1/restorePoints` à la place, lue pour tout le serveur
en un appel. Veeam ne crée un point de restauration qu'une fois une
sauvegarde au moins partiellement réussie, donc le plus récent par objet
est déjà « la dernière sauvegarde réussie » — rien n'est filtré sur le
résultat ici. Veeam n'expose pas de « dernière connexion » séparée de la
sauvegarde elle-même, donc `last_seen_timestamp_seconds` et le signal
`connection` ne sont jamais publiés ; un appareil montre
`last_connection` comme « — » dans le tableau de la page de la surveillance. Le
`type` de l'appareil est le propre nom de plateforme de Veeam (`VSphere`,
`WindowsPhysical`, `HyperV`…), montré tel quel.

## La page de la surveillance

Le panneau Backups dit ce qui ne va pas en une phrase, montre les jobs en
échec, les sessions en échec des dernières 24 heures, les jobs en cours et
la licence, puis liste les jobs activés — les échecs d'abord — avec leur
type et leur dernière exécution, et les dépôts avec leur remplissage et
leur espace libre. Le tableau des appareils protégés (nom, plateforme,
dernière sauvegarde, statut) suit, depuis la famille générique ci-dessus.

## Donner à DumbMonit un compte Veeam Backup Viewer

1. Sur le serveur de sauvegarde, ouvrez Computer Management → Local Users and Groups → Users → New User et créez le compte comme suit, avec un mot de passe long et aléatoire.

    ```
    dumbmonit
    ```

2. Dans la console Veeam, ouvrez le menu principal > Users and Roles > Security, cliquez sur Add, saisissez le compte (par exemple VBR01\dumbmonit) et donnez-lui uniquement le rôle Veeam Backup Viewer.
3. Dans DumbMonit, saisissez l'adresse du serveur de sauvegarde (par exemple vbr.lan) et le compte sous la forme HOST\dumbmonit ou DOMAIN\dumbmonit, avec son mot de passe.
4. Cochez Accepter un certificat non vérifiable sauf si vous avez installé le vôtre : l'API REST (port 9419) utilise un certificat autosigné.

### Droits requis

- **Veeam Backup Viewer** — Lit les jobs, sessions, dépôts et points de restauration

!!! warning
    Veeam réserve certaines routes de l'API REST au rôle Backup Administrator, la licence parmi elles sur les versions actuelles : avec le rôle Viewer, DumbMonit montre la licence comme illisible et ne demande jamais plus.

Documentation du fournisseur : [Requesting authorization](https://helpcenter.veeam.com/docs/backup/vbr_rest/requesting_authorization.html).

### Pourquoi ces permissions

Le compte Windows local n'a besoin d'aucun autre droit sur la machine ; un
compte de domaine convient aussi. Le rôle Viewer suffit pour les jobs, sessions,
dépôts et points de restauration que DumbMonit lit.

## Identifiants

| Identifiant | Champs |
|---|---|
| Nom d'utilisateur et mot de passe | Le compte avec le rôle Veeam Backup Viewer, comme `HOST\dumbmonit` ou `DOMAIN\dumbmonit`. |

Adresse : le nom d'hôte ou l'IP du serveur de sauvegarde, `host:port`, ou
une URL. Options :

- **Protocole** (`https`) et **Port** (9419) : ceux du service d'API REST.
- **Version d'API** (`1.1-rev0`) : envoyée comme `x-api-version`.
  Changez-la seulement si le serveur la refuse ; un serveur v13 documente
  `1.3-rev1`.
- **Accepter un certificat non vérifiable** : le propre certificat de
  l'API REST est autosigné.
- **Délai par requête** (15 s).
- **Péremption d'appareil** (`device_stale_days`, 3 jours par défaut) :
  voir [Appareils](#devices) ci-dessus.

## Limites connues

- Avec le rôle Viewer, la licence et la sauvegarde de configuration ne sont pas lisibles. Leurs règles demandent un compte au rôle Backup Administrator, qui peut aussi modifier le serveur : choisissez en connaissance de cause.
- L'état en ligne des dépôts demande la révision d'API `1.2-rev0` ou une plus récente.
- Les événements de détection de logiciels malveillants ne sont pas lus.
- Au plus 200 jobs sont décrits un par un ; au-delà, seuls les totaux restent.
- Un dépôt de stockage objet n'annonce aucune capacité : « Dépôt Veeam presque plein » ne peut donc pas se déclencher pour lui.

## Dépannage

« Veeam refused the login » : le nom d'utilisateur a besoin de son hôte ou
domaine (`VBR01\dumbmonit`), ou le compte n'a aucun rôle Veeam.

« Veeam refused to list the jobs (403) » : le compte a un rôle qui ne peut
pas lire les jobs ; donnez-lui Veeam Backup Viewer.

« Veeam refused the API version » : réglez l'option de version d'API sur
une que le serveur liste sur `https://<server>:9419/swagger`.

La licence montre « Not readable » : attendu avec le rôle Viewer sur les
versions actuelles ; DumbMonit n'en demande pas plus.

Aucun appareil, ou le panneau Appareils est vide : `restore_points` lit
« Not readable », probablement parce que le serveur est plus ancien que la
route (tout avant les révisions ultérieures de 12.0) plutôt qu'un problème
de rôle — les points de restauration sont un inventaire ordinaire, pas une
page réservée aux administrateurs.
