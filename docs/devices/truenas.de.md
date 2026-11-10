# TrueNAS

ZFS-Storage-Server (TrueNAS SCALE, jetzt TrueNAS Community Edition):
die Gesundheit jedes Pools und der ausgefallenen Platte, Scrubs und
Resilvers, Pool- und Dataset-Nutzung gegen ihre Kontingente, Snapshots
und Replikation, Plattentemperatur und SMART-Selbsttests, die von
TrueNAS selbst gehobenen Alarme, Dienste und die Maschine.

!!! warning "Nur gegen eine Testinstanz getestet"

    Diese Integration wurde gegen TrueNAS 25.04.2.6 gebaut und
    verifiziert, von der offiziellen ISO zu diesem Zweck in einer
    virtuellen Maschine installiert — ein RAIDZ1-Pool, Datasets,
    Snapshot- und Replikationstasks, dann derselbe Pool mit einer
    offline genommenen Platte — nicht gegen ein NAS, das seit Jahren
    die Fotos eines Haushalts hält. Eine virtuelle Maschine hat keine
    Plattentemperaturen und keine SMART-Ergebnisse, daher folgen diese
    TrueNAS' Quellcode statt einer beobachteten Antwort. Jeder Aufruf
    ist ein Lesevorgang. Sagen Sie uns, was kaputt ist.

## Der Fehler, für den das ist

Ein Mirror- oder RAIDZ-Vdev, das eine Platte verliert, liefert die
Daten weiter. Die Freigabe bleibt eingehängt, Backups laufen weiter,
die Web-Oberfläche jedes anderen Tools bleibt grün — und der Pool hat
nun keine Redundanz mehr. Die zweite Platte nimmt den Pool mit.
`truenas_pool_healthy` fällt auf 0 bei der ersten, die Regel **NAS
pool degraded** feuert, und die Monitorseite benennt die Platte.

## Die Monitorseite

Über die generischen Diagramme hinaus zeigt ein TrueNAS-Monitor drei
Panels, aus dem gelesen, was die Abfrage gespeichert hat — das Öffnen
der Seite fragt das NAS selbst nie ab:

* **Pools and disks** — jeder Pool mit seinem Zustand als Wort, ZFS'
  eigenem Satz, wenn etwas nicht stimmt, wie voll er ist, sein Layout
  (Mirror, RAIDZ, Cache, Log), seine Lese-, Schreib- und
  Prüfsummenfehlerzahlen, und vor allem der Name jeder Platte, die
  nicht `ONLINE` ist. Ein laufender Scrub oder Resilver zeigt seinen
  Fortschritt. Dann jede Platte: Modell, Seriennummer, Art, Größe,
  Pool, Temperatur und das Ergebnis ihres letzten SMART-Selbsttests.
* **Datasets and protection** — jedes Dataset mit seiner Nutzung
  gegen sein Kontingent, seiner Snapshot-Anzahl und wann ein
  Snapshot-Task es zuletzt abdeckte; der letzte Scrub jedes Pools und
  ob er überfällig ist; jeder Replikations- und periodische
  Snapshot-Task mit seinem Zustand, seinem letzten Lauf, dem letzten
  von ihm behandelten Snapshot und, wenn er fehlschlug, TrueNAS'
  eigenem Fehlersatz.
* **NAS health** — die von TrueNAS selbst gehobenen Alarme, am
  schwersten zuerst, dann die gestoppten Dienste, dann die Maschine:
  Version, Uptime, Last, Speicher, CPU.

## Was beobachtet wird

Alle Metriken haben das Präfix `dumbmonit_truenas_`.

