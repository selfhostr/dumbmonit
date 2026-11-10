# pfSense

Eine pfSense-Firewall über das Community-REST-API-Paket
(`pfSense-pkg-RESTAPI`, Version 2): Gateways, die down sind, Pakete
verlieren oder langsam antworten, aktivierte Schnittstellen ohne Link,
aktivierte Dienste, die gestoppt sind, und CPU, Speicher, Platte und
Temperatur.

DumbMonit sendet einen API-Key im Header `X-API-Key`. Ein Key trägt
genau die Rechte des pfSense-Benutzers, zu dem er gehört: Der Benutzer
`dumbmonit` unten hält nur die `GET`-Rechte der fünf von DumbMonit
gelesenen Seiten, daher kann der Key diese Seiten lesen und nichts
ändern. Jede Minute liest DumbMonit parallel
`/api/v2/status/system`, `/api/v2/status/gateways`,
`/api/v2/status/interfaces`, `/api/v2/status/services` und
`/api/v2/system/version`, und, falls das Recht gegeben wurde,
`/api/v2/system/restapi/version`.

!!! warning "Nur gegen die Herstellerdokumentation validiert — noch nicht an einem echten System getestet"

    Diese Integration wurde aus der veröffentlichten Dokumentation des
    REST-API-Pakets und dem Quellcode seiner Modelle gebaut (Feldnamen,
    Einheiten, der Antwort-Umschlag und seine Fehlerkennungen), nicht
    gegen ein laufendes pfSense. Jeder Aufruf ist ein Lesevorgang.
    Sagen Sie uns, was kaputt ist.

## Was beobachtet wird

Alle Metriken haben das Präfix `dumbmonit_pfsense_`.

| Metrik | Labels | Bedeutung |
|---|---|---|
| `cpu_usage_percent`, `memory_used_percent`, `swap_used_percent`, `disk_used_percent`, `mbuf_used_percent` | | Wie das Dashboard sie zeigt, in Prozent. Die CPU-Zahl ist pfSenses eigene Schätzung aus dem Load Average. |
| `temperature_celsius` | | Wenn pfSense einen Sensor lesen kann. |
| `load_average` | `period`: `1m`, `5m`, `15m` | Load Average. |
| `cpu_count` | | Prozessorkerne. |
| `version_info` | `version` | Wert 1: die pfSense-Version, `2.7.2-RELEASE`. |
| `gateway_up` | `gateway` | 0, wenn pfSense das Gateway als down markiert, sonst 1. |
| `gateway_status_info` | `gateway`, `status` | Wert 1: pfSenses eigenes Wort (`none`, wenn alles gut ist, `down`, `loss`, `delay`). |
| `gateway_loss_percent` | `gateway` | Paketverlust zur Monitor-Adresse. |
| `gateway_delay_milliseconds`, `gateway_stddev_milliseconds` | `gateway` | Round Trip zur Monitor-Adresse und seine Abweichung; fehlt, während das Gateway down ist. |
| `gateways`, `gateways_down` | | Zählungen. |
| `interface_enabled`, `interface_up` | `interface`, `descr` | In der Konfiguration aktiviert; Link up. |
| `interface_down` | `interface`, `descr` | 1, wenn die Schnittstelle aktiviert ist und keinen Link hat. Eine deaktivierte Schnittstelle ist nie down. |
| `interface_bytes_in_total`, `interface_bytes_out_total`, `interface_errors_in_total`, `interface_errors_out_total` | `interface`, `descr` | Zähler. |
| `interfaces_down` | | Zählung. |
| `service_running`, `service_enabled` | `service`, `description` | Wie Status > Services sie zeigt. |
| `service_stopped` | `service`, `description` | 1, wenn der Dienst aktiviert ist und nicht läuft. |
| `services_stopped` | | Zählung. |
| `states`, `states_max`, `states_used_percent` | | Verbindungen, die pf verfolgt, die Grenze und der belegte Anteil. Braucht das Privileg `/api/v2/firewall/states/size GET`. |
| `carp_enabled`, `carp_maintenance_mode` | | 1, wenn CARP aktiviert ist; 1, wenn es in den dauerhaften Wartungsmodus versetzt wurde. Braucht das Privileg `/api/v2/status/carp GET`. |
| `restapi_update_available` | | Mit dem Recht `/api/v2/system/restapi/version`: 1, wenn das REST-API-Paket eine neuere Version hat. |
| `restapi_version_info` | `version`, `latest` | Wert 1: installierte und neueste Version des Pakets. |

pfSense sagt über diese API nicht, ob ein pfSense-Update verfügbar ist:
Die einzige Route, die Systemupdates betrifft
(`/api/v2/system/update`), startet eines. DumbMonit meldet nur die
eigenen Updates des Pakets.

