# VMware vSphere

Un vCenter Server et les hôtes qu'il gère, ou un hôte ESXi autonome : hôtes
qui arrêtent de répondre, sont laissés en mode maintenance ou rapportent une
santé rouge, machines virtuelles et leurs VMware Tools, datastores qui se
remplissent ou deviennent inaccessibles, et les alarmes que vSphere lui-même
a levées.

## Ports réseau à ouvrir

Du serveur DumbMonit (ou de son agent relais) vers l'équipement :

| Port | Protocole | Usage |
|---|---|---|
| 443 | TCP | API vSphere (HTTPS, par défaut) |

DumbMonit lit vSphere avec un utilisateur dédié portant le rôle intégré
**Read-only**. Il n'allume ni n'éteint jamais une VM, n'entre jamais en mode
maintenance et n'acquitte jamais une alarme.

La panne à surveiller : **un datastore qui se remplit**. Quand un datastore
en provisionnement fin atteint 100 %, chaque VM qui a besoin d'y écrire est
mise en pause d'un coup. Les alarmes que vCenter lève le disent aussi, mais
seulement à qui a le client ouvert.

## Pourquoi la Web Services API

DumbMonit parle à la Web Services API de vSphere (SOAP, `/sdk`), pas à l'API
REST (`/api/vcenter/*`) : un hôte ESXi autonome n'a pas d'API REST, et sur un
vCenter l'API REST ne dit pas le mode maintenance, la santé globale
(`overallStatus`), l'état des VMware Tools ou les alarmes déclenchées. Une vue
de tout l'inventaire est lue en une seule passe, page par page, et libérée à
la fin.

La session est gardée d'une sonde à l'autre et rouverte seulement quand elle
expire : vCenter n'enregistre pas une nouvelle connexion chaque minute.

## Ce qu'il surveille

Toutes les métriques sont préfixées `dumbmonit_vsphere_`. Les valeurs de
santé suivent le `overallStatus` de vSphere : 0 vert, 1 jaune, 2 rouge, 3 gris
(inconnu).

| Métrique | Quoi | Labels |
|---|---|---|
| `info` | valeur 1 | `product` (`vcenter` ou `esxi`), `version`, `build` |
| `hosts` | hôtes par état de connexion : `connected`, `disconnected`, `notResponding` | `state` |
| `host_connection_state` | 0 connecté, 1 déconnecté par un administrateur, 2 ne répond pas | `host` |
| `host_power_state` | 0 allumé, 1 veille, 2 éteint, 3 inconnu | `host` |
| `host_maintenance` | 1 en mode maintenance | `host` |
| `host_status` | santé globale | `host` |
| `host_cpu_percent`, `host_memory_percent`, `host_memory_bytes`, `host_uptime_seconds` | hôtes connectés seulement | `host` |
| `vms` | VM par état d'alimentation : `poweredOn`, `poweredOff`, `suspended` (templates exclus) | `state` |
| `templates` | templates de VM | |
| `vm_power_state` | 0 allumée, 1 éteinte, 2 suspendue ; 1000 VM au plus | `vm` |
| `vm_status` | santé globale | `vm` |
| `vm_tools_status` | VM allumées seulement : 0 à jour, 1 vieille version, 2 installé mais pas lancé, 3 pas installé | `vm` |
| `vm_snapshots` | nombre de snapshots de la VM ; seulement pour une VM qui en a au moins un | `vm` |
| `vm_snapshot_oldest_age_seconds` | âge du plus ancien snapshot de la VM ; seulement pour une VM qui en a au moins un | `vm` |
| `vm_consolidation_needed` | 1 quand vSphere signale que les disques de la VM ont besoin d'une consolidation | `vm` |
| `vm_ip_info` | VM allumées seulement, toujours 1 : l'adresse principale que rapporte VMware Tools (`guest.ipAddress`, le rôle Read-only suffit ; aucune sans Tools, boucle locale et lien local ignorés). La carte dessine sous la VM les surveillances visées à cette adresse | `vm`, `ip` |
| `datastores` | datastores | |
| `datastore_capacity_bytes`, `datastore_free_bytes`, `datastore_used_percent` | | `datastore` |
| `datastore_accessible` | 1 accessible, 0 inaccessible | `datastore` |
| `datastore_status` | santé globale | `datastore` |
| `alarms` | alarmes déclenchées non acquittées, par couleur : `red`, `yellow` | `status` |
| `alarms_acknowledged` | alarmes déclenchées déjà acquittées | |
| `alarm` | une série par alarme non acquittée, valeur = son code de santé, 64 au plus | `alarm` (son nom), `entity`, `entity_type`, `status` |
| `scrape_errors` | appels qui ont échoué à la dernière sonde | |

