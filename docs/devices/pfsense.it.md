# pfSense

Un firewall pfSense tramite il pacchetto API REST della community
(`pfSense-pkg-RESTAPI`, versione 2): gateway inattivi, che perdono
pacchetti o rispondono lentamente, interfacce abilitate senza link,
servizi abilitati che si sono fermati, e CPU, memoria, disco e
temperatura.

DumbMonit invia una chiave API nell'header `X-API-Key`. Una chiave porta
esattamente i privilegi dell'utente pfSense a cui appartiene: l'utente
`dumbmonit` sotto possiede solo i privilegi `GET` delle cinque pagine che
DumbMonit legge, quindi la chiave può leggere quelle pagine e non
modificare nulla. Ogni minuto DumbMonit legge, in parallelo,
`/api/v2/status/system`, `/api/v2/status/gateways`,
`/api/v2/status/interfaces`, `/api/v2/status/services` e
`/api/v2/system/version`, e, se il privilegio è stato dato,
`/api/v2/system/restapi/version`.

!!! warning "Validato solo sulla documentazione del fornitore — non ancora testato su un sistema reale"

    Questa integrazione è stata costruita dalla documentazione pubblicata
    del pacchetto API REST e dal codice sorgente dei suoi modelli (nomi
    dei campi, unità, l'involucro della risposta e i suoi identificatori
    di errore), non su un pfSense in funzione. Ogni chiamata è una
    lettura. Facci sapere cosa non funziona.

## Cosa controlla

Tutte le metriche hanno il prefisso `dumbmonit_pfsense_`.

| Metrica | Etichette | Significato |
|---|---|---|
| `cpu_usage_percent`, `memory_used_percent`, `swap_used_percent`, `disk_used_percent`, `mbuf_used_percent` | | Come li mostra la dashboard, in percentuale. Il valore CPU è la stima propria di pfSense dalla media di carico. |
| `temperature_celsius` | | Quando pfSense può leggere un sensore. |
| `load_average` | `period`: `1m`, `5m`, `15m` | Media di carico. |
| `cpu_count` | | Core del processore. |
| `version_info` | `version` | Valore 1: la versione di pfSense, `2.7.2-RELEASE`. |
| `gateway_up` | `gateway` | 0 quando pfSense segna il gateway come inattivo, 1 altrimenti. |
| `gateway_status_info` | `gateway`, `status` | Valore 1: la parola propria di pfSense (`none` quando tutto va bene, `down`, `loss`, `delay`). |
| `gateway_loss_percent` | `gateway` | Perdita di pacchetti verso l'indirizzo di monitoraggio. |
| `gateway_delay_milliseconds`, `gateway_stddev_milliseconds` | `gateway` | Andata-ritorno verso l'indirizzo di monitoraggio e la sua deviazione; assente mentre il gateway è inattivo. |
| `gateways`, `gateways_down` | | Conteggi. |
| `interface_enabled`, `interface_up` | `interface`, `descr` | Abilitata nella configurazione; link attivo. |
| `interface_down` | `interface`, `descr` | 1 quando l'interfaccia è abilitata e non ha link. Un'interfaccia disattivata non è mai inattiva. |
| `interface_bytes_in_total`, `interface_bytes_out_total`, `interface_errors_in_total`, `interface_errors_out_total` | `interface`, `descr` | Contatori. |
| `interfaces_down` | | Conteggio. |
| `service_running`, `service_enabled` | `service`, `description` | Come li mostra Status > Services. |
| `service_stopped` | `service`, `description` | 1 quando il servizio è abilitato e non in esecuzione. |
| `services_stopped` | | Conteggio. |
| `states`, `states_max`, `states_used_percent` | | Connessioni seguite da pf, il limite e la quota in uso. Richiede il privilegio `/api/v2/firewall/states/size GET`. |
| `carp_enabled`, `carp_maintenance_mode` | | 1 quando CARP è abilitato; 1 quando è stato messo in modalità manutenzione persistente. Richiede il privilegio `/api/v2/status/carp GET`. |
| `restapi_update_available` | | Con il privilegio `/api/v2/system/restapi/version`: 1 quando il pacchetto API REST ha una release più recente. |
| `restapi_version_info` | `version`, `latest` | Valore 1: release installata e più recente del pacchetto. |

pfSense non dice tramite questa API se un aggiornamento di pfSense è
disponibile: la sola rotta che tocca gli aggiornamenti di sistema
(`/api/v2/system/update`) ne avvia uno. DumbMonit segnala solo gli
aggiornamenti propri del pacchetto.

