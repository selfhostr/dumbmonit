# VMware vSphere

Un vCenter Server e gli host che gestisce, o un host ESXi autonomo: host
che smettono di rispondere, rimangono in modalità manutenzione o segnalano una salute rossa,
macchine virtuali e i loro VMware Tools, datastore che si riempiono o diventano
inaccessibili, e gli allarmi che vSphere stesso ha generato.

DumbMonit legge vSphere con un utente dedicato che detiene il ruolo integrato
**Read-only**. Non spegne mai una VM, non entra mai in modalità manutenzione e mai
riconosce un allarme.

Il guasto che vale la pena osservare: **un datastore che si riempie**. Quando un
datastore con thin provisioning raggiunge il 100%, ogni VM che ha bisogno di scrivere su di esso
viene messa in pausa all'istante. Gli allarmi che vCenter genera lo dicono anche, ma solo a
chiunque abbia il client aperto.

## Perché l'API Web Services

DumbMonit parla all'API Web Services di vSphere (SOAP, `/sdk`), non all'API REST
(`/api/vcenter/*`): un host ESXi autonomo non ha API REST, e su un
vCenter l'API REST non dice la modalità manutenzione, la salute complessiva
(`overallStatus`), lo stato di VMware Tools o gli allarmi attivati. Una
vista dell'intero inventario viene letta in un singolo passaggio, pagina per pagina, e
rilasciata alla fine.

La sessione viene mantenuta da una sonda all'altra e riaprerte solo quando
scade: vCenter non registra un nuovo login ogni minuto.

## Cosa monitora

Tutte le metriche hanno il prefisso `dumbmonit_vsphere_`. I valori di salute seguono il
`overallStatus` di vSphere: 0 verde, 1 giallo, 2 rosso, 3 grigio (sconosciuto).

| Metrica | Cosa | Etichette |
|---|---|---|
| `info` | valore 1 | `product` (`vcenter` o `esxi`), `version`, `build` |
| `hosts` | host per stato di connessione: `connected`, `disconnected`, `notResponding` | `state` |
| `host_connection_state` | 0 connesso, 1 disconnesso da un amministratore, 2 non rispondente | `esxi` |
| `host_power_state` | 0 acceso, 1 standby, 2 spento, 3 sconosciuto | `esxi` |
| `host_maintenance` | 1 in modalità manutenzione | `esxi` |
| `host_status` | salute complessiva | `esxi` |
| `host_cpu_percent`, `host_memory_percent`, `host_memory_bytes`, `host_uptime_seconds` | solo host connessi | `esxi` |
| `vms` | VM per stato di alimentazione: `poweredOn`, `poweredOff`, `suspended` (template esclusi) | `state` |
| `templates` | template di VM | |
| `vm_power_state` | 0 accesa, 1 spenta, 2 sospesa; al massimo 1000 VM | `vm` |
| `vm_status` | salute complessiva | `vm` |
| `vm_tools_status` | solo VM accese: 0 aggiornate, 1 versione obsoleta, 2 installate ma non in esecuzione, 3 non installate | `vm` |
| `vm_snapshots` | numero di snapshot della VM; solo per una VM che ne ha almeno uno | `vm` |
| `vm_snapshot_oldest_age_seconds` | età dello snapshot più vecchio della VM; solo per una VM che ne ha almeno uno | `vm` |
| `vm_consolidation_needed` | 1 quando vSphere segnala che i dischi della VM hanno bisogno di un consolidamento | `vm` |
| `datastores` | datastore | |
| `datastore_capacity_bytes`, `datastore_free_bytes`, `datastore_used_percent` | | `datastore` |
| `datastore_accessible` | 1 accessibile, 0 inaccessibile | `datastore` |
| `datastore_status` | salute complessiva | `datastore` |
| `alarms` | allarmi attivati non riconosciuti, per colore: `red`, `yellow` | `status` |
| `alarms_acknowledged` | allarmi attivati già riconosciuti | |
| `alarm` | una serie per allarme non riconosciuto, valore = il suo codice di salute, al massimo 64 | `alarm` (il suo nome), `entity`, `entity_type`, `status` |
| `scrape_errors` | chiamate che hanno fallito durante l'ultimo probe | |

Gli snapshot vengono letti dall'albero degli snapshot di ogni VM, nello stesso passaggio del resto dell'inventario. DumbMonit conserva il loro numero e la data del più vecchio, mai i loro nomi. Una VM senza snapshot non ha nessuna serie di snapshot.