Les snapshots sont lus dans l'arbre de snapshots de chaque VM, dans la même passe que le reste de l'inventaire. DumbMonit garde leur nombre et la date du plus ancien, jamais leurs noms. Une VM sans snapshot n'a aucune série de snapshot.

Les [règles intégrées](../alerting/rules.md#vmware-vsphere) qui s'appliquent :

| Règle | Quoi | Seuil par défaut | Maintien | Gravité | Rappel |
|---|---|---|---|---|---|
| Hôte vSphere ne répond pas | Un hôte vSphere ne répond pas à vCenter (`dumbmonit_vsphere_host_connection_state` : 2 ne répond pas). Un hôte déconnecté volontairement (1) ne se déclenche pas. | ≥ 2 | 3 min | Avertissement (`critical`) | 1 h |
| Santé d'hôte vSphere rouge | vSphere rapporte la santé globale d'un hôte comme rouge (`dumbmonit_vsphere_host_status == 2`). Gris signifie inconnu et ne se déclenche pas. | > 0 | 5 min | Avertissement (`critical`) | 1 h |
| Hôte vSphere laissé en maintenance | Un hôte vSphere est en mode maintenance depuis un jour : ses VM ont été déplacées et il a peut-être été oublié là (`dumbmonit_vsphere_host_maintenance`). | > 0 | 24 h | Info (`info`) | 24 h |
| Datastore vSphere presque plein | Un datastore vSphere est plein à 90 % ou plus (`dumbmonit_vsphere_datastore_used_percent`) ; se résout sous 85 %. | ≥ 90 % | 15 min | Avis (`warning`), escalade après 24 h | 6 h |
| Datastore vSphere inaccessible | Un datastore vSphere est inaccessible : les VM stockées sur lui ne peuvent pas tourner (`dumbmonit_vsphere_datastore_accessible`). | < 1 | 5 min | Avertissement (`critical`) | 1 h |
| VMware Tools vSphere non lancés | VMware Tools est installé mais ne tourne pas dans une VM allumée : l'invité est peut-être bloqué (`dumbmonit_vsphere_vm_tools_status == 2`). Les VM sans Tools ne se déclenchent pas. | > 0 | 30 min | Info (`info`) | 24 h |
| Alarme rouge vSphere | vSphere a levé une alarme rouge que personne n'a acquittée (`dumbmonit_vsphere_alarm{status="red"}`, une série par alarme, nommée d'après l'alarme et l'objet). | > 0 | 5 min | Avis (`warning`) | 6 h |
| Snapshot de VM vSphere trop ancien | Le plus ancien snapshot d'une VM a plus de trois jours : les snapshots grossissent, ralentissent la VM et peuvent remplir le datastore. VMware recommande de n'en garder aucun au-delà de 72 heures (`dumbmonit_vsphere_vm_snapshot_oldest_age_seconds`). | > 3 j | 1 h | Avis (`warning`) | 24 h |
| Disques de VM vSphere à consolider | vSphere signale que les disques d'une VM ont besoin d'une consolidation : la suppression d'un snapshot a laissé des fichiers delta, et ils continuent de grossir (`dumbmonit_vsphere_vm_consolidation_needed`). | > 0 | 1 h | Avis (`warning`) | 24 h |

## La page de la surveillance