La tabella degli stati e CARP sono letture facoltative. Ognuna richiede un privilegio in più sull'utente `dumbmonit`: `/api/v2/firewall/states/size GET` e `/api/v2/status/carp GET`. Senza il privilegio, pfSense rifiuta quella lettura e i valori corrispondenti sono semplicemente assenti: nessun errore, nessun avviso.

Le [regole integrate](../alerting/rules.md#pfsense) che si applicano:

| Regola | Cosa | Soglia predefinita | Mantenimento | Gravità | Promemoria |
|---|---|---|---|---|---|
| Gateway pfSense non attivo | Un gateway pfSense è giù: il suo indirizzo di monitoraggio non risponde più e il traffico che trasporta si ferma o passa a un altro percorso (`dumbmonit_pfsense_gateway_up`). | < 1 | 3 min | Avvertenza (`critical`) | 6 h |
| Gateway pfSense con perdita di pacchetti | Un gateway pfSense perde più del 10 % dei probe verso il suo indirizzo di monitoraggio: la linea è degradata (`dumbmonit_pfsense_gateway_loss_percent unless on (target, gateway) dumbmonit_pfsense_gateway_up == 0`). | > 10 % | 10 min | Avviso (`warning`) | 6 h |
| Gateway pfSense lento | Il tempo di andata e ritorno verso l'indirizzo di monitoraggio di un gateway pfSense è rimasto sopra i 500 ms per quindici minuti (`dumbmonit_pfsense_gateway_delay_milliseconds`). | > 500 ms | 15 min | Avviso (`warning`) | 6 h |
| Interfaccia pfSense senza link | Un'interfaccia pfSense abilitata non ha link: cavo, porta dello switch o scheda di rete (`dumbmonit_pfsense_interface_down`). | > 0 | 5 min | Avviso (`warning`) | 6 h |
| Servizio pfSense fermo | Un servizio pfSense abilitato non è in esecuzione: resolver DNS, VPN, NTP… (`dumbmonit_pfsense_service_stopped`). | > 0 | 5 min | Avviso (`warning`) | 6 h |
| Disco pfSense quasi pieno | Il disco di pfSense è pieno per più del 90 %: log, report e aggiornamenti di pacchetti smettono di essere scritti (`dumbmonit_pfsense_disk_used_percent`). | > 90 % | 30 min | Avviso (`warning`) | 1 d |
| Tabella degli stati pfSense quasi piena | pf segue più dell'80 % delle connessioni che gli sono consentite: oltre il limite il firewall scarta le nuove connessioni senza nessun altro sintomo. Rientra sotto il 70 % (`dumbmonit_pfsense_states_used_percent`). | > 80 % | 10 min | Avviso (`warning`) | 6 h |
| Buffer di rete pfSense quasi esauriti | Più del 90 % dei buffer di rete è in uso: un firewall che li esaurisce smette di inoltrare il traffico mentre il suo processore è ancora inattivo. Rientra sotto l'80 % (`dumbmonit_pfsense_mbuf_used_percent`). | > 90 % | 10 min | Avviso (`warning`) | 6 h |
| Temperatura pfSense elevata | La temperatura letta da pfSense è rimasta sopra gli 85 °C per dieci minuti: l'apparato sta per rallentarsi da solo. Rientra a 78 °C (`dumbmonit_pfsense_temperature_celsius`). | > 85 °C | 10 min | Avviso (`warning`) | 6 h |
| CPU pfSense elevata | Il processore è rimasto sopra il 90 % per quindici minuti. Rientra sotto l'80 % (`dumbmonit_pfsense_cpu_usage_percent`). | > 90 % | 15 min | Avviso (`warning`) | 6 h |
| Swap pfSense in uso | pfSense usa più della metà del suo swap da un'ora: è a corto di memoria (`dumbmonit_pfsense_swap_used_percent`). | > 50 % | 1 h | Avviso (`warning`) | 24 h |
| Errori di interfaccia pfSense | Un'interfaccia ha registrato più di 100 errori di ricezione o di trasmissione in quindici minuti: controlla il cavo, il modulo SFP e l'impostazione duplex (`dumbmonit_pfsense_interface_errors_in_total` + `dumbmonit_pfsense_interface_errors_out_total`). | > 100 | 15 min | Avviso (`warning`) | 24 h |
| Modalità manutenzione CARP pfSense lasciata attiva | CARP è rimasto in modalità manutenzione persistente per un'ora: questo firewall ha ceduto i suoi indirizzi virtuali al partner e non li riprenderà da solo (`dumbmonit_pfsense_carp_maintenance_mode`). | > 0 | 1 h | Informazioni (`info`) | 24 h |

## La pagina del monitor

Il pannello Firewall indica cosa non va in una frase, mostra CPU, memoria,
disco e temperatura, poi elenca ogni gateway con la sua latenza e perdita,
le interfacce senza link, i servizi fermi, e un aggiornamento disponibile
del pacchetto API REST.

## Creare una chiave API REST di sola lettura per DumbMonit

1. Installa il pacchetto API REST della community (pfSense-pkg-RESTAPI, versione 2) da Diagnostics > Command Prompt oppure una shell SSH, con il file della pagina delle release del progetto che corrisponde alla tua versione di pfSense (2.7.2 sotto). Poi, sotto System > REST API > Settings, spunta sia BasicAuth che Key tra i metodi di autenticazione.

    ```
    pkg-static add https://github.com/pfrest/pfSense-pkg-RESTAPI/releases/latest/download/pfSense-2.7.2-pkg-RESTAPI.pkg
    ```

2. Sotto System > User Manager, aggiungi un utente chiamato dumbmonit con una password lunga e casuale. Sotto Effective Privileges, aggiungi solo questi privilegi API REST: /api/v2/status/system GET, /api/v2/status/gateways GET, /api/v2/status/interfaces GET, /api/v2/status/services GET, /api/v2/system/version GET, più /api/v2/system/restapi/version GET per conoscere gli aggiornamenti propri del pacchetto. Solo per il passaggio successivo, aggiungi anche /api/v2/auth/key POST.

3. Crea la chiave come dumbmonit, da qualsiasi macchina che raggiunge pfSense: la chiave porta esattamente i privilegi di quell'utente. Copia il valore della chiave dalla risposta, poi rimuovi di nuovo il privilegio /api/v2/auth/key POST.

    ```
    curl -k -u dumbmonit -X POST -H 'Content-Type: application/json' -d '{"descr":"DumbMonit"}' https://pfsense.lan/api/v2/auth/key
    ```

4. In DumbMonit, inserisci l'indirizzo di pfSense, per esempio "pfsense.lan", e incolla la chiave come API key. pfSense fornisce un certificato autofirmato: spunta Accept an unverifiable certificate a meno che tu non lo abbia sostituito.

!!! warning
    L'API REST non dice se un aggiornamento di pfSense è disponibile, solo se il pacchetto stesso ne ha uno. Il pacchetto è un progetto della community, non parte di pfSense: reinstallalo dopo ogni aggiornamento di pfSense.

## Credenziali

| Credenziale | Campi |
|---|---|
| REST API key | La chiave creata per `dumbmonit` nel passaggio 3, inviata come `X-API-Key`. |

Indirizzo: il nome host o IP di pfSense, `host:porta`, oppure un URL.
Opzioni:

- **Protocol** (`https`) e **Port** (443): quelli del webConfigurator,
  impostati sotto System > Advanced > Admin Access.
- **Accept an unverifiable certificate**: il certificato proprio di
  pfSense è autofirmato.
- **Timeout per request** (15 s).

## Limiti noti

- I certificati non vengono letti: il pacchetto restituisce insieme a loro le chiavi private, e DumbMonit si rifiuta di chiederli. Tieni d'occhio una data di scadenza con un [monitor TLS](services.md#tls) sull'indirizzo che serve il certificato.
- I tunnel VPN (IPsec, OpenVPN, WireGuard) non vengono seguiti uno per uno. I loro servizi sì, tramite «Servizio pfSense fermo».
- Gli aggiornamenti di pfSense non vengono segnalati: l'unica rotta dell'API che li riguarda ne avvia uno.
- La memoria non ha una regola: pfSense conta la cache ZFS come memoria usata, quindi un valore alto è normale. Lo swap in uso è il segnale affidabile di un firewall a corto di memoria.

## Risoluzione dei problemi

"pfSense refused the API key (401)": la chiave è errata, oppure Key non è
spuntata tra i metodi di autenticazione sotto System > REST API >
Settings.

"pfSense accepted the API key but refused … (403)": all'utente
`dumbmonit` manca il privilegio di quella pagina; il messaggio lo nomina.

"… not found: is the REST API package installed?": il pacchetto manca,
tipicamente dopo un aggiornamento di pfSense, che lo rimuove. Reinstallalo
come nel passaggio 1.
