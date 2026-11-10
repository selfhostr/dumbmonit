# OPNsense

Open-Source-Firewall und -Router: der Zustand jedes Gateways mit
seiner Round-Trip-Zeit und Paketverlust, die pf-State-Tabelle,
Schnittstellenzähler und -adressen, VPN-Tunnel, DHCP-Leases, der
Resolver, Dienste, CARP und ausstehende Updates.

!!! warning "Nur gegen eine Testinstanz getestet"

    Diese Integration wurde gegen OPNsense 26.1 gebaut und verifiziert,
    das offizielle Image zu diesem Zweck in einer virtuellen Maschine
    gestartet, mit dem unten beschriebenen eingeschränkten API-Key —
    nicht gegen eine Firewall, die seit Jahren den Verkehr eines
    Haushalts routet. Jeder Aufruf ist ein Lesevorgang, und jede Form
    stammt von diesem echten OPNsense, aber seine Gateways hatten keine
    Monitor-Adresse, es war kein Teil eines CARP-Paars und führte
    keinen OpenVPN- oder IPsec-Tunnel: Diese Teile folgen OPNsense'
    Quellcode statt einer beobachteten Antwort. Sagen Sie uns, was
    kaputt ist.

## Die Monitorseite

Über die generischen Diagramme hinaus zeigt ein OPNsense-Monitor drei
Panels, aus dem gelesen, was die Abfrage gespeichert hat — das Öffnen
der Seite fragt die Firewall selbst nie ab:

* **WAN and gateways** — die aktuelle öffentliche Adresse jeder
  Schnittstelle, die ein Gateway trägt, dann jedes Gateway mit seinem
  Zustand als Wort, seiner Round-Trip-Verzögerung und seinem
  Paketverlust. Down gegangene Gateways kommen zuerst. Das ist das
  Panel, für das die Integration existiert: Auf einer Multi-WAN-
  Firewall übernimmt die Backup-Leitung leise, und nichts sonst im
  Netzwerk ändert sich.
* **Traffic and state table** — wie viele Verbindungen pf gegen sein
  konfiguriertes Limit verfolgt, dann jede Schnittstelle mit ihrem
  Link-Zustand, ihren Adressen, ihren Byte- und Paketzählern, und
  ihren Fehlern und Drops, wenn es sie gibt. Dann die DHCP-Leases,
  pro Server gezählt.
* **Firewall health** — gestoppte Dienste zuerst, dann die VPN-
  Tunnel mit der Anzahl verbundener Peers, dann CARP, wenn die
  Firewall die Hälfte eines Paars ist, dann die Firmware (laufende
  Version, was der Mirror anbietet, wartende Pakete, und ob ein
  Reboot ausstehend ist), dann der Resolver, dann die Maschine:
  Uptime, Last, Speicher, Swap, Netzwerkpuffer, Partitionen und
  Temperatursensoren.

Nur Zählungen, nie Verkehr. DumbMonit liest keine Firewall-Regel,
keinen Verbindungszustand, keine Client-Adresse und keinen Hostnamen.
DHCP-Leases werden gezählt, nie gelistet; VPN-Sitzungen werden
gezählt, nie benannt.

## "none" bedeutet "online"

`GET /api/routes/gateway/status` schreibt `"none"` in das
`status`-Feld eines Gateways, wenn **alles in Ordnung ist** — ein
Wort, das für sich wie "keine Information" liest. DumbMonit übersetzt
es zu `online`, sodass die Seite, die Zeitreihen und die Regeln
dasselbe sagen wie OPNsense' eigene Oberfläche. `down` und
`force_down` sind die Fehler; `loss` und `delay` sind dpingers
Warnungen, und das Gateway antwortet trotzdem.

Derselbe Aufruf gibt Verzögerung, Verlust und Standardabweichung als
**Strings mit ihrer Einheit** zurück (`"8.4 ms"`, `"0.0 %"`), und als
`"~"`, wenn das Gateway keine Monitor-Adresse hat. `"~"` erzeugt eine
Zeitreihe weniger, nie eine Null: Ein Gateway, das niemand pingt, hat
nicht null Millisekunden Latenz, es hat keine.