Le panneau au-dessus des graphiques lit ce que la sonde a stocké ; ouvrir la
page n'interroge jamais vSphere. Il dit d'abord ce qui ne va pas (hôtes qui
ne répondent pas, alarmes rouges, datastores presque pleins ou
inaccessibles), puis les hôtes avec leur charge, les datastores avec leur
niveau de remplissage, et les VM par état d'alimentation.

## Comment cela a été validé

Contre le simulateur govmomi `vcsim` 0.56.0, en mode vCenter et en mode ESXi :
connexion et mot de passe refusé, réutilisation et expiration de session,
pagination dans l'inventaire, un hôte qui ne répond pas, un hôte en mode
maintenance, une VM éteinte et un template. Le simulateur ne lève aucune
alarme : lire les alarmes déclenchées est validé contre la structure
`AlarmState` documentée de la Web Services API.

## Créer un utilisateur en lecture seule dans vSphere

1. Sur un vCenter, ouvrez Administration → Single Sign On → Users and Groups → Users, choisissez le domaine vsphere.local, cliquez sur Add et saisissez ce nom d'utilisateur avec un long mot de passe aléatoire.

    ```
    dumbmonit
    ```

2. Ouvrez Administration → Access Control → Global Permissions → Add : choisissez cet utilisateur, le rôle Read-only, cochez Propagate to children.

3. Sur un hôte ESXi autonome, à la place : dans le host client, Manage → Security & users → Users → Add user dumbmonit, puis Host → Actions → Permissions → Add user avec le rôle Read-only.

4. Dans DumbMonit, saisissez l'adresse du vCenter ou de l'hôte, avec l'utilisateur dumbmonit@vsphere.local sur un vCenter ou dumbmonit sur un hôte. N'ajoutez que le vCenter quand il gère les hôtes.

### Droits requis

- **Read-only, propagé aux enfants** : Voit les hôtes, VM, datastores et alarmes

!!! warning
    vCenter et ESXi utilisent par défaut un certificat autosigné : activez « Accepter un certificat non vérifiable » sauf si l'hôte de DumbMonit fait confiance à leur autorité de certification.

Documentation de l'éditeur : [vSphere global permissions](https://docs.vmware.com/en/VMware-vSphere/8.0/vsphere-security/GUID-74F53189-EF41-4AC1-A78E-D25621855800.html)

### Pourquoi ces droits

Ne réutilisez pas le compte du domaine Single Sign On ni le compte intégré de
l'hôte : le rôle `Read-only` suffit à DumbMonit. Il voit chaque hôte, VM,
datastore et alarme, et ne peut rien modifier.

Quand un vCenter gère les hôtes, n'ajoutez que le vCenter : il remonte tous
les hôtes, VM et datastores.

DumbMonit ne fait que lire, via l'API Web Services de vSphere (`/sdk`). Il
n'allume ni n'éteint jamais une VM, n'active jamais le mode maintenance et
n'acquitte jamais une alarme.

## Identifiants

| Identifiant | Champs |
|---|---|
| Nom d'utilisateur / mot de passe | L'utilisateur dédié avec le rôle Read-only : `dumbmonit@vsphere.local` sur un vCenter, `dumbmonit` sur un hôte. |

Adresse : un nom d'hôte ou une IP (`vcenter.lan`), `host:port`, ou une URL
complète ; un `/sdk` ou `/ui` final est accepté. Options : port (443),
vérification du certificat, délai de requête (20 s) et alarmes déclenchées
(lues).

## Limites connues

- L'expiration des licences n'est pas lue : le rôle Lecture seule ne voit pas les licences.
- Au plus 1000 VM et 64 alarmes sont décrites une par une.
- Pas de compteurs de performance par VM (processeur, mémoire, latence disque) : DumbMonit lit l'inventaire et ses états.
- Les ventilateurs, les alimentations et les températures ne sont vus qu'à travers la santé globale d'un hôte et les alarmes que vSphere lève.
- Les snapshots et la consolidation ont été construits d'après la structure documentée de l'API et n'ont pas encore été vérifiés sur un vrai vCenter.
