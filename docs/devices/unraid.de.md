# Unraid

Ein Unraid-Server über seine GraphQL-API: ein nicht gestartetes Array,
deaktivierte, fehlende oder Lesefehler meldende Platten,
Paritätsprüfungen, die Fehler fanden oder seit Wochen nicht liefen,
sich füllende Cache-Pools, Container, die laufen sollten und nicht
laufen, und der Zustand der VMs.

DumbMonit sendet einen API-Key mit der Viewer-Rolle im Header
`x-api-key` an `/graphql`. Die Viewer-Rolle liest alles und kann nichts
ändern. Die API ist ab 7.2 in Unraid eingebaut; auf 6.12 bis 7.1 kommt
sie mit dem Unraid-Connect-Plugin.

Jede Minute sendet DumbMonit eine erforderliche Abfrage (Version,
Array, Parität, Daten- und Cache-Platten) und vier optionale, jede für
sich, sodass ein Feld, das eine ältere API-Version nicht hat, oder
ausgeschaltete VMs, nur diesen Teil weglässt: die Paritätsprüfung, die
Docker-Container, die VMs und die ungelesenen Benachrichtigungen.

!!! warning "Nur gegen die Herstellerdokumentation validiert — noch nicht an einem echten System getestet"

    Diese Integration wurde aus Unraids API-Dokumentation und dem
    veröffentlichten GraphQL-Schema der Unraid-API gebaut (Feldnamen,
    Enumerationen, Größen in Kilobyte), nicht gegen einen laufenden
    Server. Jede Abfrage ist ein Lesevorgang. Sagen Sie uns, was kaputt
    ist.

## Was beobachtet wird

Alle Metriken haben das Präfix `dumbmonit_unraid_`.

| Metrik | Labels | Bedeutung |
|---|---|---|
| `version_info` | `version` | Wert 1: die Unraid-Version. |
| `array_started` | | 1, wenn das Array gestartet ist. |
| `array_state_info` | `state` | Wert 1: `started`, `stopped`, `new_array`, `recon_disk`, `disable_disk`… |
| `array_size_bytes`, `array_used_bytes`, `array_free_bytes`, `array_used_percent` | | Array-Kapazität. |
| `disk_ok` | `disk`, `role` (`parity`, `data`, `cache`) | 1, wenn der Status der Platte `DISK_OK` ist. Leere Steckplätze werden ausgelassen. |
| `disk_status_info` | `disk`, `role`, `status` | Wert 1: Unraids Status (`disk_ok`, `disk_dsbl`, `disk_np_missing`, `disk_invalid`…). |
| `disk_errors` | `disk`, `role` | Lesefehler seit Array-Start (die Errors-Spalte). |
| `disk_temperature_celsius` | `disk`, `role` | Nur während die Platte dreht: Unraid meldet keine für eine schlafende Platte. |
| `disk_spinning` | `disk`, `role` | 1, wenn die Platte dreht. |
| `disk_size_bytes`, `disk_fs_size_bytes`, `disk_used_percent` | `disk`, `role` | Größe, Dateisystemgröße und Füllstand, für Platten mit einem Dateisystem. |
| `disks`, `disks_problem`, `disks_with_errors` | | Zählungen. |
| `parity_check_status_info` | `status` | Wert 1: `completed`, `running`, `paused`, `cancelled`, `failed`, `never_run`. |
| `parity_check_running`, `parity_check_progress_percent` | | Eine laufende Prüfung. |
| `parity_check_ok` | | Nach einer abgeschlossenen Prüfung: 1 ohne Sync-Fehler, 0 mit Fehlern oder wenn sie fehlschlug. Eine abgebrochene Prüfung beweist nichts und setzt nichts. |
| `parity_check_errors`, `parity_check_age_seconds`, `parity_check_duration_seconds` | | Die letzte Prüfung. |
| `containers`, `containers_running`, `containers_autostart_stopped` | | Docker-Container. |
| `container_running`, `container_autostart_stopped` | `container` | Pro Container; das zweite ist 1, wenn ein zum Start mit dem Array vorgesehener Container nicht läuft. |
| `vms`, `vms_running` | | VMs. |
| `vm_state_info`, `vm_crashed` | `vm` (und `state`) | Pro VM. |
| `notifications_unread` | `importance`: `alert`, `warning`, `info` | Ungelesene Benachrichtigungen. |
| `registration_ok` | | 1, wenn Unraid kein Lizenzproblem meldet, 0, wenn der Lizenzstatus ein Fehler ist (abgelaufene Testlizenz, Schlüssel passt nicht zum USB-Stick, fehlende Schlüsseldatei…). |
| `registration_info` | `state`, `type` | Wert 1: der Lizenzstatus, wie Unraid ihn meldet (`trial`, `pro`, `eexpired`, `eguid`…), und der Lizenztyp. |
| `trial_expiry_seconds` | | Bis die Testlizenz endet; nur während einer Testphase. |
| `os_updates_expiry_seconds` | | Bis der Anspruch der Lizenz auf Unraid-OS-Updates endet; fehlt, wenn die Lizenz kein solches Datum hat. |
| `uptime_seconds` | | Seit dem Start des Servers. |
| `cpu_usage_percent` | | Prozessorlast, alle Kerne zusammen. |
| `memory_total_bytes`, `memory_available_bytes`, `memory_used_percent` | | Arbeitsspeicher. Als belegt zählt, was nicht verfügbar ist: Der Cache, den Unraid zurückgeben kann, wird nicht mitgezählt. |
| `swap_used_percent` | | Nur wenn der Server Swap hat. |