## Der Update-Check wird nie ausgelöst

`GET /api/core/firmware/status` gibt das Ergebnis der *letzten*
Prüfung der Firewall zurück — das, was OPNsense' eigenes Dashboard
zeigt. DumbMonit sendet nie die `POST`-Form dieses Aufrufs, noch
`/api/core/firmware/check`, was die Firewall bei jeder Messung von
jeder DumbMonit-Installation zum Mirror schicken würde.

Eine Firewall, die nie geprüft hat — oder gerade ein Update installiert
hat, was den Cache löscht — antwortet mit `"status": "none"`, genau
wie eine aktuelle. DumbMonit unterscheidet die beiden, und bis eine
Prüfung gelaufen ist, veröffentlicht es `firmware_checked = 0` und
nichts sonst: kein "zero updates pending", kein "no reboot needed".
Führen Sie einmal eine Prüfung aus unter System → Firmware, oder
lassen Sie den eigenen Zeitplan der Firewall das tun.

## Was beobachtet wird

Alle Metriken haben das Präfix `dumbmonit_opnsense_`.

| Familie | Metriken | Labels |
|---|---|---|
| Identität | `version_info` (Labels tragen die OPNsense-Version, die FreeBSD-Version und die Produktlinie) | |
| Maschine | `uptime_seconds`, `load1/5/15`, `cpu_count`, `memory_used/total_bytes`, `memory_used_percent` (Speicher schließt den ZFS-Cache ein, den FreeBSD als genutzt zählt), `swap_used/total_bytes`, `swap_used_percent` (fehlt auf einer Maschine ohne Swap), `mbuf_used` und `mbuf_total` (genutzte Netzwerkpuffer-Cluster gegen `kern.ipc.nmbclusters`), `mbuf_used_percent`, `mbuf_failures` (seit dem Boot abgelehnte Allokationen, ein Zähler) | |
| Storage | `disk_used_bytes`, `disk_total_bytes`, `disk_used_percent` | `device`, `mountpoint` |
| Sensoren | `temperature_celsius` | `sensor` |
| Gateways | `gateway_up` (nur bei `down` und `force_down` 0), `gateway_monitored`, `gateway_delay_seconds`, `gateway_stddev_seconds`, `gateway_loss_percent`, `gateways_total`, `gateways_down` | `gateway`, `address` |
| Schnittstellen | `interface_up`, `interface_bytes_in/out`, `interface_packets_in/out`, `interface_errors_in/out`, `interface_drops`, `interface_collisions` (Zähler, roh gespeichert), `interface_address_info` (die aktuelle Adresse, in einem Label) | `interface`, `device`, und `address` für die letzte |
| Firewall | `pf_states`, `pf_state_limit`, `pf_states_used_percent` | |
| DHCP | `dhcp_leases`, `dhcp_leases_active` | `backend` (`kea`, `isc`, `dnsmasq`) |
| VPN | `vpn_tunnel_up`, `vpn_peers_total`, `vpn_peers_connected` (WireGuard: Handshake innerhalb von drei Minuten), `vpn_handshake_age_seconds` (WireGuard, der jüngste Peer; fehlt, solange sich noch kein Peer verbunden hat), `vpn_bytes_in/out`, `vpn_tunnels_down` | `kind` (`wireguard`, `openvpn`, `ipsec`), `tunnel` (das WireGuard-Gerät, `wg0`, oder die IPsec- und OpenVPN-Beschreibung) |
| Resolver | `unbound_running`, `unbound_queries` (ein Zähler), `unbound_cache_hit_percent` | |
| Dienste | `service_running` (1 oder 0, eine Zeitreihe pro von der Firewall gelistetem Dienst) | `service` |
| CARP | `carp_enabled`, `carp_maintenance_mode`, `carp_vip_master` (alle fehlen auf einer Firewall, die kein Teil eines Paars ist) | `interface`, `vhid`, `address` |
| Firmware | `firmware_checked`, dann — nur sobald eine Prüfung gelaufen ist — `firmware_updates_pending`, `firmware_upgrade_available`, `firmware_reboot_required`, `firmware_connection_ok` | |
| Sammlung | `up`, `scrape_errors`, `scrape_duration_seconds` | |