| Familie | Metriken | Labels |
|---|---|---|
| Identität | `version_info` (die Version, ohne das `TrueNAS-SCALE-`-Präfix, das ältere Releases voranstellen) | `version` |
| Maschine | `uptime_seconds`, `load1/5/15`, `memory_total_bytes`, `cpu_count` | |
| Pools | `pool_healthy` (ZFS' eigenes Urteil: 0 bei `DEGRADED` wie bei `FAULTED`), `pool_warning` (Resilvering, Features nicht aktiviert), `pool_status_info`, `pool_size/allocated/free_bytes`, `pool_used_percent`, `pool_fragmentation_percent` | `pool`, und `status` für die Info-Zeitreihe |
| Pool-Geräte | `pool_device_errors` (kumuliert seit dem letzten `zpool clear`), `pool_devices_unhealthy` (beide fehlen, wenn der Pool nicht geöffnet werden kann) | `pool`, `kind` (`read`, `write`, `checksum`) |
| Scrubs | `pool_scan_running`, `pool_scan_percent` (nur während des Laufens), `pool_last_scrub_age_seconds`, `pool_last_scrub_errors`, `pool_scrub_threshold_days` | `pool`, `function` (`SCRUB`, `RESILVER`) |
| Datasets | `dataset_used_bytes`, `dataset_available_bytes`, `dataset_quota_bytes` und `dataset_quota_used_percent` (fehlt ohne Kontingent), `dataset_snapshots`, `dataset_snapshot_age_seconds` (Alter des letzten von einem das Dataset abdeckenden periodischen Task erstellten Snapshots; fehlt, wenn keiner das tut), `dataset_locked` (nur verschlüsselte Datasets) | `dataset`, `pool` |
| Snapshots | `snapshots` (gesamt) | |
| Platten | `disk_temperature_celsius`, `disk_size_bytes`, `disk_smart_failed` (fehlt für eine nie getestete Platte) | `disk`, `serial` |
| Alarme | `alerts` — die Anzahl aktiver Alarme pro Stufe, null eingeschlossen | `level` (`INFO` … `EMERGENCY`) |
| Tasks | `replication_error`, `replication_last_run_age_seconds`, `snapshot_task_error`, `snapshot_task_last_run_age_seconds` (nur aktivierte Tasks) | `task` |
| Dienste | `service_running` (nur zum Start mit dem NAS vorgesehene Dienste) | `service` |
| Sammlung | `up`, `scrape_errors`, `scrape_duration_seconds` | |

Ein paar Dinge, die TrueNAS tut und wissenswert sind:

* **Der letzte Scrub lebt nur im Scan-Datensatz des Pools.** Es gibt
  keine Scrub-Historie in der API, und ein Resilver überschreibt den
  Scan-Datensatz: Nach einem Resilver ist das Datum des letzten Scrubs
  einfach unbekannt, bis der nächste fertig ist, und
  `pool_last_scrub_age_seconds` fehlt statt falsch zu sein.
* **Ein Pool, der nicht geöffnet werden kann** — exportiert, oder mit
  zu vielen fehlenden Platten — erscheint trotzdem, ohne Layout und
  ohne Größen. Sein `pool_healthy` ist 0; die Geräte- und
  Größen-Zeitreihen fehlen statt null zu sein.
* **Ein nicht gesetztes Kontingent ist kein Kontingent von null.**
  TrueNAS antwortet mit `parsed: null` für "kein Kontingent", und
  DumbMonit veröffentlicht für dieses Dataset keine Kontingent-
  Zeitreihe. `quota` hat Vorrang vor `refquota`, wenn beide gesetzt
  sind.
* **Snapshot-Zählungen kommen aus ZFS' eigenem Cache**
  (`snapshots_count`), und die Gesamtzahl aus TrueNAS' schneller
  Snapshot-Zählung — kein Snapshot wird je aufgelistet. Die Gesamtzahl
  ist größer als die Summe der Pro-Dataset-Zählungen: Sie schließt die
  Snapshots von TrueNAS' internen Datasets ein, die die
  Dataset-Liste verbirgt.
* **Das Alter des neuesten Snapshots eines Datasets** ist der letzte
  Lauf des periodischen Snapshot-Tasks, der es abdeckt, direkt oder
  über ein rekursives übergeordnetes Dataset. TrueNAS kann das Datum
  des neuesten Snapshots nicht geben, ohne sie alle aufzulisten, daher
  hat ein von keinem Task abgedecktes Dataset kein Alter — was selbst
  wissenswert ist.
* **Temperaturen werden aus TrueNAS' Cache gelesen** (`only_cached`),
  alle fünf Minuten oder so von TrueNAS aktualisiert. Eine schlafende
  Platte wird nie geweckt, und eine Platte ohne Messwert hat keine
  Zeitreihe.
* **Nichts wird je gestartet.** Kein Scrub, kein SMART-Test, kein
  Update-Check, kein Replikationslauf. Die Abfrage liest.
* **Pfade, die sich zwischen Versionen änderten, sind optional.**
  `zfs/snapshot` wurde in 25.10 zu `pool/snapshot`, und beide werden
  versucht; `smart/test/results` verschwand in 25.10 und wird dann
  still übersprungen.

Geltende eingebaute Regeln: Monitor unreachable, NAS pool degraded,
NAS disk errors, NAS pool almost full, NAS scrub found errors, NAS
scrub overdue, NAS disk failed its SMART test, NAS disk too hot, NAS
dataset near its quota, NAS replication failed, NAS snapshot task
failed, NAS snapshots stale, TrueNAS alert raised, NAS service
stopped. Benachrichtigungen benennen den betroffenen Pool, die
Platte, das Dataset, den Task oder den Dienst.

## Was in TrueNAS vorzubereiten ist

Die Schritte unten sind die, die der Hinweis neben dem Formular zeigt.
Eine Sache zuerst zu wissen: Über die REST-API, die DumbMonit nutzt,
**hat TrueNAS keinen nur-lesenden Key**. Seine nur-lesenden Rollen
wurden nur in seine neuere WebSocket-API eingebaut; ein Key, dessen
Benutzer ein Read-Only Administrator ist, wird mit einem leeren `403`
auf jedem REST-Aufruf abgelehnt. Das wurde auf TrueNAS 25.04.2.6
geprüft: ein Read-Only-Administrator-Key erhielt `403` auf allen 55
versuchten REST-Pfaden, und derselbe Key funktionierte über die
WebSocket-API. Der Key muss zu einem Benutzer mit dem Recht Local
Administrator gehören — daher erhält er einen eigenen Benutzer und
eine eigene Gruppe, und nichts sonst.

1. In der Web-Oberfläche: Credentials → Groups → Add. Benennen Sie die
   Gruppe wie folgt und geben Sie ihr das Recht Local Administrator.

    ```
    dumbmonit
    ```

2. Warum Local Administrator und nicht Read-Only Administrator:
   TrueNAS hat seine nur-lesenden Rollen nur in seine WebSocket-API
   eingebaut. Über die von DumbMonit genutzte REST-API wird ein
   nur-lesender Key auf jedem einzelnen Aufruf abgelehnt. DumbMonit
   selbst liest immer nur: Es startet nie einen Scrub, einen
   SMART-Test, ein Update oder eine Replikation.

3. Credentials → Users → Add. Benennen Sie das Konto wie folgt, machen
   Sie dumbmonit zu seiner primären Gruppe, und lassen Sie Shell-
   Zugriff, Sudo und SSH aus: Keines davon ist nötig.

    ```
    dumbmonit
    ```

4. Erstellen Sie den Key: Credentials → Users → API Keys → Add,
   wählen Sie den Benutzer dumbmonit und geben Sie dem Key einen
   Namen. TrueNAS zeigt den Key nur einmal: Kopieren Sie ihn. Auf
   TrueNAS 24.10 ist der Pfad das Benutzermenü oben rechts → API Keys
   → Add, und ein Key gehört keinem Benutzer.

5. Geben Sie in DumbMonit die NAS-Adresse ein, zum Beispiel
   "nas.lan", und fügen Sie den Key ein. Behalten Sie HTTPS: TrueNAS
   widerruft einen Key, den es je über einfaches HTTP von einer
   anderen Maschine empfängt.

!!! warning

    Dieser Key kann alles auf dem NAS ändern: TrueNAS bietet keinen
    nur-lesenden Key über seine REST-API. Behandeln Sie ihn wie das
    Passwort eines Kontos, das Ihre Pools löschen kann, behalten Sie
    ihn exklusiv für DumbMonit, und widerrufen Sie ihn von derselben
    Seite, falls er durchsickert. TrueNAS liefert auch ein
    selbstsigniertes Zertifikat: Wird die Verbindung deshalb
    abgelehnt, kreuzen Sie "Accept an unverifiable certificate" in
    den Optionen an.

Adresse: `192.168.1.20`, `nas.lan`, `nas.lan:8443`, `[fd00::20]` oder
eine vollständige URL. Das Schema `https` und Port 443 werden
hinzugefügt, falls fehlend.

TrueNAS hat angekündigt, dass die REST-API in TrueNAS 26 entfernt
wird, zugunsten der WebSocket-API. Bis dahin ist REST, was jedes
Release bis 25.10 bedient.

## Optionen

| Schlüssel | Label | Standard | Hilfe |
|---|---|---|---|
| `scheme` | Protocol | `https` | Behalten Sie HTTPS: TrueNAS widerruft einen über einfaches HTTP von einer anderen Maschine empfangenen API-Key. |
| `port` | Web interface port | `443` | Genutzt, falls die Adresse keinen Port angibt. |
| `insecure_tls` | Accept an unverifiable certificate | `false` | TrueNAS liefert standardmäßig ein selbstsigniertes Zertifikat. |
| `request_timeout_seconds` | Timeout per request (seconds) | `20` | Erlaubte Zeit für jeden API-Aufruf, von 1 bis 120. |
| `datasets` | Watch the datasets | `true` | Nutzung gegen Kontingent, Snapshot-Anzahl, letzte Snapshot-Änderung. |
| `disks` | Watch the disks | `true` | Inventar und zwischengespeicherte Temperaturen; eine schlafende Platte wird nie geweckt. |
| `smart` | Read the SMART test results | `true` | Der letzte Selbsttest jeder Platte; keiner wird je gestartet. |
| `alerts` | Read TrueNAS's own alerts | `true` | Die von TrueNAS selbst geführte Alarmliste, verworfene Alarme ausgelassen. |
| `tasks` | Watch replication and snapshot tasks | `true` | Zustand, letzter Lauf und letzter Snapshot jedes Tasks. |
| `services` | Watch the services | `true` | Zum Start mit dem NAS vorgesehene Dienste, und ob sie laufen. |

## Integrierte Alarmregeln

Diese Regeln werden mit DumbMonit ausgeliefert und sind von Anfang an aktiv. Jede lässt sich unter Warnungen bearbeiten oder abschalten; die [Referenz der integrierten Regeln](../alerting/rules.md#truenas) führt alle Regeln aller Integrationen auf.

| Regel | Was | Standardschwelle | Haltezeit | Schweregrad | Erinnerung |
|---|---|---|---|---|---|
| NAS-Pool degradiert | ZFS nennt einen Pool nicht mehr gesund (`dumbmonit_truenas_pool_healthy`). Ein degradierter Pool liefert seine Daten weiter aus, und genau deshalb merkt es niemand, bis die nächste Platte ausfällt. Die Monitorseite nennt die Platte. | < 1 | 5 Min. | Warnung (`critical`) | 6 h |
| NAS-Plattenfehler | Platten eines Pools haben seit dem letzten `zpool clear` Lese-, Schreib- oder Prüfsummenfehler gemeldet (`dumbmonit_truenas_pool_device_errors`, eine Serie pro Art). | > 0 | 15 Min. | Hinweis (`warning`) | 24 h |
| NAS-Pool fast voll | Ein Pool ist zu mehr als 85 % voll (`dumbmonit_truenas_pool_used_percent`). ZFS wird lange vor dem Vollwerden langsam. Löst sich bei 80 %. | > 85 % | 30 Min. | Hinweis (`warning`), eskaliert nach 7 T. | 24 h |
| NAS-Scrub hat Fehler gefunden | Der letzte Scrub eines Pools hat beschädigte Daten gefunden (`dumbmonit_truenas_pool_last_scrub_errors`). | > 0 | 5 Min. | Warnung (`critical`) | 24 h |
| NAS-Scrub überfällig | Ein Pool hat seit mehr als 45 Tagen keinen Scrub abgeschlossen (`dumbmonit_truenas_pool_last_scrub_age_seconds`). Fehlt nach einem Resilver, der die Aufzeichnung des letzten Scrubs löscht. | > 45 T. | 1 h | Info (`info`) | 7 T. |
| NAS-Platte hat ihren SMART-Test nicht bestanden | Ein SMART-Selbsttest im Log einer Platte ist fehlgeschlagen (`dumbmonit_truenas_disk_smart_failed`). Eine nie getestete Platte hat keine Serie. | > 0 | 5 Min. | Warnung (`critical`) | 24 h |
| NAS-Platte zu heiß | Eine Platte liegt über 55 °C (`dumbmonit_truenas_disk_temperature_celsius`, aus dem Cache von TrueNAS gelesen). Löst sich bei 50 °C. | > 55 °C | 15 Min. | Hinweis (`warning`) | 6 h |
| NAS-Dataset nahe seiner Quota | Ein Dataset hat mehr als 90 % seiner Quota verbraucht (`dumbmonit_truenas_dataset_quota_used_percent`): Bei 100 % scheitern seine Schreibvorgänge, obwohl der Pool noch Platz hat. Keine Quota, keine Serie. | > 90 % | 30 Min. | Hinweis (`warning`) | 24 h |
| NAS-Replikation fehlgeschlagen | Ein aktivierter Replikations-Task ist im Fehler (`dumbmonit_truenas_replication_error`): Die Kopie auf der anderen Seite wird älter. | > 0 | 10 Min. | Hinweis (`warning`) | 12 h |
| NAS-Snapshot-Task fehlgeschlagen | Ein aktivierter periodischer Snapshot-Task ist im Fehler (`dumbmonit_truenas_snapshot_task_error`). | > 0 | 10 Min. | Hinweis (`warning`) | 12 h |
| NAS-Snapshots veraltet | Ein aktivierter periodischer Snapshot-Task ist seit mehr als acht Tagen nicht gelaufen (`dumbmonit_truenas_snapshot_task_last_run_age_seconds`): Ein Task, der aufhört zu laufen, schlägt nicht fehl, er wird still. | > 8 T. | 1 h | Hinweis (`warning`) | 24 h |
| TrueNAS-Alarm ausgelöst | TrueNAS selbst hat einen Alarm der Stufe ERROR oder höher ausgelöst (`dumbmonit_truenas_alerts`). Sein eigener Satz steht auf der Monitorseite. | > 0 | 10 Min. | Hinweis (`warning`) | 12 h |
| NAS-Dienst gestoppt | Ein Dienst, der mit dem NAS starten soll, ist gestoppt (`dumbmonit_truenas_service_running`). Ein absichtlich abgeschalteter Dienst hat keine Serie. | < 1 | 10 Min. | Hinweis (`warning`) | 6 h |
| NAS-Zertifikat läuft bald ab | Das Zertifikat, das die TrueNAS-Weboberfläche ausliefert, läuft in weniger als 14 Tagen ab: Browser, Apps und Replikationspartner werden es ablehnen (`dumbmonit_truenas_certificate_expiry_days{in_use="1"}`). | < 14 T. | 1 h | Hinweis (`warning`) | 24 h |
| NAS-Zertifikat abgelaufen | Das Zertifikat, das die TrueNAS-Weboberfläche ausliefert, läuft in weniger als 2 Tagen ab oder ist bereits abgelaufen (`dumbmonit_truenas_certificate_expiry_days{in_use="1"}`). | < 2 T. | 15 Min. | Warnung (`critical`) | 12 h |
| NAS-Boot-Pool degradiert | Der Boot-Pool ist nicht mehr gesund: Das NAS startet noch vom verbleibenden Gerät, bis auch dieses ausfällt (`dumbmonit_truenas_boot_pool_healthy`). | < 1 | 10 Min. | Hinweis (`warning`) | 12 h |
| NAS-Boot-Pool fast voll | Der Boot-Pool ist zu mehr als 85 % voll: Alte Boot-Umgebungen sammeln sich mit jedem Update an und blockieren das nächste. Endet unter 80 % (`dumbmonit_truenas_boot_pool_used_percent`). | > 85 % | 1 h | Hinweis (`warning`) | 24 h |
| NAS-Aufgabe für externe Sicherung fehlgeschlagen | Der letzte Lauf einer Cloud-Sync-, Rsync- oder Cloud-Backup-Aufgabe ist fehlgeschlagen: Die externe Kopie veraltet (`dumbmonit_truenas_backup_task_failed`). | > 0 | 10 Min. | Hinweis (`warning`) | 12 h |
| NAS-App abgestürzt | Eine App ist im Zustand „abgestürzt“ (crashed) (`dumbmonit_truenas_app_crashed`). | > 0 | 10 Min. | Hinweis (`warning`) | 6 h |
| Virtuelle Maschine des NAS gestoppt | Eine virtuelle Maschine, die mit dem NAS starten soll, läuft nicht (`dumbmonit_truenas_vm_running`). | < 1 | 10 Min. | Hinweis (`warning`) | 6 h |

## Bekannte Grenzen

- Über die REST-API, die DumbMonit verwendet, kennt TrueNAS keinen Nur-Lese-Schlüssel: Der Schlüssel gehört einem vollwertigen Administrator. DumbMonit liest ausschließlich und startet nie einen Scrub, einen SMART-Test oder eine Replikation.
- Das Alter des neuesten Snapshots eines Datasets ist der letzte Lauf der Snapshot-Aufgabe, die es abdeckt. Ein Dataset, das keine Aufgabe abdeckt, hat kein Alter.
- Temperaturen stammen aus dem Cache von TrueNAS, der etwa alle fünf Minuten aufgefrischt wird. Eine schlafende Platte wird nie aufgeweckt.
- Die Ergebnisse der SMART-Tests sind mit TrueNAS 25.10 aus der API verschwunden und werden ab dieser Version übersprungen.
- Die Zertifikatsregeln überwachen nur das Zertifikat, das die Weboberfläche ausliefert.

## Häufige Fehler

| Symptom | Wahrscheinliche Ursache |
|---|---|
| Authentifizierungsfehler, der Local Administrator erwähnt | Der Benutzer des Keys ist kein vollständiger Administrator — typischerweise ein Read-Only Administrator. TrueNAS lehnt einen solchen Key auf jedem REST-Aufruf mit einem leeren `403` ab. |
| Authentifizierungsfehler "Invalid API key" | Der Key wurde falsch eingegeben, gelöscht, ist abgelaufen — oder wurde von TrueNAS widerrufen, weil er einmal über einfaches HTTP von einer anderen Maschine gesendet wurde. Erstellen Sie einen neuen und behalten Sie HTTPS. |
| Zertifikatsfehler | Selbstsigniertes Zertifikat: installieren Sie ein vertrauenswürdiges (ACME ist eingebaut) oder aktivieren Sie `insecure_tls`. |
| Keine Plattentemperaturen | Das NAS läuft in einer virtuellen Maschine, oder TrueNAS hat die Platten seit dem Start noch nicht abgefragt. Messwerte kommen aus seinem Cache. |
| Keine SMART-Zeitreihe | Auf dieser Platte lief noch kein Selbsttest, oder das NAS läuft mit TrueNAS 25.10, das diesen Aufruf entfernt hat. Planen Sie SMART-Tests unter Data Protection. |
| Kein `pool_last_scrub_age_seconds` | Der letzte Scan dieses Pools war ein Resilver, der den Eintrag des letzten Scrubs löscht, oder der Pool wurde nie gescrubbt. |
| Ein Dataset hat keine Kontingent-Zeitreihe | Es hat kein Kontingent. Ein nicht gesetztes Kontingent ist kein Kontingent von null. |
| "NAS pool degraded" und die Seite zeigt keinen Plattennamen | Der Pool kann überhaupt nicht geöffnet werden (exportiert, oder zu viele fehlende Platten): TrueNAS meldet dann kein Layout. |
| `scrape_errors` über null | Ein optionaler Aufruf schlug aus einem anderen Grund als einem fehlenden Pfad fehl — meist ein Timeout. Die anderen Metriken sind nicht betroffen. |