Die Lizenz und die Systemlast (Betriebszeit, Prozessor, Arbeitsspeicher) stammen aus zwei weiteren, optionalen Abfragen. Eine ältere API-Version, die sie nicht kennt, lässt nur diese Zeilen weg: Der Rest des Monitors ist nicht betroffen.

Die geltenden [eingebauten Regeln](../alerting/rules.md#unraid):

| Regel | Was | Standardschwelle | Haltezeit | Schweregrad | Erinnerung |
|---|---|---|---|---|---|
| Unraid-Array gestoppt | Das Unraid-Array ist nicht gestartet: Darauf liegende Shares, Container und VMs sind nicht verfügbar (`dumbmonit_unraid_array_started`). | < 1 | 10 Min. | Warnung (`critical`) | 6 h |
| Unraid-Platte deaktiviert oder fehlt | Eine Unraid-Platte ist deaktiviert, fehlt oder ist ungültig: Das Array läuft degradiert, und ein weiterer Ausfall verliert Daten (`dumbmonit_unraid_disk_ok`). | < 1 | 5 Min. | Warnung (`critical`) | 6 h |
| Unraid-Platte Lesefehler | Eine Unraid-Platte hat seit dem Start des Arrays Lesefehler gemeldet: Beim nächsten wird sie möglicherweise deaktiviert (`dumbmonit_unraid_disk_errors`). | > 0 | 5 Min. | Hinweis (`warning`) | 1 T. |
| Unraid-Platte zu heiß | Eine Unraid-Array- oder Paritätsplatte lag fünfzehn Minuten lang über 55 °C (`dumbmonit_unraid_disk_temperature_celsius{role=~"data\|parity"}`). | > 55 °C | 15 Min. | Hinweis (`warning`) | 6 h |
| Unraid-Paritätsprüfung hat Fehler gefunden | Die letzte Unraid-Paritätsprüfung ist fehlgeschlagen oder hat Sync-Fehler gefunden: Die Parität stimmt nicht mehr mit den Daten überein (`dumbmonit_unraid_parity_check_ok`). | < 1 | 5 Min. | Hinweis (`warning`) | 1 T. |
| Unraid-Paritätsprüfung überfällig | Die letzte Unraid-Paritätsprüfung ist mehr als 40 Tage alt: Eine monatliche Prüfung wurde verpasst (`dumbmonit_unraid_parity_check_age_seconds`). | > 40 T. | 1 h | Hinweis (`warning`) | 1 T. |
| Unraid-Cache fast voll | Ein Unraid-Cache-Pool ist zu mehr als 90 % voll: Schreibvorgänge fallen auf das langsame Array zurück oder scheitern (`dumbmonit_unraid_disk_used_percent{role="cache"}`). | > 90 % | 30 Min. | Hinweis (`warning`) | 1 T. |
| Unraid-Container gestoppt | Ein Docker-Container, der mit dem Array starten soll, läuft nicht (`dumbmonit_unraid_container_autostart_stopped`). | > 0 | 10 Min. | Hinweis (`warning`) | 6 h |
| Unraid-VM abgestürzt | Unraid meldet eine virtuelle Maschine als abgestürzt (`dumbmonit_unraid_vm_crashed`). | > 0 | 5 Min. | Hinweis (`warning`) | 6 h |
| Ungelesener Unraid-Alarm | Unraid hat eine Benachrichtigung der höchsten Wichtigkeit (alert) ausgelöst, die niemand gelesen hat: ausgefallene Platte, Überhitzung, Array-Fehler… (`dumbmonit_unraid_notifications_unread{importance="alert"}`). | > 0 | 10 Min. | Hinweis (`warning`) | 24 h |
| Unraid-Array fast voll | Das Array ist zu mehr als 95 % voll: Schreibvorgänge auf die Freigaben scheitern bald. Endet unter 93 % (`dumbmonit_unraid_array_used_percent`). | > 95 % | 1 h | Hinweis (`warning`) | 24 h |
| Unraid-Arbeitsspeicher fast erschöpft | Seit fünfzehn Minuten sind mehr als 95 % des Arbeitsspeichers belegt: Unraid läuft aus dem Arbeitsspeicher, und Container oder VMs werden beendet, wenn er ausgeht. Endet unter 90 % (`dumbmonit_unraid_memory_used_percent`). | > 95 % | 15 Min. | Hinweis (`warning`) | 6 h |
| Unraid-Lizenzproblem | Unraid meldet ein Lizenzproblem (abgelaufene Testlizenz, Schlüssel passt nicht zum USB-Stick, fehlende Schlüsseldatei…): Das Array verweigert nach dem nächsten Stopp womöglich den Start (`dumbmonit_unraid_registration_ok`). | < 1 | 30 Min. | Hinweis (`warning`) | 24 h |
| Unraid-Testlizenz endet bald | Die Testlizenz endet in weniger als drei Tagen: Danach startet das Array nicht mehr (`dumbmonit_unraid_trial_expiry_seconds`). | < 3 T. | 1 h | Hinweis (`warning`) | 24 h |
| Anspruch auf Unraid-OS-Updates endet | Der Anspruch der Lizenz auf Unraid-OS-Updates endet in weniger als 30 Tagen. Der Server läuft weiter; er erhält nur keine neuen Versionen mehr (`dumbmonit_unraid_os_updates_expiry_seconds`). | < 30 T. | 1 h | Info (`info`) | 7 T. |

## Die Monitorseite

Das Array-Panel sagt in einem Satz, was nicht stimmt, zeigt den
Array-Füllstand, die letzte Paritätsprüfung, die laufenden Container
und VMs, listet dann jede Platte — Parität zuerst, dann Daten, dann
Cache — mit ihrem Status, Temperatur und Füllstand, und die Container,
die laufen sollten.

## Einen nur-lesenden API-Key für DumbMonit erstellen

1. Auf Unraid 7.2 oder später ist die API eingebaut. Auf Unraid 6.12
   bis 7.1 installieren Sie zuerst das Unraid-Connect-Plugin aus dem
   Apps-Tab: Es bringt dieselbe API, und für lokale Nutzung ist kein
   Unraid-Connect-Login nötig.

2. Erstellen Sie einen API-Key mit der Viewer-Rolle, die alles liest
   und nichts ändern kann: unter Settings > Management Access > API
   Keys, oder vom Unraid-Terminal.

    ```
    unraid-api apikey --create --name dumbmonit --roles VIEWER --description "DumbMonit monitoring" --json
    ```

3. Kopieren Sie den Key. Geben Sie in DumbMonit die Adresse des
   Unraid-Servers ein, zum Beispiel "tower.lan", und fügen Sie den Key
   als API-Key ein. Wird die Web-Oberfläche über HTTPS bedient, wählen
   Sie https als Protokoll.

!!! warning
    Unraid meldet keine Temperatur für eine Platte, die
    herunterdreht, und DumbMonit weckt nie eine: Eine schlafende Platte
    hat einfach keine Temperatur, bis sie wieder dreht.

## Credentials

| Credential | Felder |
|---|---|
| API-Key | Ein Key mit der Viewer-Rolle, als `x-api-key` gesendet. |

Adresse: der Hostname oder die IP des Unraid-Servers, `host:port`, oder
eine URL. Optionen:

- **Protocol** (`http`) und **Port** (80): die der Web-Oberfläche. Mit
  SSL auf Strict unter Settings > Management Access, nutzen Sie
  `https`, und die `myunraid.net`-Adresse, wenn das Zertifikat diese
  ist.
- **Accept an unverifiable certificate**: für ein selbstsigniertes
  Zertifikat.
- **Timeout pro Request** (15 s).

## Bekannte Grenzen

- Die USV wird nicht gelesen: Binden Sie sie stattdessen über [NUT](nut.md) an.
- Keine SMART-Details: DumbMonit liest den Status, die Fehler und die Temperatur, die Unraid für jede Platte angibt, nicht die SMART-Attribute.
- Ob für einen Container ein Image-Update ansteht, wird nicht gelesen.
- Die Werte zu Lizenz, Prozessor und Arbeitsspeicher brauchen eine aktuelle Version der Unraid-API. Bei einer Version, der sie fehlen, fehlen diese Zeilen und ihre Regeln einfach.

## Fehlerbehebung

"Unraid refused the API key (401)": der Key ist falsch oder wurde
gelöscht.

"Unraid accepted the API key but refused to answer (403)": der Key hat
keine Rolle, oder eine engere als Viewer.

Keine Container-, VM- oder Paritätszahl: diese optionale Abfrage ist
fehlgeschlagen (VMs ausgeschaltet, eine ältere API-Version ohne das
Feld); der Rest wird trotzdem gelesen. Der Grund steht im Server-Log
auf Debug-Ebene.