Schnittstellenzähler werden roh gespeichert, als Zähler: Wickeln Sie
sie in `rate()` ein, um einen Durchsatz zu erhalten. Das so zu tun
bedeutet, dass ein Firewall-Reboot als Lücke erscheint, nicht als ein
Ausschlag von mehreren Gigabyte pro Sekunde.

`gateway_up` ist 1 für ein überhaupt nicht überwachtes Gateway, weil
es nichts anderes zu sagen gibt; `gateway_monitored` ist das, was
die beiden unterscheidet. Ein Gateway ohne Monitor-Adresse erzeugt
weder eine Verzögerungs- noch eine Verlust-Zeitreihe.

Eine Firewall, die kein Teil eines Hochverfügbarkeitspaars ist, hat
keine CARP-virtuelle-Adresse und überhaupt keine CARP-Zeitreihe. Das
ist eine Firewall ohne Cluster, kein degradierter Cluster: Keine
Regel kann feuern.

WireGuard, OpenVPN, IPsec, Unbound und Kea sind Plugins. Eine
Firewall, die eines nicht hat, antwortet mit `404`, was weder ein
Sammlungsfehler noch ein Authentifizierungsfehler ist: eine Metrik
weniger, und nichts in der Oberfläche.

Geltende eingebaute Regeln: Monitor unreachable, Internet gateway
down, Gateway losing packets, Gateway slow, Firewall state table
filling up, Firewall network buffers exhausted, VPN tunnel down,
Firewall resolver stopped, Firewall core service stopped, Firewall
too hot, Firewall left in CARP maintenance mode, Firewall reboot
pending, Firewall updates pending. Benachrichtigungen benennen das
betroffene Gateway, den Tunnel, Dienst oder Sensor.

## Was in OPNsense vorzubereiten ist

Die Schritte unten sind die, die der Hinweis neben dem Formular zeigt.
Das Prinzip: eine Gruppe mit nur den Status-Rechten, die DumbMonit
braucht, ein Benutzer, der sich nie einloggt, und ein API-Key, der
keinem Ihrer eigenen Konten gehört. Genau diese Gruppe wurde gegen
jeden Aufruf von DumbMonit geprüft, auf OPNsense 26.1.

1. In der Web-Oberfläche: System → Access → Groups → Add. Benennen
   Sie die Gruppe wie folgt.

    ```
    dumbmonit
    ```

2. Kreuzen Sie diese Rechte in der Gruppe an, genau wie OPNsense sie
   benennt: Lobby: Dashboard (System, Speicher, Platten, Temperaturen,
   State-Tabelle), System: Gateways, Status: Interfaces, Status:
   Services, System: Firmware, Interfaces: Virtual IPs: Status
   (CARP) und Services: Unbound (MVC). Dann eines pro von Ihnen
   genutztem Feature: Services: DHCP: Kea(v4) oder Services: Dnsmasq
   DNS/DHCP: Settings für die Leases, VPN: WireGuard: Status, Status:
   OpenVPN, Status: IPsec. Ein ausgelassenes Recht kostet genau die
   Metriken, die es trägt, nichts sonst.

3. System → Access → Users → Add. Benennen Sie das Konto wie folgt,
   lassen Sie OPNsense ein verwürfeltes Passwort erzeugen (dieses
   Konto loggt sich nie ein: Es antwortet mit seinem Key), und machen
   Sie es zu einem Mitglied der dumbmonit-Gruppe.

    ```
    dumbmonit
    ```

4. Bearbeiten Sie diesen Benutzer erneut und klicken Sie unter API
   keys auf +. OPNsense lädt eine kleine Textdatei mit zwei Zeilen,
   key= und secret=, herunter und zeigt das Secret nur dieses eine
   Mal. Fügen Sie den Key in DumbMonits Feld API key ein und das
   Secret in sein Feld API secret.

