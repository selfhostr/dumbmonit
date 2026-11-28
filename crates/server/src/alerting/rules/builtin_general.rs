//! Règles livrées générales : injoignable, processeur, disques, onduleur,
//! ports, Redfish, premières règles PBS, services et heartbeats, certificats,
//! conteneurs, sauvegardes, VPN, débit, journaux Windows, SMART et ZFS.

use super::*;

/// Les règles livrées de cette tranche, dans l'ordre du catalogue.
///
/// Le tableau est mis en page à la main, comme lorsqu'il tenait dans un seul
/// fichier : rustfmt n'y touche pas.
#[rustfmt::skip]
pub(super) fn rules() -> Vec<Rule> {
    vec![
        // Équipement injoignable.
        //
        // On ne cherche pas un `up == 0` : le planificateur n'écrit rien quand
        // l'interrogation échoue, la série s'arrête simplement. On mesure donc l'âge
        // du dernier échantillon, ce qui détecte aussi bien une panne réseau qu'un
        // collecteur bloqué. La fenêtre de sept jours borne le coût de la requête ;
        // au-delà, la cible n'a plus de série du tout et l'interface prend le relais.
        //
        // La règle est par cible, pas par série : sept jours de fenêtre gardent
        // aussi les variantes que plus personne n'écrit (tags retouchés, options
        // qui ont cessé d'être exportées, renommage). Leur âge ne fait que croître
        // et elles sonnaient une semaine durant, la cible bien vivante. `bottomk`
        // ne garde que la plus fraîche de chaque cible, avec ses étiquettes : les
        // silences par tag et le nom affiché continuent de fonctionner.
        //
        // Le seuil de trois minutes est déjà l'anti-faux-positif (deux sondes
        // manquées à la période par défaut) ; le `for` ne fait que confirmer au
        // cycle suivant, trente secondes suffisent. Une minute de plus retardait
        // chaque première alerte sans écarter de fausse panne.
        //
        // Le seuil suit la période de chaque cible (trois périodes manquées, voir
        // `model::host_down_threshold`) : un nom de domaine consulté toutes les
        // heures n'est pas injoignable trois minutes après une sonde réussie.
        Rule {
            description: "No measurement received for more than three minutes, or three probe \
                          intervals for monitors checked less often."
                .to_string(),
            operator: Operator::Gt,
            threshold: 180.0,
            for_duration: Duration::from_secs(30),
            severity: Severity::Critical,
            unit: "s".to_string(),
            ..base(
                RULE_HOST_DOWN,
                "Monitor unreachable",
                RuleKind::Threshold,
                HOST_DOWN_QUERY,
            )
        },
        // Processeur élevé. L'agrégation ramène les cœurs à une seule série par
        // équipement : personne ne veut vingt-quatre alertes pour un seul serveur.
        Rule {
            description: "CPU load sustained above 90%.".to_string(),
            operator: Operator::Gt,
            threshold: 90.0,
            // Hystérésis : une charge qui oscille entre 88 et 92 % ne doit pas
            // déclencher et résoudre à chaque cycle.
            clear_threshold: Some(85.0),
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            escalate_after: Some(Duration::from_secs(3600)),
            ..base("cpu_high", "High CPU", RuleKind::Threshold, CPU_PERCENT)
        },
        // Disque presque plein, par point de montage : ici on veut bien une série
        // par système de fichiers, la remédiation n'étant pas la même.
        Rule {
            description: "Filesystem 90% full or more.".to_string(),
            operator: Operator::Ge,
            threshold: 90.0,
            clear_threshold: Some(88.0),
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            escalate_after: Some(Duration::from_secs(24 * 3600)),
            ..base("disk_almost_full", "Disk almost full", RuleKind::Threshold, FS_USED_PERCENT)
        },
        // Onduleur sur batterie : `for` très court, la coupure est déjà l'événement.
        Rule {
            description: "The UPS is powering the load from battery.".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(30),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(900)),
            ..base(
                "ups_on_battery",
                "UPS on battery",
                RuleKind::Threshold,
                // upsOutputSource : 5 = batterie (RFC 1628). La comparaison rend une
                // série valant 1 seulement quand la condition est vraie, ce qui donne
                // bien « supérieur à 0 » comme test de déclenchement.
                "dumbmonit_ups_output_source == 5",
            )
        },
        // Prédictif : tout le calcul est fait par VictoriaMetrics. Le `and deriv(...)`
        // écarte les systèmes de fichiers stables ou en décroissance, pour lesquels
        // l'extrapolation linéaire n'a aucun sens.
        Rule {
            description: "At this rate, the filesystem will be full within four days.".to_string(),
            operator: Operator::Ge,
            threshold: 100.0,
            for_duration: Duration::from_secs(30 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            ..base(
                "fs_will_be_full",
                "Filesystem almost full",
                RuleKind::Predict,
                &predict_full_query(FS_USED_PERCENT, 6, 4),
            )
        },
        // Anomalie saisonnière sur le processeur. Le seuil et la durée sont ceux du
        // jalon 5 ; la règle reste muette pendant les quatorze jours d'apprentissage.
        Rule {
            description: "CPU load noticeably different from the usual at this time \
                          and day of the week."
                .to_string(),
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Info,
            unit: "%".to_string(),
            ..base("cpu_anomaly", "Unusual CPU", RuleKind::Anomaly, CPU_PERCENT)
        },
        // Prévision de remplissage, commune à toutes les intégrations : disques,
        // datastores, pools, volumes, licences, autonomie des onduleurs (voir
        // `capacity.rs`). Information sous trente jours, avertissement sous
        // quatorze, critique sous sept : la sévérité monte d'elle-même avec les
        // paliers et l'alerte reparle à chaque franchissement.
        Rule {
            description: "At the current growth rate, this storage (disk, datastore, pool, \
                          volume, licences…) will be full within thirty days."
                .to_string(),
            operator: Operator::Lt,
            threshold: 30.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Info,
            unit: "d".to_string(),
            repeat_interval: Some(Duration::from_secs(7 * 24 * 3600)),
            ..base(RULE_CAPACITY_FORECAST, "Storage filling up", RuleKind::Forecast, "*")
        },
        // Dérive de la latence des sondes réussies (HTTP, ping, TCP, DNS…) par
        // rapport à la même heure des quatre semaines précédentes. Trois fois la
        // normale, pendant quinze minutes, et au moins 50 ms d'écart : une
        // latence qui passe de 2 à 6 ms ne réveille personne.
        Rule {
            description: "Response time three times higher than usual for this time of day \
                          and day of the week."
                .to_string(),
            operator: Operator::Ge,
            threshold: 3.0,
            clear_threshold: Some(2.0),
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            unit: "×".to_string(),
            params: AnomalyParams {
                drift_min_value: 0.05,
                drift_min_delta: 0.05,
                ..AnomalyParams::default()
            },
            ..base("latency_drift", "Response time drifting", RuleKind::Drift, SERVICE_SLOW_QUERY)
        },
        // Dérive du taux de réponses 5xx d'un proxy ou d'une zone Cloudflare :
        // le seuil fixe de `proxy_5xx_rate` rate le site qui passe de 0,1 à 4 %.
        // Au moins un point de pourcentage d'écart, et le plancher de cinquante
        // requêtes de la requête elle-même.
        Rule {
            description: "The share of 5xx answers is three times higher than usual for this \
                          time of day."
                .to_string(),
            operator: Operator::Ge,
            threshold: 3.0,
            clear_threshold: Some(2.0),
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            unit: "×".to_string(),
            params: AnomalyParams {
                drift_min_value: 1.0,
                drift_min_delta: 1.0,
                ..AnomalyParams::default()
            },
            ..base("http_5xx_drift", "5xx errors drifting", RuleKind::Drift, HTTP_5XX_PERCENT)
        },
        // Consommation du budget d'erreur des SLO (voir `crate::slo`) : 14,4×
        // sur une heure et cinq minutes, critique (« Warning » à l'écran) ; 6×
        // sur six heures et trente minutes, `warning` (« Advisory »). Sans SLO
        // défini, la règle ne rend aucune série.
        Rule {
            description: "A service level objective is spending its error budget too fast: \
                          14.4 times the sustainable rate over the last hour and 5 minutes \
                          (warning), or 6 times over the last 6 hours and 30 minutes \
                          (advisory)."
                .to_string(),
            operator: Operator::Ge,
            threshold: 1.0,
            for_duration: Duration::from_secs(60),
            severity: Severity::Warning,
            ..base(RULE_SLO_BURN, "Error budget burning", RuleKind::SloBurn, "*")
        },
        // Batterie d'onduleur en fin de vie ou déchargée. Distincte de « sur
        // batterie » : une coupure brève est normale, une batterie basse annonce un
        // arrêt brutal des équipements alimentés.
        Rule {
            description: "The UPS battery is low or depleted.".to_string(),
            operator: Operator::Ge,
            threshold: 3.0,
            for_duration: Duration::from_secs(60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(1800)),
            ..base(
                "ups_battery_low",
                "UPS battery low",
                RuleKind::Threshold,
                // upsBatteryStatus : 3 = basse, 4 = épuisée (RFC 1628).
                "dumbmonit_ups_battery_status",
            )
        },
        // Port réseau qui accumule les erreurs (IF-MIB, tout équipement SNMP).
        //
        // `increase` sur une heure plutôt qu'un seuil sur le compteur brut : un
        // commutateur allumé depuis trois ans a forcément quelques erreurs, ce
        // qui compte est qu'elles continuent d'arriver. Le relevé réel d'un Zyxel
        // montre le cas type : un port au lien tombé, des milliers d'erreurs en
        // entrée, les ports voisins à zéro. Cinquante par heure écarte la trame
        // isolée d'un débranchement.
        //
        // `increase_prometheus` et non `increase` : sur une série neuve,
        // VictoriaMetrics compte la première valeur comme un accroissement quand
        // elle lui paraît petite — vérifié sur le relevé Zyxel, où un port à sept
        // erreurs historiques ressortait avec sept erreurs « dans l'heure » dès
        // l'ajout de l'équipement. La variante Prometheus exige deux points dans
        // la fenêtre : rien ne sonne pendant la première heure, puis seules les
        // erreurs réellement nouvelles comptent.
        Rule {
            description: "A network port logged more than fifty errors in the last hour: \
                          usually a failing cable, a dirty fibre or a duplex mismatch."
                .to_string(),
            operator: Operator::Gt,
            threshold: 50.0,
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "port_errors",
                "Port accumulating errors",
                RuleKind::Threshold,
                "increase_prometheus(dumbmonit_if_errors_in[1h]) \
                 + increase_prometheus(dumbmonit_if_errors_out[1h])",
            )
        },
        // Port qui bagote : chaque montée et chaque descente change ifOperStatus.
        // Plus de quatre changements en trente minutes, c'est au moins deux
        // coupures, ce qu'aucune maintenance ordinaire ne produit. Un port coupé
        // par l'administrateur disparaît des séries (filtre d'IF-MIB) et ne peut
        // donc pas déclencher. `changes_prometheus`, pour la même raison que
        // ci-dessus : `changes` compte la première valeur d'une série neuve.
        Rule {
            description: "A network port went down and up again more than twice in thirty \
                          minutes."
                .to_string(),
            operator: Operator::Gt,
            threshold: 4.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(3600)),
            ..base(
                "port_flapping",
                "Port flapping",
                RuleKind::Threshold,
                "changes_prometheus(dumbmonit_if_oper_status[30m])",
            )
        },
        // Matériel serveur vu par le contrôleur de gestion, via Redfish
        // (`collectors/redfish`). Les séries de santé valent 0 OK, 1 Warning,
        // 2 Critical ; un emplacement « Absent » ou « Disabled » n'en a pas.
        Rule {
            description: "The server's management controller reports a fan as failed \
                          (health Critical)."
                .to_string(),
            operator: Operator::Ge,
            threshold: 2.0,
            for_duration: Duration::from_secs(2 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "redfish_fan_failed",
                "Server fan failed",
                RuleKind::Threshold,
                "dumbmonit_redfish_fan_health",
            )
        },
        // Même principe que la règle de l'agent : le seuil est celui que le
        // capteur publie (UpperThresholdCritical), jamais une valeur en dur.
        Rule {
            description: "A server temperature sensor is above the critical threshold its \
                          management controller declares."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Critical,
            unit: "°C".to_string(),
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "redfish_temperature_critical",
                "Server temperature above critical",
                RuleKind::Threshold,
                "dumbmonit_redfish_temperature_celsius \
                 >= dumbmonit_redfish_temperature_upper_critical_celsius",
            )
        },
        // Redondance d'alimentation perdue : le serveur tourne encore, sur une
        // seule alimentation. C'est l'alerte qui laisse le temps d'agir.
        Rule {
            description: "The server's power supplies are no longer redundant: one more \
                          failure and it goes down."
                .to_string(),
            operator: Operator::Ge,
            threshold: 1.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Warning,
            escalate_after: Some(Duration::from_secs(3600)),
            ..base(
                "redfish_psu_redundancy_lost",
                "Power supply redundancy lost",
                RuleKind::Threshold,
                "dumbmonit_redfish_power_redundancy_health",
            )
        },
        Rule {
            description: "The server's management controller reports a power supply as \
                          failed (health Critical)."
                .to_string(),
            operator: Operator::Ge,
            threshold: 2.0,
            for_duration: Duration::from_secs(2 * 60),
            severity: Severity::Critical,
            ..base(
                "redfish_psu_failed",
                "Power supply failed",
                RuleKind::Threshold,
                "dumbmonit_redfish_psu_health",
            )
        },
        // FailurePredicted : le contrôleur de stockage (SMART, PFA) annonce la fin
        // prochaine du disque. Il fonctionne encore : c'est le moment de le changer.
        Rule {
            description: "A drive reports a predicted failure: replace it while it still \
                          works."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "redfish_drive_failure_predicted",
                "Drive failure predicted",
                RuleKind::Threshold,
                "dumbmonit_redfish_drive_failure_predicted",
            )
        },
        // La santé propre du système (Status.Health), pas son agrégat : le
        // HealthRollup redirait en double ce que les règles ci-dessus disent déjà
        // composant par composant.
        Rule {
            description: "The server's management controller reports the system health as \
                          Critical."
                .to_string(),
            operator: Operator::Ge,
            threshold: 2.0,
            for_duration: Duration::from_secs(2 * 60),
            severity: Severity::Critical,
            ..base(
                "redfish_system_critical",
                "Server health critical",
                RuleKind::Threshold,
                "dumbmonit_redfish_system_health",
            )
        },
        // Sauvegarde Proxmox trop ancienne. C'est le genre de panne silencieuse qui
        // ne se découvre qu'au moment de restaurer, quand il est trop tard.
        Rule {
            description: "No successful backup for more than seven days.".to_string(),
            operator: Operator::Gt,
            threshold: 7.0 * 24.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "backup_too_old",
                "Backup too old",
                RuleKind::Threshold,
                "dumbmonit_proxmox_backup_last_age_seconds",
            )
        },
        // Proxmox Backup Server. Les seuils sont ceux d'un homelab qui sauvegarde
        // chaque nuit : deux jours sans sauvegarde, c'est une nuit ratée plus la
        // marge d'une nuit ; huit jours sans GC, c'est une GC hebdomadaire manquée.
        Rule {
            description: "PBS datastore more than 90% full.".to_string(),
            operator: Operator::Gt,
            threshold: 90.0,
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            escalate_after: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "pbs_datastore_almost_full",
                "PBS datastore almost full",
                RuleKind::Threshold,
                "dumbmonit_pbs_datastore_used_percent",
            )
        },
        // L'estimation est calculée par PBS lui-même sur un mois de mesures : elle
        // est absente tant qu'il manque de points, la règle reste alors muette.
        Rule {
            description: "At the current rate, PBS estimates the datastore full within seven days."
                .to_string(),
            operator: Operator::Lt,
            threshold: 7.0 * 24.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "pbs_datastore_will_be_full",
                "PBS datastore filling up",
                RuleKind::Threshold,
                "dumbmonit_pbs_datastore_estimated_full_seconds",
            )
        },
        // Même logique que `backup_too_old` côté PVE, par groupe de sauvegarde.
        Rule {
            description: "No new snapshot for this machine for more than two days.".to_string(),
            operator: Operator::Gt,
            threshold: 2.0 * 24.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "pbs_backup_too_old",
                "PBS backup too old",
                RuleKind::Threshold,
                "dumbmonit_pbs_backup_last_age_seconds",
            )
        },
        // Critique : une sauvegarde dont la vérification échoue est une sauvegarde
        // que l'on ne pourra pas restaurer. La série vaut 1 (vérifiée) ou 0 (en
        // échec) et n'existe pas pour un instantané jamais vérifié : « < 1 » ne vise
        // donc que les vrais échecs. On ne filtre pas par `== 0` côté MetricsQL :
        // la série renvoyée garderait sa valeur 0, et aucun seuil « > 0 » ne la
        // verrait.
        Rule {
            description: "Verification of the latest snapshot for this machine failed.".to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(30 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "pbs_backup_verification_failed",
                "PBS backup verification failed",
                RuleKind::Threshold,
                "dumbmonit_pbs_backup_last_verified",
            )
        },
        Rule {
            description: "At least one PBS task failed in the review window.".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "pbs_task_failed",
                "PBS task failed",
                RuleKind::Threshold,
                "dumbmonit_pbs_tasks_failed",
            )
        },
        // Sans GC, les blocs des instantanés supprimés ne sont jamais libérés et
        // le datastore se remplit sans raison apparente.
        Rule {
            description:
                "No successful garbage collection (GC) on this datastore for more than eight days."
                    .to_string(),
            operator: Operator::Gt,
            threshold: 8.0 * 24.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "pbs_gc_too_old",
                "PBS garbage collection too old",
                RuleKind::Threshold,
                "dumbmonit_pbs_gc_last_success_age_seconds",
            )
        },
        // Moniteurs de disponibilité. Contrairement au matériel, un service en
        // panne écrit bien un point (`probe_success = 0`) : la règle est un simple
        // seuil, et le `for` absorbe un raté isolé. Deux minutes, soit trois
        // sondes en échec d'affilée à la période par défaut : un raté isolé, et
        // même deux, ne sonnent jamais. `== bool 0` rend 1 pour un
        // service en panne ; sans `bool`, la comparaison renverrait 0 (la valeur
        // de gauche) et le seuil « > 0 » ne se déclencherait jamais.
        //
        // Les heartbeats (`probe="push"`) écrivent la même série mais ont leur
        // propre règle, `push_missed`, en avertissement : un cron en retard n'est
        // pas une panne critique, et il ne doit pas alerter deux fois.
        Rule {
            description: "The service has not responded correctly for two minutes.".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(2 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(1800)),
            ..base(
                "service_down",
                "Service down",
                RuleKind::Threshold,
                "dumbmonit_probe_success{probe!=\"push\"} == bool 0",
            )
        },
        // Heartbeat manqué. Le collecteur `push` écrit `probe_success = 0` dès que
        // le dernier appel dépasse la période attendue plus la tolérance, ou que
        // le script a lui-même signalé `status=down` ; la règle n'a donc aucun
        // délai à connaître — il est propre à chaque cible. Le `for` court
        // n'absorbe que le battement du planificateur : la tolérance est déjà
        // dans le verdict.
        Rule {
            description: "The job has not called in within its expected interval or cron schedule plus grace period, or reported a failure itself (status=down)."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(2 * 60),
            severity: Severity::Warning,
            ..base(
                "push_missed",
                "Heartbeat missed",
                RuleKind::Threshold,
                "dumbmonit_probe_success{probe=\"push\"} == bool 0",
            )
        },
        // Exécution en échec : le travail a bien appelé (`/fail`, ou un code de
        // sortie non nul), c'est donc une panne du travail et non un silence. Le
        // moteur joint la sortie tronquée au message (`engine::occurrences`). La
        // série reste à 1 jusqu'à la prochaine exécution réussie.
        Rule {
            description: "The job's last run failed: it called /fail or finished with a non-zero exit code."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            // Un délai court, comme pour `webchange_detected` : il écarte un point isolé
            // sans retarder l'annonce d'un échec réel.
            for_duration: Duration::from_secs(30),
            severity: Severity::Warning,
            ..base(
                "push_failed",
                "Job failed",
                RuleKind::Threshold,
                "dumbmonit_push_run_failed{probe=\"push\"}",
            )
        },
        // Exécution trop longue : plus que la durée maximale réglée, ou, sans
        // elle, un `/start` resté sans fin au-delà d'une période plus la tolérance.
        Rule {
            description: "A run lasts longer than the maximum duration, or started and never finished in time."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            // Un délai court, comme pour `webchange_detected` : il écarte un point isolé
            // sans retarder l'annonce d'un échec réel.
            for_duration: Duration::from_secs(30),
            severity: Severity::Warning,
            ..base(
                "push_too_long",
                "Job too long",
                RuleKind::Threshold,
                "dumbmonit_push_run_too_long{probe=\"push\"}",
            )
        },
        // Instabilité : un service qui alterne sans cesse n'est jamais « en panne »
        // assez longtemps pour la règle précédente, et pourtant il est inutilisable.
        Rule {
            description: "The service changed state more than six times in thirty minutes."
                .to_string(),
            operator: Operator::Gt,
            threshold: 6.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(3600)),
            ..base(
                "service_flapping",
                "Service flapping",
                RuleKind::Threshold,
                "changes(dumbmonit_probe_success{probe!=\"push\"}[30m])",
            )
        },
        // Lenteur : le défaut de délai des sondes est de 5 s, au-delà elles échouent
        // déjà ; 3 s soutenues pendant dix minutes est un service qui souffre.
        // Seules les sondes réussies comptent : une sonde en échec dure le temps
        // de son délai, et ce n'est pas de la lenteur mais une panne, que
        // `service_down` porte déjà.
        Rule {
            description: "The service takes more than three seconds to respond.".to_string(),
            operator: Operator::Gt,
            threshold: 3.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            unit: "s".to_string(),
            ..base(
                "service_slow",
                "Slow service",
                RuleKind::Threshold,
                SERVICE_SLOW_QUERY,
            )
        },
        // Certificats. Les deux règles sont disjointes (`>= 0` / `< 0`) pour qu'un
        // certificat périmé ne déclenche pas aussi « bientôt expiré ».
        Rule {
            description: "The certificate expires in less than fourteen days.".to_string(),
            operator: Operator::Lt,
            threshold: 14.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "d".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "tls_cert_expiring",
                "Certificate expiring soon",
                RuleKind::Threshold,
                "dumbmonit_probe_ssl_cert_expiry_days >= 0",
            )
        },
        Rule {
            description: "The certificate has expired.".to_string(),
            operator: Operator::Lt,
            threshold: 0.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Critical,
            unit: "d".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "tls_cert_expired",
                "Certificate expired",
                RuleKind::Threshold,
                "dumbmonit_probe_ssl_cert_expiry_days < 0",
            )
        },
        // Conteneurs Docker, vus par l'agent. Chaque série porte le nom du
        // conteneur : l'alerte le montre, et une politique peut y répondre.
        Rule {
            description: "The container has been stopped for two minutes. A container that \
                          finished on its own and that Docker would not restart (no restart \
                          policy, clean exit or `docker stop`) is not reported."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(2 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(3600)),
            ..base(
                "container_stopped",
                "Container stopped",
                RuleKind::Threshold,
                // `== bool` rend 1 quand le conteneur est arrêté, 0 sinon ; sans
                // `bool`, MetricsQL renverrait la valeur de gauche — 0 — qu'un
                // seuil « > 0 » ne verrait jamais. Voir `service_down`.
                // `unless` écarte les arrêts voulus que l'agent signale
                // (`container_stop_expected`, conteneur ponctuel terminé) ; un
                // agent plus ancien n'émet pas cette série et tout reste visé.
                CONTAINER_STOPPED_QUERY,
            )
        },
        Rule {
            description: "The container's health check has been failing for three minutes."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(3 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(3600)),
            ..base(
                "container_unhealthy",
                "Container unhealthy",
                RuleKind::Threshold,
                // 2 = unhealthy (0 none, 1 healthy, 3 starting). Docker garde
                // le dernier verdict d'un conteneur arrêté : il n'a plus de
                // sens, et `container_stopped` dit déjà l'essentiel.
                "dumbmonit_container_health == 2 unless on(target, container) \
                 dumbmonit_container_up == 0",
            )
        },
        // Un conteneur qui redémarre en boucle est rarement « arrêté » assez
        // longtemps pour la règle précédente.
        Rule {
            description: "The container restarted three times or more in fifteen minutes."
                .to_string(),
            operator: Operator::Ge,
            threshold: 3.0,
            for_duration: Duration::from_secs(60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(3600)),
            ..base(
                "container_restarting",
                "Container restarting",
                RuleKind::Threshold,
                // `increase_prometheus` et non `increase` : ce dernier compte la
                // première valeur d'une série neuve comme une hausse. Un conteneur
                // qui a redémarré cinquante fois l'an dernier et dont la série
                // réapparaît (agent réinstallé, étiquettes changées) sonnerait
                // aussitôt « cinquante redémarrages en un quart d'heure ».
                "increase_prometheus(dumbmonit_container_restart_count[15m])",
            )
        },
        // Information, pas panne : une image plus récente existe dans le dépôt.
        // Une heure de `for` absorbe une vérification passagèrement fausse.
        Rule {
            description: "A newer image is available in the registry for this container."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Info,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "container_update_available",
                "Container update available",
                RuleKind::Threshold,
                "dumbmonit_container_update_available == 1",
            )
        },
        // Sauvegardes Plakar, par kloset et par source. Mêmes seuils que PBS :
        // deux jours, c'est une nuit ratée plus la marge d'une nuit. Les deux
        // règles ne visent que des séries étiquetées `kloset`, que l'agent
        // n'émet qu'une fois Plakar détecté : sans Plakar, rien ne peut se
        // déclencher.
        Rule {
            description: "No new Plakar snapshot for this source for more than two days."
                .to_string(),
            operator: Operator::Gt,
            threshold: 2.0 * 24.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "plakar_backup_too_old",
                "Plakar backup too old",
                RuleKind::Threshold,
                "dumbmonit_backup_last_success_seconds",
            )
        },
        // La série vaut 1 (dernier instantané sans erreur) ou 0 (erreurs, ou
        // kloset illisible) : « < 1 » ne vise que les vrais échecs.
        Rule {
            description: "The latest Plakar snapshot has errors.".to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "plakar_backup_failed",
                "Plakar backup failed",
                RuleKind::Threshold,
                // `source="*"` est la série d'un kloset illisible : ce n'est pas
                // une sauvegarde ratée, la règle suivante s'en charge.
                "dumbmonit_backup_last_status{source!=\"*\"}",
            )
        },
        // Un kloset que l'agent n'ouvre pas : stockage injoignable, phrase de
        // passe ou greffon manquant. Rien ne dit que les sauvegardes échouent —
        // seulement qu'on ne peut plus le vérifier : avertissement, après une
        // demi-heure, comme pour restic et Borg.
        Rule {
            description: "The agent cannot open this Plakar kloset: storage unreachable, \
                          passphrase or plugin missing (the agent's log says which)."
                .to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(30 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "plakar_kloset_unreadable",
                "Plakar kloset unreadable",
                RuleKind::Threshold,
                "dumbmonit_backup_last_status{source=\"*\"}",
            )
        },
        // Dépôts restic et Borg déclarés dans `agent.yaml`
        // (`agent/src/collect/backup_repos.rs`). Mêmes deux jours que Plakar
        // et PBS : une nuit ratée plus la marge d'une nuit. Sans dépôt déclaré,
        // aucune série, aucune alerte possible.
        Rule {
            description: "No new restic or Borg snapshot in this repository for more than 48 \
                          hours."
                .to_string(),
            operator: Operator::Gt,
            threshold: 48.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "backup_repo_too_old",
                "restic/Borg backup too old",
                RuleKind::Threshold,
                "dumbmonit_agent_backup_repo_last_snapshot_age_seconds",
            )
        },
        // Illisible couvre l'injoignable comme le mot de passe refusé : dans les
        // deux cas, plus personne ne sait si les sauvegardes existent. La raison
        // exacte est dans le journal de l'agent.
        Rule {
            description: "The agent cannot read this restic or Borg repository: unreachable, \
                          password refused or binary missing (the agent's log says which)."
                .to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(30 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "backup_repo_unreachable",
                "restic/Borg repository unreachable",
                RuleKind::Threshold,
                "dumbmonit_agent_backup_repo_reachable",
            )
        },
        // Bases de données détectées par l'agent
        // (`agent/src/collect/databases.rs`). L'agent calcule lui-même `overdue`
        // d'après l'âge maximal choisi dans `agent.yaml` : la règle ne fait que
        // lire le verdict. Sans dossier de sauvegarde connu ni déclaré, aucune
        // série `overdue`, donc aucune alerte.
        Rule {
            description: "The newest backup file of this database is older than the maximum \
                          age set for it (or no backup file was found in the watched folders)."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.5,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "database_backup_overdue",
                "Database backup overdue",
                RuleKind::Threshold,
                "dumbmonit_agent_database_backup_overdue",
            )
        },
        Rule {
            description: "A database service installed on this machine is stopped.".to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "database_service_stopped",
                "Database service stopped",
                RuleKind::Threshold,
                "dumbmonit_agent_database_up",
            )
        },
        // WireGuard : seuls les pairs à maintien de connexion
        // (`PersistentKeepalive`) sont visés. Ceux-là refont une poignée de main
        // toutes les deux minutes tant que le tunnel vit ; un téléphone ou un
        // portable sans maintien peut se taire des jours sans que rien ne soit
        // cassé. Un pair qui n'a jamais répondu compte son silence depuis que
        // l'agent le voit : un tunnel jamais monté alerte aussi.
        Rule {
            description: "A WireGuard peer with a persistent keepalive has not completed a \
                          handshake for more than 15 minutes: the tunnel is down."
                .to_string(),
            operator: Operator::Gt,
            threshold: 15.0 * 60.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "wireguard_peer_silent",
                "WireGuard peer silent",
                RuleKind::Threshold,
                "dumbmonit_agent_wireguard_peer_handshake_age_seconds \
                 and on (target, interface, peer) \
                 dumbmonit_agent_wireguard_peer_keepalive_seconds > 0",
            )
        },
        // IPsec (strongSwan) : la série n'existe que pour une CHILD_SA
        // *configurée* (`swanctl --list-conns`) — une qui n'a jamais été montée
        // alerte donc aussi, exactement comme pour OPNsense et FortiGate.
        Rule {
            description: "A configured IPsec tunnel has no installed session.".to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "ipsec_tunnel_down",
                "IPsec tunnel down",
                RuleKind::Threshold,
                "dumbmonit_agent_ipsec_tunnel_up",
            )
        },
        // Test de débit Internet (`agent/src/collect/speedtest.rs`). Les trois
        // règles ne visent que les séries jumelles des seuils : sans seuil
        // configuré, aucune ne peut se déclencher — c'est une option, pas un
        // défaut imposé à une ligne dont personne ne connaît le débit réel.
        Rule {
            description: "The last Internet speed test measured a download rate below the \
                          configured minimum."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "speedtest_download_slow",
                "Internet download rate low",
                RuleKind::Threshold,
                "dumbmonit_agent_speedtest_download_min_bytes_per_second \
                 - dumbmonit_agent_speedtest_download_bytes_per_second",
            )
        },
        Rule {
            description: "The last Internet speed test measured an upload rate below the \
                          configured minimum."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "speedtest_upload_slow",
                "Internet upload rate low",
                RuleKind::Threshold,
                "dumbmonit_agent_speedtest_upload_min_bytes_per_second \
                 - dumbmonit_agent_speedtest_upload_bytes_per_second",
            )
        },
        Rule {
            description: "The last Internet speed test measured a latency above the configured \
                          maximum."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "speedtest_latency_high",
                "Internet latency high",
                RuleKind::Threshold,
                "dumbmonit_agent_speedtest_latency_ms - dumbmonit_agent_speedtest_max_latency_ms",
            )
        },
        // ------------------------------------------------------------------
        // Journal d'événements Windows (`collectors/agent/eventlog.rs`).
        //
        // Une jauge par sévérité et par machine : le nombre d'événements reçus
        // dans les quinze dernières minutes pour les règles d'événements de
        // cette sévérité. Aucune règle d'événement n'existe par défaut, ces
        // jauges n'existent donc que là où quelqu'un en a créé. Comme pour
        // `webchange_detected`, l'alerte cite l'événement le plus récent et
        // reparle au plus toutes les dix minutes tant que d'autres arrivent ;
        // elle se résout quinze minutes après le dernier. Pas de rappel : un
        // événement est un fait, pas une panne en cours.
        // ------------------------------------------------------------------
        Rule {
            description: "A Windows event matched one of the machine's critical event log rules."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(30),
            severity: Severity::Critical,
            repeat_interval: None,
            ..base(
                "windows_event_critical",
                "Windows event (critical)",
                RuleKind::Threshold,
                "dumbmonit_agent_eventlog_matches_critical",
            )
        },
        Rule {
            description: "A Windows event matched one of the machine's warning event log rules."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(30),
            severity: Severity::Warning,
            repeat_interval: None,
            ..base(
                "windows_event_warning",
                "Windows event (warning)",
                RuleKind::Threshold,
                "dumbmonit_agent_eventlog_matches_warning",
            )
        },
        Rule {
            description: "A Windows event matched one of the machine's info event log rules."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(30),
            severity: Severity::Info,
            repeat_interval: None,
            ..base(
                "windows_event_info",
                "Windows event (info)",
                RuleKind::Threshold,
                "dumbmonit_agent_eventlog_matches_info",
            )
        },
        // ------------------------------------------------------------------
        // Matériel remonté par l'agent : disques, pools ZFS, sondes.
        //
        // Ces quatre règles ne visent que des séries que l'agent n'émet **que**
        // s'il a trouvé de quoi les remplir — `smartctl` installé, un pool ZFS,
        // une sonde qui annonce son propre seuil. Sur une machine sans rien de
        // tout cela, elles ne peuvent pas se déclencher, et n'ont donc pas à
        // être désactivées.
        // ------------------------------------------------------------------

        // Le disque a fait son autodiagnostic et se déclare en fin de vie. C'est
        // le préavis le plus précieux qu'un homelab puisse recevoir, et il
        // arrive en général des semaines avant la panne.
        Rule {
            description: "This disk's own SMART self-assessment reports it as failing. \
                          Replace it and check the backups before it goes."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "disk_smart_failed",
                "Disk SMART failing",
                RuleKind::Threshold,
                // « == bool 0 » rend une série valant 1 par disque en panne
                // annoncée et 0 pour les autres : le seuil se lit sans détour.
                "dumbmonit_agent_disk_smart_ok == bool 0",
            )
        },
        // 0 = ONLINE, 1 = DEGRADED, 2 = tout le reste. « > 0 » attrape donc le
        // pool dégradé comme le pool en faute, sans énumérer des états dont la
        // liste s'allonge à chaque version d'OpenZFS.
        Rule {
            description: "A ZFS pool is no longer healthy: a device is missing, faulted \
                          or removed."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "zfs_pool_degraded",
                "ZFS pool degraded",
                RuleKind::Threshold,
                "dumbmonit_agent_zfs_pool_health",
            )
        },
        // Un nettoyage qui trouve des erreurs a lu des données abîmées. Le pool
        // peut être ONLINE et le rester : c'est justement le silence que cette
        // règle vient rompre.
        Rule {
            description: "The last ZFS scrub found errors on this pool.".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "zfs_scrub_errors",
                "ZFS scrub found errors",
                RuleKind::Threshold,
                // Les fichiers définitivement perdus comptent autant que les
                // erreurs relevées par le nettoyage lui-même.
                "dumbmonit_agent_zfs_pool_scrub_errors \
                 or dumbmonit_agent_zfs_pool_data_errors",
            )
        },
        // Température au-dessus du seuil critique **que la sonde annonce
        // elle-même**. Pas de valeur en dur : 85 °C est une alerte sur un disque
        // et une journée ordinaire pour un processeur de portable.
        Rule {
            description: "A sensor is above the critical temperature its own hardware \
                          declares."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Critical,
            unit: "°C".to_string(),
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "sensor_temperature_critical",
                "Temperature above critical",
                RuleKind::Threshold,
                // Les deux séries portent exactement les mêmes étiquettes : la
                // comparaison les apparie d'elle-même, et rend la température,
                // qui est ce qu'on veut lire dans l'alerte.
                "dumbmonit_agent_sensor_temperature_celsius \
                 >= dumbmonit_agent_sensor_temperature_critical_celsius",
            )
        },
        // Active Backup for Business (`collectors/synology/abb.rs`). `last_status`
        // vaut 1 réussite, 0 échec, 2 en cours, -1 inconnu : « == bool 0 » rend une
        // série valant 1 pour chaque tâche en échec et 0 pour les autres, ce qu'un
        // seuil « > 0 » lit sans ambiguïté — un « < 1 » attraperait aussi l'inconnu.
        Rule {
            description: "The last run of this Active Backup for Business task failed, \
                          or backed up only part of its devices."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "synology_abb_task_failed",
                "Active Backup task failed",
                RuleKind::Threshold,
                "dumbmonit_abb_task_last_status == bool 0",
            )
        },
        // Mêmes seuils que les autres sauvegardes : deux jours, c'est une nuit ratée
        // plus la marge d'une nuit. La série n'existe pas pour une tâche qui n'a
        // jamais réussi ; c'est alors `synology_abb_task_failed` qui parle.
        Rule {
            description: "No successful Active Backup for Business run for this task for more \
                          than two days."
                .to_string(),
            operator: Operator::Gt,
            threshold: 2.0 * 24.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "synology_abb_backup_too_old",
                "Active Backup too old",
                RuleKind::Threshold,
                "dumbmonit_abb_task_last_success_seconds",
            )
        },
        // Information : une tâche sans planning ne sauvegardera plus rien tant que
        // personne ne la lance à la main. Une heure de `for` absorbe une
        // reprogrammation en cours.
        Rule {
            description: "This Active Backup for Business task has no schedule, or its \
                          continuous backup is paused."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Info,
            repeat_interval: Some(Duration::from_secs(7 * 24 * 3600)),
            ..base(
                "synology_abb_task_disabled",
                "Active Backup task disabled",
                RuleKind::Threshold,
                "dumbmonit_abb_task_enabled == bool 0",
            )
        },
    ]
}