Le [regole integrate](../alerting/rules.md#vmware-vsphere) che si applicano:

| Regola | Cosa | Soglia predefinita | Mantenimento | Gravità | Promemoria |
|---|---|---|---|---|---|
| Host vSphere non risponde | Un host vSphere non risponde a vCenter (`dumbmonit_vsphere_host_connection_state`: 2 non risponde). Un host disconnesso di proposito (1) non scatta. | ≥ 2 | 3 min | Avvertenza (`critical`) | 1 h |
| Salute dell'host vSphere rossa | vSphere segnala la salute complessiva di un host come rossa (`dumbmonit_vsphere_host_status == 2`). Grigio significa sconosciuto e non scatta. | > 0 | 5 min | Avvertenza (`critical`) | 1 h |
| Host vSphere lasciato in manutenzione | Un host vSphere è in modalità manutenzione da un giorno: le sue VM sono state spostate ed è stato forse dimenticato lì (`dumbmonit_vsphere_host_maintenance`). | > 0 | 24 h | Informazioni (`info`) | 24 h |
| Datastore vSphere quasi pieno | Un datastore vSphere è pieno per il 90% o più (`dumbmonit_vsphere_datastore_used_percent`); rientra sotto l'85 %. | ≥ 90 % | 15 min | Avviso (`warning`), escalation dopo 24 h | 6 h |
| Datastore vSphere inaccessibile | Un datastore vSphere è inaccessibile: le VM che vi sono memorizzate non possono funzionare (`dumbmonit_vsphere_datastore_accessible`). | < 1 | 5 min | Avvertenza (`critical`) | 1 h |
| VMware Tools non in esecuzione in una VM | VMware Tools è installato ma non in esecuzione in una VM accesa: il guest potrebbe essere bloccato (`dumbmonit_vsphere_vm_tools_status == 2`). Le VM senza Tools non scattano. | > 0 | 30 min | Informazioni (`info`) | 24 h |
| Allarme rosso vSphere | vSphere ha sollevato un allarme rosso che nessuno ha riconosciuto (`dumbmonit_vsphere_alarm{status="red"}`, una serie per allarme, con il nome dell'allarme e dell'oggetto). | > 0 | 5 min | Avviso (`warning`) | 6 h |
| Snapshot di VM vSphere troppo vecchio | Lo snapshot più vecchio di una VM ha più di tre giorni: gli snapshot crescono, rallentano la VM e possono riempire il datastore. VMware raccomanda di non tenerne nessuno oltre le 72 ore (`dumbmonit_vsphere_vm_snapshot_oldest_age_seconds`). | > 3 d | 1 h | Avviso (`warning`) | 24 h |
| Dischi di VM vSphere da consolidare | vSphere segnala che i dischi di una VM hanno bisogno di un consolidamento: la rimozione di uno snapshot ha lasciato dei file delta, e continuano a crescere (`dumbmonit_vsphere_vm_consolidation_needed`). | > 0 | 1 h | Avviso (`warning`) | 24 h |

## La pagina del monitor

Il pannello sopra i grafici legge quello che la sonda ha memorizzato; aprire la pagina non
interroga mai vSphere. Dice per primo cosa è sbagliato (host non rispondenti, allarmi
rossi, datastore quasi pieni o inaccessibili), quindi gli host con il loro
carico, i datastore con il loro livello di riempimento, e le VM per stato di alimentazione.

## Come è stato convalidato

Contro il simulatore govmomi `vcsim` 0.56.0, in modalità vCenter ed ESXi:
accesso e password rifiutata, riutilizzo della sessione e scadenza, paging attraverso l'inventario,
un host non rispondente, un host in modalità manutenzione, una VM spenta
e un template. Il simulatore non genera allarmi: la lettura degli allarmi attivati è
convalidata rispetto alla struttura `AlarmState` documentata dell'API Web Services.

## Crea un utente di sola lettura in vSphere

1. Su un vCenter: Administration → Single Sign On → Users and Groups → Users, dominio vsphere.local → Add. Denomina l'utente come segue e assegnagli una lunga password casuale.

    ```
    dumbmonit
    ```

2. Ancora su vCenter: Administration → Access Control → Global Permissions → Add. Scegli quell'utente, il ruolo Read-only, e spunta Propagate to children. Read-only vede ogni host, VM, datastore e allarme, e non può cambiare nulla.

3. Su un host ESXi autonomo invece: nel client dell'host, Manage → Security & users → Users → Add user, con lo stesso nome. Poi Host → Actions → Permissions → Add user: sceglilo, il ruolo Read-only, e spunta Propagate to all children.

4. In DumbMonit, inserisci l'indirizzo del vCenter o dell'host, ad esempio "vcenter.lan" o "esxi1.lan", con il nome utente come creato: dumbmonit@vsphere.local su un vCenter, dumbmonit su un host. Quando un vCenter gestisce gli host, aggiungi solo il vCenter: segnala ogni host, VM e datastore.

5. DumbMonit legge solo, attraverso l'API Web Services di vSphere (/sdk). Non spegne mai una VM, non entra mai in modalità manutenzione e mai riconosce un allarme.

!!! warning
    Non riutilizzare l'account del dominio di single sign-on o l'account integrato dell'host: il ruolo Read-only sopra è tutto quello di cui DumbMonit ha bisogno. vCenter ed ESXi usano un certificato autofirmato per impostazione predefinita: abilita "Accetta un certificato non verificabile" a meno che l'host DumbMonit non si fidi della loro autorità certificativa.

## Credenziali

| Credenziale | Campi |
|---|---|
| Nome utente / password | L'utente dedicato con il ruolo Read-only: `dumbmonit@vsphere.local` su un vCenter, `dumbmonit` su un host. |

Indirizzo: un nome host o IP (`vcenter.lan`), `host:port`, o un URL completo; un
`/sdk` o `/ui` finale è accettato. Opzioni: porta (443), verifica certificato,
timeout delle richieste (20 s) e allarmi attivati (lettura).

## Limiti noti

- La scadenza delle licenze non viene letta: il ruolo di sola lettura non vede le licenze.
- Al massimo 1000 VM e 64 allarmi vengono descritti uno per uno.
- Nessun contatore di prestazioni per VM (processore, memoria, latenza del disco): DumbMonit legge l'inventario e i suoi stati.
- Ventole, alimentatori e temperature si vedono solo attraverso la salute complessiva di un host e gli allarmi che vSphere solleva.
- Gli snapshot e il consolidamento sono stati costruiti in base alla struttura documentata dell'API e non sono ancora stati verificati su un vCenter reale.