5. Geben Sie in DumbMonit die Firewall-Adresse ein, zum Beispiel
   "192.168.1.1" oder "fw.lan:8443". Die Firewall wird nie gebeten,
   nach Updates zu suchen: DumbMonit liest das Ergebnis der von
   OPNsense nach eigenem Zeitplan ausgeführten Prüfung, oder die Sie
   von System → Firmware aus starten.

!!! warning

    Nutzen Sie nicht das Konto wiederverwendet, mit dem Sie sich
    einloggen: Ein API-Key ist ein Passwort, das nie abläuft. OPNsense
    hat keine nur-lesende Variante einiger obiger Rechte: Status:
    Services erlaubt auch das Starten und Stoppen eines Dienstes,
    System: Firmware erlaubt auch das Starten eines Updates, und die
    Unbound-, Kea- und Dnsmasq-Rechte decken auch ihre Einstellungen
    ab. DumbMonit liest immer nur, aber um zu begrenzen, was ein
    durchgesickerter Key tun könnte, beschränken Sie die
    Quellnetzwerke der Gruppe auf die Adresse des DumbMonit-Hosts.
    Gewähren Sie nie All pages. OPNsense liefert auch ein
    selbstsigniertes Zertifikat: Wird die Verbindung deshalb
    abgelehnt, kreuzen Sie "Accept an unverifiable certificate" in
    den Optionen an.

Adresse: `192.168.1.1`, `fw.lan`, `fw.lan:8443`, `[fd00::1]` oder eine
vollständige URL. Das Schema `https` und Port 443 werden hinzugefügt,
falls fehlend.

Manche Rechte decken mehr als einen Aufruf ab. `Lobby: Dashboard`
trägt die Systeminformations-, Zeit-, Speicher-, Platten-,
Swap-, Netzwerkpuffer- und Temperatur-Endpunkte sowie die
State-Tabelle; `Status: Interfaces` trägt die Schnittstellenübersicht
mit ihren Zählern und Adressen. Ein ausgelassenes Recht kostet genau
die Metriken, die es trägt: Die Abfrage gelingt trotzdem, und
`scrape_errors` bleibt bei null, weil ein `403` auf einem optionalen
Aufruf als "dieses Konto darf das nicht sehen" gelesen wird, nicht
als Fehlschlag. Nur `Lobby: Dashboard` ist erforderlich: Ohne es kann
die Abfrage nicht einmal den Namen und die Version der Firewall
lesen, und meldet einen Authentifizierungsfehler.

### camelCase oder snake_case

OPNsense 25.7 hat seine API-Pfade von `systemResources` zu
`system_resources` umbenannt. Beide Schreibweisen antworten noch,
aber die Rechte vergleichen den Pfad buchstabengetreu: Auf 25.7 und
später erhält ein eingeschränkter Key `403` auf der alten
Schreibweise, und auf 25.1 bei manchen der neuen. DumbMonit fragt
zuerst nach der aktuellen Schreibweise und fällt bei einem `403` auf
die andere zurück, sodass dieselbe Gruppe auf beiden Seiten der
Umbenennung funktioniert.

## Optionen

| Schlüssel | Label | Standard | Hilfe |
|---|---|---|---|
| `scheme` | Protocol | `https` | HTTPS passt zu einer Firewall out of the box. HTTP nur, wenn die Web-Oberfläche im Klartext bedient wird. |
| `port` | Web interface port | `443` | Genutzt, falls die Adresse keinen Port angibt. |
| `insecure_tls` | Accept an unverifiable certificate | `false` | OPNsense liefert standardmäßig ein selbstsigniertes Zertifikat. |
| `request_timeout_seconds` | Timeout per request (seconds) | `15` | Erlaubte Zeit für jeden API-Aufruf, von 1 bis 120. |
| `gateways` | Watch the gateways | `true` | Liest, was dpinger über jedes Gateway sagt: up oder down, Round-Trip-Verzögerung und Paketverlust. |
| `interfaces` | Watch the interfaces | `true` | Liest den Link-Zustand jeder Schnittstelle, Adressen und Byte-, Paket-, Fehler- und Drop-Zähler. |
| `firewall` | Watch the state table | `true` | Liest, wie viele Verbindungen pf verfolgt und das konfigurierte Limit. |
| `dhcp` | Count the DHCP leases | `true` | Zählt die von der Firewall vergebenen Leases, welchen Server sie auch nutzt. Nur Zählungen. |
| `vpn` | Watch the VPN tunnels | `true` | Liest WireGuard, OpenVPN und IPsec: welche Tunnel existieren und welche up sind. |
| `unbound` | Watch the resolver | `true` | Liest, ob Unbound antwortet. |
| `services` | Watch the services | `true` | Liest die Liste der von der Firewall verwalteten Dienste und welche davon laufen. |
| `carp` | Watch CARP | `true` | Liest die virtuellen Adressen eines Hochverfügbarkeitspaars, und dauerhaften Wartungsmodus. |
| `firmware` | Watch for updates | `true` | Liest das Ergebnis der letzten Update-Prüfung der Firewall und ob ein Reboot ausstehend ist. |
| `temperature` | Read the temperature sensors | `true` | Liest die von der Firewall exponierten CPU- und Board-Sensoren. |