Die Zustandstabelle und CARP sind optionale Abfragen. Jede braucht ein weiteres Privileg für den Benutzer `dumbmonit`: `/api/v2/firewall/states/size GET` und `/api/v2/status/carp GET`. Ohne das Privileg lehnt pfSense diese Abfrage ab, und die zugehörigen Werte fehlen einfach: kein Fehler, kein Alarm.

Die geltenden [eingebauten Regeln](../alerting/rules.md#pfsense):

| Regel | Was | Standardschwelle | Haltezeit | Schweregrad | Erinnerung |
|---|---|---|---|---|---|
| pfSense-Gateway ausgefallen | Ein pfSense-Gateway ist ausgefallen: Seine Monitor-Adresse antwortet nicht mehr, und der von ihm getragene Verkehr stoppt oder wechselt (`dumbmonit_pfsense_gateway_up`). | < 1 | 3 Min. | Warnung (`critical`) | 6 h |
| pfSense-Gateway verliert Pakete | Ein pfSense-Gateway verliert mehr als 10 % der Probes zu seiner Monitor-Adresse: Die Leitung ist beeinträchtigt (`dumbmonit_pfsense_gateway_loss_percent unless on (target, gateway) dumbmonit_pfsense_gateway_up == 0`). | > 10 % | 10 Min. | Hinweis (`warning`) | 6 h |
| pfSense-Gateway langsam | Der Round-Trip zur Monitor-Adresse eines pfSense-Gateways liegt seit fünfzehn Minuten über 500 ms (`dumbmonit_pfsense_gateway_delay_milliseconds`). | > 500 ms | 15 Min. | Hinweis (`warning`) | 6 h |
| pfSense-Schnittstelle ohne Link | Eine aktivierte pfSense-Schnittstelle hat keinen Link: Kabel, Switch-Port oder Netzwerkkarte (`dumbmonit_pfsense_interface_down`). | > 0 | 5 Min. | Hinweis (`warning`) | 6 h |
| pfSense-Dienst gestoppt | Ein aktivierter pfSense-Dienst läuft nicht: DNS-Resolver, VPN, NTP … (`dumbmonit_pfsense_service_stopped`). | > 0 | 5 Min. | Hinweis (`warning`) | 6 h |
| pfSense-Platte fast voll | Die pfSense-Platte ist zu mehr als 90 % voll: Logs, Berichte und Paket-Updates werden nicht mehr geschrieben (`dumbmonit_pfsense_disk_used_percent`). | > 90 % | 30 Min. | Hinweis (`warning`) | 1 T. |
| pfSense-Zustandstabelle fast voll | pf verfolgt mehr als 80 % der Verbindungen, die ihm erlaubt sind: Jenseits der Grenze verwirft die Firewall neue Verbindungen ohne jedes andere Symptom. Endet unter 70 % (`dumbmonit_pfsense_states_used_percent`). | > 80 % | 10 Min. | Hinweis (`warning`) | 6 h |
| pfSense-Netzwerkpuffer fast erschöpft | Mehr als 90 % der Netzwerkpuffer sind belegt: Eine Firewall, der sie ausgehen, leitet nichts mehr weiter, während ihr Prozessor im Leerlauf ist. Endet unter 80 % (`dumbmonit_pfsense_mbuf_used_percent`). | > 90 % | 10 Min. | Hinweis (`warning`) | 6 h |
| pfSense-Temperatur hoch | Die Temperatur, die pfSense misst, lag zehn Minuten lang über 85 °C: Das Gerät wird sich gleich selbst drosseln. Endet bei 78 °C (`dumbmonit_pfsense_temperature_celsius`). | > 85 °C | 10 Min. | Hinweis (`warning`) | 6 h |
| pfSense-CPU hoch | Der Prozessor lag fünfzehn Minuten lang über 90 %. Endet unter 80 % (`dumbmonit_pfsense_cpu_usage_percent`). | > 90 % | 15 Min. | Hinweis (`warning`) | 6 h |
| pfSense-Swap in Benutzung | pfSense nutzt seit einer Stunde mehr als die Hälfte seines Swap: Ihm fehlt Arbeitsspeicher (`dumbmonit_pfsense_swap_used_percent`). | > 50 % | 1 h | Hinweis (`warning`) | 24 h |
| pfSense-Schnittstellenfehler | Eine Schnittstelle hat in fünfzehn Minuten mehr als 100 Empfangs- oder Sendefehler verzeichnet: Prüfen Sie das Kabel, das SFP-Modul und die Duplex-Einstellung (`dumbmonit_pfsense_interface_errors_in_total` + `dumbmonit_pfsense_interface_errors_out_total`). | > 100 | 15 Min. | Hinweis (`warning`) | 24 h |
| pfSense-CARP-Wartungsmodus aktiv belassen | CARP ist seit einer Stunde im dauerhaften Wartungsmodus: Diese Firewall hat ihre virtuellen Adressen an ihren Partner abgegeben und holt sie nicht von selbst zurück (`dumbmonit_pfsense_carp_maintenance_mode`). | > 0 | 1 h | Info (`info`) | 24 h |

## Die Monitorseite

Das Firewall-Panel sagt in einem Satz, was nicht stimmt, zeigt CPU,
Speicher, Platte und Temperatur, listet dann jedes Gateway mit seiner
Latenz und seinem Verlust, die Schnittstellen ohne Link, die
gestoppten Dienste, und ein verfügbares Update des REST-API-Pakets.

## Einen nur-lesenden REST-API-Key für DumbMonit erstellen

1. Installieren Sie das Community-REST-API-Paket
   (pfSense-pkg-RESTAPI, Version 2) über Diagnostics > Command Prompt
   oder eine SSH-Shell, mit der zu Ihrer pfSense-Version passenden
   Datei von der Releases-Seite des Projekts (unten 2.7.2). Kreuzen
   Sie dann unter System > REST API > Settings beide BasicAuth und Key
   unter den Authentifizierungsmethoden an.

    ```
    pkg-static add https://github.com/pfrest/pfSense-pkg-RESTAPI/releases/latest/download/pfSense-2.7.2-pkg-RESTAPI.pkg
    ```

2. Fügen Sie unter System > User Manager einen Benutzer namens
   dumbmonit mit einem langen Zufallspasswort hinzu. Fügen Sie unter
   Effective Privileges nur diese REST-API-Rechte hinzu:
   /api/v2/status/system GET, /api/v2/status/gateways GET,
   /api/v2/status/interfaces GET, /api/v2/status/services GET,
   /api/v2/system/version GET, plus /api/v2/system/restapi/version GET,
   um von den eigenen Updates des Pakets zu hören. Nur für den
   nächsten Schritt fügen Sie außerdem /api/v2/auth/key POST hinzu.

3. Erstellen Sie den Key als dumbmonit, von jeder Maschine aus, die
   pfSense erreicht: Der Key trägt genau die Rechte dieses Benutzers.
   Kopieren Sie den Key-Wert der Antwort, entfernen Sie dann das Recht
   /api/v2/auth/key POST wieder.

    ```
    curl -k -u dumbmonit -X POST -H 'Content-Type: application/json' -d '{"descr":"DumbMonit"}' https://pfsense.lan/api/v2/auth/key
    ```

4. Geben Sie in DumbMonit die Adresse von pfSense ein, zum Beispiel
   "pfsense.lan", und fügen Sie den Key als API-Key ein. pfSense
   liefert ein selbstsigniertes Zertifikat: Kreuzen Sie "Accept an
   unverifiable certificate" an, sofern Sie es nicht ersetzt haben.

!!! warning
    Die REST-API sagt nicht, ob ein pfSense-Update verfügbar ist, nur
    ob das Paket selbst eines hat. Das Paket ist ein Community-Projekt,
    nicht Teil von pfSense: Installieren Sie es nach jedem
    pfSense-Upgrade neu.

## Credentials

| Credential | Felder |
|---|---|
| REST-API-Key | Der in Schritt 3 für `dumbmonit` erstellte Key, als `X-API-Key` gesendet. |

Adresse: der Hostname oder die IP von pfSense, `host:port`, oder eine
URL. Optionen:

- **Protocol** (`https`) und **Port** (443): die des webConfigurator,
  gesetzt unter System > Advanced > Admin Access.
- **Accept an unverifiable certificate**: pfSenses eigenes Zertifikat
  ist selbstsigniert.
- **Timeout pro Request** (15 s).

## Bekannte Grenzen

- Zertifikate werden nicht gelesen: Das Paket liefert ihre privaten Schlüssel gleich mit, und DumbMonit weigert sich, sie anzufordern. Überwachen Sie ein Ablaufdatum mit einem [TLS-Monitor](services.md#tls) auf der Adresse, die das Zertifikat ausliefert.
- VPN-Tunnel (IPsec, OpenVPN, WireGuard) werden nicht einzeln verfolgt. Ihre Dienste schon, über „pfSense-Dienst gestoppt“.
- pfSense-Updates werden nicht gemeldet: Die einzige API-Route, die sie berührt, startet eines.
- Für den Arbeitsspeicher gibt es keine Regel: pfSense zählt den ZFS-Cache als belegten Speicher, ein hoher Wert ist also normal. Belegter Swap ist das verlässliche Zeichen für eine Firewall, der Arbeitsspeicher fehlt.

## Fehlerbehebung

"pfSense refused the API key (401)": der Key ist falsch, oder Key ist
unter System > REST API > Settings nicht unter den
Authentifizierungsmethoden angekreuzt.

"pfSense accepted the API key but refused … (403)": dem Benutzer
`dumbmonit` fehlt das Recht dieser Seite; die Meldung benennt es.

"… not found: is the REST API package installed?": das Paket fehlt,
typischerweise nach einem pfSense-Upgrade, das es entfernt.
Installieren Sie es wie in Schritt 1 neu.