## Integrierte Alarmregeln

Diese Regeln werden mit DumbMonit ausgeliefert und sind von Anfang an aktiv. Jede lässt sich unter Warnungen bearbeiten oder abschalten; die [Referenz der integrierten Regeln](../alerting/rules.md#opnsense) führt alle Regeln aller Integrationen auf.

| Regel | Was | Standardschwelle | Haltezeit | Schweregrad | Erinnerung |
|---|---|---|---|---|---|
| Internet-Gateway ausgefallen | dpinger erhält von einem Gateway keine Antwort mehr (`dumbmonit_opnsense_gateway_up`). Bei einer Multi-WAN-Firewall ist der Verkehr bereits still auf eine andere Verbindung gewandert. `loss` und `delay` sind Warnungen, keine Ausfälle, und lösen diese Regel nicht aus. | < 1 | 5 Min. | Warnung (`critical`) | 6 h |
| Gateway verliert Pakete | Ein Gateway verliert mehr als ein Fünftel der an es gesendeten Pakete (`dumbmonit_opnsense_gateway_loss_percent`): Die Verbindung antwortet, schlecht. Löst sich unter 10 %. | > 20 % | 10 Min. | Hinweis (`warning`) | 6 h |
| Gateway langsam | Die Round-Trip-Zeit eines Gateways liegt dauerhaft über 300 ms (`dumbmonit_opnsense_gateway_delay_seconds`). Dreihundert Millisekunden lassen eine mobile Backup-Verbindung in Ruhe. | > 300 ms | 15 Min. | Info (`info`) | 12 h |
| Zustandstabelle der Firewall füllt sich | pf verfolgt mehr als 80 % der erlaubten Verbindungen (`dumbmonit_opnsense_pf_states_used_percent`). Über dem Limit verwirft die Firewall neue Verbindungen ohne anderes Symptom. | > 80 % | 10 Min. | Hinweis (`warning`) | 6 h |
| Netzwerkpuffer der Firewall erschöpft | Mehr als 90 % der FreeBSD-Netzwerkpuffer sind in Gebrauch (`dumbmonit_opnsense_mbuf_used_percent`). Eine Firewall, der sie ausgehen, hört auf zu forwarden, während die CPU noch im Leerlauf ist. | > 90 % | 10 Min. | Hinweis (`warning`) | 6 h |
| VPN-Tunnel ausgefallen | Ein konfigurierter VPN-Tunnel hat seit zehn Minuten keine Sitzung (`dumbmonit_opnsense_vpn_tunnel_up`). Ein nicht installiertes Plugin oder eine deaktivierte Verbindung hat keine Serie und kann nicht auslösen. | < 1 | 10 Min. | Hinweis (`warning`) | 6 h |
| Resolver der Firewall gestoppt | Unbound antwortet nicht mehr (`dumbmonit_opnsense_unbound_running`). Die Firewall routet weiter; die Maschinen dahinter lösen nicht mehr auf. Schalten Sie diese Regel aus, wenn Ihre Firewall mit etwas anderem auflöst. | < 1 | 5 Min. | Hinweis (`warning`) | 6 h |
| Kerndienst der Firewall gestoppt | dpinger, der die Gateways überwacht, oder configd, der alles andere ausführt, ist gestoppt (`dumbmonit_opnsense_service_running{service=~"dpinger\|configd"}`). Nur diese beiden, weil ein absichtlich abgeschalteter Dienst kein Ausfall ist. | < 1 | 5 Min. | Warnung (`critical`) | 6 h |
| Firewall zu heiß | Ein Sensor liest über 85 °C (`dumbmonit_opnsense_temperature_celsius`): Die lüfterlose Box im Schrank beginnt gleich zu drosseln. Löst sich bei 78 °C. | > 85 °C | 10 Min. | Hinweis (`warning`) | 6 h |
| Firewall im CARP-Wartungsmodus belassen | CARP wurde im dauerhaften Wartungsmodus belassen (`dumbmonit_opnsense_carp_maintenance_mode`): Diese Firewall hat ihre virtuellen Adressen an ihren Partner übergeben und nimmt sie nicht von selbst zurück. | > 0 | 1 h | Info (`info`) | 24 h |
| Firewall-Neustart ausstehend | Ein Update ist installiert, braucht aber einen Neustart, um zu wirken (`dumbmonit_opnsense_firmware_reboot_required`). | > 0 | 1 h | Info (`info`) | 7 T. |
| Firewall-Updates ausstehend | Die Firewall hat Pakete, die auf ein Update warten (`dumbmonit_opnsense_firmware_updates_pending`). Sie ist die am stärksten exponierte Maschine im Netzwerk. | > 0 | 1 h | Info (`info`) | 7 T. |
| OPNsense-Zertifikat läuft ab | Ein Zertifikat, das die Firewall verwendet (Weboberfläche, Captive Portal, VPN), läuft in weniger als 14 Tagen ab (`dumbmonit_opnsense_certificate_expiry_seconds`). | < 14 T. | 1 h | Hinweis (`warning`) | 24 h |
| OPNsense-Zertifikat abgelaufen | Ein Zertifikat, das die Firewall verwendet, ist abgelaufen: Clients lehnen es jetzt ab (`dumbmonit_opnsense_certificate_expiry_seconds`). | < 0 | 10 Min. | Warnung (`critical`) | 12 h |
| OPNsense-Platte fast voll | Die Platte der Firewall ist zu mehr als 90 % voll: Protokolle, Mitschnitte und Updates werden scheitern. Endet unter 85 % (`dumbmonit_opnsense_disk_used_percent`). | > 90 % | 15 Min. | Hinweis (`warning`) | 12 h |
| OPNsense-CPU hoch | Der Prozessor der Firewall lag fünfzehn Minuten lang über 90 %. Endet unter 80 % (`dumbmonit_opnsense_cpu_percent`). | > 90 % | 15 Min. | Hinweis (`warning`) | 6 h |
| OPNsense-Swap in Benutzung | Die Firewall nutzt seit einer Stunde mehr als die Hälfte ihres Swap: Ihr fehlt Arbeitsspeicher (`dumbmonit_opnsense_swap_used_percent`). | > 50 % | 1 h | Hinweis (`warning`) | 24 h |
| OPNsense-Paketfilter deaktiviert | Der Paketfilter (pf) ist deaktiviert: Die Firewall filtert und übersetzt nichts mehr (`dumbmonit_opnsense_pf_enabled`). | < 1 | 5 Min. | Warnung (`critical`) | 1 h |
| OPNsense-Schnittstellenfehler | Eine Schnittstelle hat in fünfzehn Minuten mehr als 100 Empfangs- oder Sendefehler verzeichnet: Prüfen Sie das Kabel, das SFP-Modul und die Duplex-Einstellung (`dumbmonit_opnsense_interface_errors_in` + `dumbmonit_opnsense_interface_errors_out`). | > 100 | 15 Min. | Hinweis (`warning`) | 24 h |
| OPNsense-Update-Spiegel nicht erreichbar | Die Firewall erreicht ihren Update-Spiegel seit zwölf Stunden nicht: Sie erfährt nichts von Sicherheitsupdates (`dumbmonit_opnsense_firmware_connection_ok`). | < 1 | 12 h | Info (`info`) | 7 T. |

## Bekannte Grenzen

- Die Update-Prüfung wird nie ausgelöst: DumbMonit liest das Ergebnis der letzten Prüfung, die die Firewall selbst durchgeführt hat. Eine Firewall, die nie geprüft hat, sagt nichts über Updates.
- Höchstens 64 Reihen pro Familie: Gateways, Schnittstellen, Dienste, Tunnel, Partitionen, Sensoren.
- Ein nicht installiertes Plugin (WireGuard, OpenVPN, IPsec, Unbound, Kea) macht kein Geräusch: keine Reihe, keine Regel, kein Fehler.
- VPN-Sitzungen und DHCP-Leases werden gezählt, nie aufgelistet: Es wird keine Client-Adresse und kein Hostname gelesen.
- Zertifikate, die OPNsense als unbenutzt markiert, werden ignoriert.

## Häufige Fehler

| Symptom | Wahrscheinliche Ursache |
|---|---|
| Authentifizierungsfehler | Key und Secret sind vertauscht, oder der Key wurde vom Benutzer gelöscht. OPNsense zeigt das Secret nur einmal: Ist es verloren, erstellen Sie einen neuen Key und löschen Sie den alten. |
| Eine Webseite statt JSON | Der API-Key wird abgelehnt, oder das Konto kann diesen Endpunkt nicht erreichen. OPNsense liefert dann seine Login-Seite mit einem `200`; DumbMonit meldet das als Authentifizierungsproblem statt als nicht lesbare Antwort. |
| Zertifikatsfehler | Selbstsigniertes Zertifikat: installieren Sie ein vertrauenswürdiges (ACME ist ein Plugin) oder aktivieren Sie `insecure_tls`. |
| Überhaupt keine Gateway-Metriken | Die Option `gateways` ist aus, oder der Gruppe fehlt `System: Gateways`. Eine Firewall ohne definiertes Gateway hat tatsächlich keines. |
| Ein Gateway zeigt keine Verzögerung oder keinen Verlust | Es hat keine Monitor-Adresse, daher pingt dpinger es nicht. OPNsense schreibt `~`, und DumbMonit veröffentlicht keine Zeitreihe statt einer Null. |
| "Gateway down" feuert für eine funktionierende Leitung | Die Monitor-Adresse ist nicht erreichbar, obwohl der Link up ist — ein häufiger Fall bei einem ISP, der ICMP verwirft. Setzen Sie eine andere Monitor-Adresse unter System → Gateways. |
| Keine State-Tabellen-Metriken | Der Gruppe fehlt `Lobby: Dashboard`. |
| Authentifizierungsfehler bei einem in einem Browser-Tool funktionierenden Key | Das Tool nutzte die andere Schreibweise des Pfads. Mit einem eingeschränkten Key antworten OPNsense 25.7 und später mit `403` auf `systemInformation` und `200` auf `system_information`. |
| Keine Update-Metriken außer `firmware_checked` | Die Firewall hat nie nach Updates gesucht, oder hat gerade eines installiert. Führen Sie eine Prüfung aus unter System → Firmware. |
| Keine VPN-Metriken | Das Plugin ist nicht installiert, oder kein Tunnel ist konfiguriert. Eine deaktivierte IPsec-Verbindung wird absichtlich nicht als down gemeldet. |
| "Firewall resolver stopped" auf einer gut aufgelösten Firewall | Unbound ist installiert, aber ausgeschaltet, weil die Firewall mit etwas anderem auflöst. Schalten Sie diese Regel aus. |
| Firmware-Version sieht veraltet aus | Die Firewall hat den Mirror nicht kürzlich geprüft. DumbMonit liest die letzte Prüfung, es löst nie eine aus. |
| `scrape_errors` über null | Ein optionaler Aufruf schlug aus einem anderen Grund als einem fehlenden Recht oder fehlenden Plugin fehl — meist ein Timeout. Die anderen Metriken sind nicht betroffen. |
