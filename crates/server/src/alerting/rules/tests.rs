use super::*;

#[test]
fn les_regles_livrees_couvrent_les_cas_annonces() {
    let rules = builtin_rules();
    let uids: Vec<&str> = rules.iter().map(|r| r.uid.as_str()).collect();
    for attendu in [
        RULE_HOST_DOWN,
        "cpu_high",
        "disk_almost_full",
        "ups_on_battery",
        "ups_battery_low",
        // Ports réseau (IF-MIB) et matériel serveur (Redfish).
        "port_errors",
        "port_flapping",
        "redfish_fan_failed",
        "redfish_temperature_critical",
        "redfish_psu_redundancy_lost",
        "redfish_psu_failed",
        "redfish_drive_failure_predicted",
        "redfish_system_critical",
        "fs_will_be_full",
        "backup_too_old",
        "pbs_datastore_almost_full",
        "pbs_datastore_will_be_full",
        "pbs_backup_too_old",
        "pbs_backup_verification_failed",
        "pbs_task_failed",
        "pbs_gc_too_old",
        "service_down",
        "push_missed",
        "push_failed",
        "push_too_long",
        "script_warning",
        "service_flapping",
        "service_slow",
        "tls_cert_expiring",
        "tls_cert_expired",
        "container_stopped",
        "container_unhealthy",
        "container_restarting",
        "container_update_available",
        "plakar_backup_too_old",
        "plakar_backup_failed",
        "plakar_kloset_unreadable",
        // Matériel vu par l'agent (`crates/agent/src/collect/{smart,zfs,sensors}.rs`).
        "disk_smart_failed",
        "zfs_pool_degraded",
        "zfs_scrub_errors",
        "sensor_temperature_critical",
        "synology_abb_task_failed",
        "synology_abb_backup_too_old",
        "synology_abb_task_disabled",
        "pve_guest_stopped",
        "pve_ha_resource_error",
        "pve_cluster_no_quorum",
        "pve_node_offline",
        "pve_storage_almost_full",
        "pve_backup_job_failed",
        "pve_snapshot_old",
        "pve_replication_failed",
        "pve_ceph_health_error",
        "pve_ceph_health_warning",
        "pve_updates_pending",
        "pve_certificate_expiring",
        "pbs_sync_failed",
        "pbs_updates_pending",
        "pbs_prune_failed",
        "pbs_verify_job_failed",
        "pbs_gc_failed",
        "pbs_disk_smart_failed",
        "pbs_disk_wearout",
        "pbs_zpool_degraded",
        // Proxmox Datacenter Manager (`collectors/pdm`).
        "pdm_remote_unreachable",
        "pdm_remote_version_behind",
        "pdm_task_failed",
        "pdm_node_disk_almost_full",
        "pdm_certificate_expiring",
        "pdm_updates_pending",
        "pbs_service_down",
        "pbs_certificate_expiring",
        "pbs_version_stale",
        "pbs_tape_job_failed",
        "pbs_job_never_run",
        "pbs_gc_bad_chunks",
        "pbs_datastore_unmounted",
        "pve_guest_cpu_high",
        "pve_guest_memory_high",
        "pve_guest_disk_almost_full",
        "pve_disk_wearout",
        "pve_disk_smart_failed",
        // Synology DSM (`collectors/synology`).
        "synology_disk_smart_warning",
        "synology_disk_failed",
        "synology_disk_bad_sectors",
        "synology_disk_bad_sectors_growing",
        "synology_ssd_wearout",
        "synology_volume_degraded",
        "synology_volume_almost_full",
        "synology_thin_overcommit",
        "synology_lun_almost_full",
        "synology_temperature_high",
        "synology_memory_high",
        "synology_abb_device_overdue",
        "synology_abb_device_failing",
        "pve_zfs_pool_degraded",
        "pve_security_updates_pending",
        "pve_packages_changed",
        // Proxmox Mail Gateway (`collectors/pmg`).
        "pmg_queue_growing",
        "pmg_queue_stuck",
        "pmg_filter_service_down",
        "pmg_virus_signatures_stale",
        "pmg_spam_rules_stale",
        "pmg_quarantine_growing",
        "pmg_cluster_degraded",
        "pmg_certificate_expiring",
        "pmg_updates_pending",
        // MDaemon et SecurityGateway (`collectors/mdaemon`).
        "mdaemon_service_down",
        "mdaemon_api_down",
        "mdaemon_queue_growing",
        "mdaemon_bad_queue_not_empty",
        "mdaemon_retry_queue_high",
        "mail_server_service_stopped",
        "mail_queue_stuck",
        "mdaemon_queue_frozen",
        "securitygateway_service_down",
        "securitygateway_api_down",
        "securitygateway_queue_growing",
        // OPNsense (`collectors/opnsense`).
        "opnsense_gateway_down",
        "opnsense_gateway_loss",
        "opnsense_gateway_latency",
        "opnsense_state_table_filling",
        "opnsense_mbuf_exhausted",
        "opnsense_vpn_tunnel_down",
        "opnsense_resolver_down",
        "opnsense_core_service_down",
        "opnsense_temperature_high",
        "opnsense_carp_maintenance",
        "opnsense_reboot_pending",
        "opnsense_updates_pending",
        // TrueNAS (`collectors/truenas`).
        "truenas_pool_degraded",
        "truenas_pool_device_errors",
        "truenas_pool_almost_full",
        "truenas_scrub_errors",
        "truenas_scrub_overdue",
        "truenas_disk_smart_failed",
        "truenas_disk_hot",
        "truenas_dataset_quota_full",
        "truenas_replication_failed",
        "truenas_snapshot_task_failed",
        "truenas_snapshots_stale",
        "truenas_alert_raised",
        "truenas_service_down",
        // Serveurs de journaux et de métriques (`collectors/observability`).
        "victoria_read_only",
        "victoria_disk_headroom_low",
        "victoria_unhealthy",
        "victoria_rows_rejected",
        "victoriametrics_slow_inserts",
        "loki_not_ready",
        "loki_lines_discarded",
        "loki_flush_failing",
        "loki_wal_disk_full",
        "loki_request_errors",
        "graylog_not_processing",
        "graylog_search_cluster_down",
        "graylog_search_cluster_yellow",
        "graylog_journal_filling",
        "graylog_input_failed",
        "graylog_indexing_failures",
        // Applications auto-hébergées (`collectors/selfhosted`).
        "nextcloud_maintenance",
        "nextcloud_upgrade_pending",
        "nextcloud_update_available",
        "nextcloud_opcache_full",
        "nextcloud_free_space_low",
        "immich_jobs_stalled",
        "immich_queue_paused",
        "immich_storage_almost_full",
        "immich_update_available",
        // Applications (`collectors/selfhosted/{arr,qbittorrent,protect,frigate,
        // keycloak,authentik,vaultwarden,mailcow}.rs`).
        "sonarr_health_error",
        "sonarr_queue_errors",
        "sonarr_disk_almost_full",
        "sonarr_update_available",
        "radarr_health_error",
        "radarr_queue_errors",
        "radarr_disk_almost_full",
        "radarr_update_available",
        "lidarr_health_error",
        "lidarr_queue_errors",
        "lidarr_disk_almost_full",
        "lidarr_update_available",
        "prowlarr_health_error",
        "prowlarr_update_available",
        "prowlarr_indexers_disabled",
        "qbittorrent_torrents_errored",
        "qbittorrent_disconnected",
        "qbittorrent_free_space_low",
        "frigate_camera_offline",
        "frigate_detector_stuck",
        "frigate_storage_almost_full",
        "frigate_update_available",
        "keycloak_not_ready",
        "keycloak_realm_disabled",
        "authentik_not_ready",
        "authentik_no_worker",
        "authentik_tasks_failed",
        "authentik_outpost_disconnected",
        "authentik_update_available",
        "vaultwarden_signups_open",
        "mailcow_container_down",
        "mailcow_queue_stuck",
        "mailcow_vmail_almost_full",
        "mailcow_mailbox_near_quota",
        // Appareils clients, générique (`collectors/client_devices.rs`).
        "client_device_stale",
        "paperless_broker_down",
        "paperless_component_error",
        "paperless_migrations_pending",
        "paperless_task_failed",
        "paperless_storage_almost_full",
        "paperless_update_available",
        "jellyfin_task_failed",
        "jellyfin_plugin_broken",
        "jellyfin_restart_pending",
        "plex_update_available",
        "immich_jobs_failed",
        "immich_ml_failing",
        "immich_maintenance_mode",
        "immich_user_quota_almost_full",
        "immich_db_backup_stale",
        "jellyfin_library_scan_stale",
        "jellyfin_storage_almost_full",
        "jellyfin_cpu_transcoding_heavy",
        "jellyfin_failed_logins",
        "plex_transcoder_too_slow",
        "plex_transcode_error",
        "plex_remote_access_failed",
        "plex_signin_lost",
        "plex_database_backup_off",
        "gitlab_runners_all_offline",
        "gitlab_sidekiq_backlog_high",
        "gitlab_migrations_pending",
        "gitlab_license_expiring",
        "forgejo_runners_all_offline",
        "forgejo_cron_task_overdue",
        // Pi-hole (`collectors/pihole`).
        "pihole_blocking_disabled",
        "pihole_gravity_stale",
        "pihole_update_available",
        "pihole_diagnosis_messages",
        // AdGuard Home (`collectors/adguard`).
        "adguard_protection_disabled",
        "adguard_dns_not_running",
        "adguard_upstream_down",
        "adguard_filters_stale",
        "adguard_update_available",
        // Onduleurs derrière NUT (`collectors/nut`).
        "nut_on_battery",
        "nut_battery_low",
        "nut_replace_battery",
        "nut_load_high",
        "nut_runtime_short",
        "nut_data_stale",
        // MikroTik RouterOS (`collectors/mikrotik`).
        "mikrotik_cpu_high",
        "mikrotik_memory_high",
        "mikrotik_storage_almost_full",
        "mikrotik_temperature_high",
        "mikrotik_hardware_failed",
        "mikrotik_interface_errors",
        "mikrotik_link_flapping",
        "mikrotik_update_available",
        "mikrotik_firmware_upgrade_pending",
        // UniFi, Home Assistant, vSphere (`collectors/{unifi,homeassistant,vsphere}`).
        "unifi_device_offline",
        "unifi_internet_down",
        "unifi_wan_link_down",
        "unifi_gateway_cpu_high",
        "unifi_device_pending_adoption",
        "unifi_firmware_update",
        "unifi_alarms",
        "unifi_subsystem_error",
        "unifi_device_overheating",
        "unifi_poe_budget_high",
        "unifi_radio_channel_busy",
        "unifi_wifi_weak_signal",
        "unifi_port_errors",
        "opnsense_certificate_expiring",
        "opnsense_certificate_expired",
        "opnsense_disk_almost_full",
        "opnsense_cpu_high",
        "opnsense_swap_high",
        "opnsense_pf_disabled",
        "opnsense_interface_errors",
        "opnsense_firmware_mirror_unreachable",
        "tailscale_key_expired",
        "tailscale_router_offline",
        "tailscale_routes_unapproved",
        "tailscale_tailnet_lock_error",
        "homeassistant_recovery_mode",
        "homeassistant_not_running",
        "homeassistant_entities_unavailable",
        "homeassistant_battery_low",
        "homeassistant_update_available",
        "homeassistant_repair",
        "vsphere_host_not_responding",
        "vsphere_host_red",
        "vsphere_host_maintenance",
        "vsphere_datastore_almost_full",
        "vsphere_datastore_inaccessible",
        "vsphere_vm_tools_not_running",
        "vsphere_alarm_red",
        "vsphere_vm_snapshot_old",
        "vsphere_vm_consolidation_needed",
        // Redis, MongoDB, RabbitMQ, CrowdSec.
        "redis_memory_near_limit",
        "redis_rejecting_connections",
        "redis_replication_broken",
        "redis_replica_lagging",
        "redis_save_failed",
        "redis_aof_write_failed",
        "mongodb_no_primary",
        "mongodb_member_unhealthy",
        "mongodb_replication_lag",
        "mongodb_connections_high",
        "mongodb_cache_dirty",
        "mongodb_assertions",
        "rabbitmq_resource_alarm",
        "rabbitmq_node_down",
        "rabbitmq_partition",
        "rabbitmq_queue_no_consumer",
        "rabbitmq_queue_not_running",
        // SQL Server, Oracle, Elasticsearch, InfluxDB, Kafka.
        "mssql_database_unavailable",
        "mssql_blocking",
        "mssql_volume_almost_full",
        "mssql_agent_job_failed",
        "mssql_alwayson_unhealthy",
        "mssql_sessions_high",
        "oracle_instance_not_open",
        "oracle_tablespace_almost_full",
        "oracle_sessions_near_limit",
        "oracle_dataguard_lag",
        "oracle_recovery_area_full",
        "oracle_scheduler_job_failed",
        "elasticsearch_cluster_red",
        "elasticsearch_cluster_yellow",
        "elasticsearch_unassigned_shards",
        "elasticsearch_disk_almost_full",
        "elasticsearch_heap_high",
        "influxdb_unhealthy",
        "influxdb_not_ready",
        "influxdb_request_errors",
        "kafka_offline_partitions",
        "kafka_under_replicated",
        "kafka_no_controller",
        "kafka_consumer_lag",
        "crowdsec_lapi_down",
        "crowdsec_bouncer_stale",
        "crowdsec_no_logs_read",
        "traefik_router_disabled",
        "traefik_server_down",
        "traefik_service_down",
        "traefik_certificate_missing",
        "caddy_upstream_unhealthy",
        "caddy_reload_failed",
        "npm_host_offline",
        "proxy_cert_expiring",
        "proxy_5xx_rate",
        "domain_expiring",
        "domain_expiring_soon",
        "domain_on_hold",
        "domain_redemption",
        "domain_nameservers_changed",
        "domain_registrar_changed",
        "domain_dnssec_lost",
        "dns_answer_changed",
        "webchange_detected",
        "netconfig_changed",
        "netconfig_host_key_changed",
        "website_down",
        "website_blank",
        "website_expected_text_missing",
        "website_noindex",
        "website_robots_blocks_all",
        "website_canonical_changed",
        "website_final_url_changed",
        "website_metadata_changed",
        "website_text_dropped",
        "website_google_verification_changed",
        "website_seo_markup_changed",
        "website_slow",
        "k8s_node_not_ready",
        "k8s_node_pressure",
        "k8s_pod_crashlooping",
        "k8s_pod_pending",
        "k8s_pod_restarting",
        "k8s_workload_unavailable",
        "k8s_pvc_pending",
        "backup_repo_too_old",
        "backup_repo_unreachable",
        "database_backup_overdue",
        "database_service_stopped",
        "wireguard_peer_silent",
        "ipsec_tunnel_down",
        "speedtest_download_slow",
        "speedtest_upload_slow",
        "speedtest_latency_high",
        "windows_event_critical",
        "windows_event_warning",
        "windows_event_info",
        "pfsense_gateway_down",
        "pfsense_gateway_loss",
        "pfsense_gateway_latency",
        "pfsense_interface_down",
        "pfsense_service_stopped",
        "pfsense_disk_full",
        "pfsense_states_table_full",
        "pfsense_mbuf_high",
        "pfsense_temperature_high",
        "pfsense_cpu_high",
        "pfsense_swap_high",
        "pfsense_interface_errors",
        "pfsense_carp_maintenance",
        "unraid_array_stopped",
        "unraid_disk_problem",
        "unraid_disk_errors",
        "unraid_disk_hot",
        "unraid_parity_errors",
        "unraid_parity_check_old",
        "unraid_cache_full",
        "unraid_container_stopped",
        "unraid_vm_crashed",
        "unraid_alert_unread",
        "unraid_array_almost_full",
        "unraid_memory_high",
        "unraid_license_problem",
        "unraid_trial_expiring",
        "unraid_os_updates_ending",
        "veeam_job_failed",
        "veeam_job_warning",
        "veeam_job_old",
        "veeam_repository_full",
        "veeam_license_expiring",
        "veeam_support_expiring",
        "veeam_repository_offline",
        "veeam_license_invalid",
        "veeam_config_backup_disabled",
        "veeam_config_backup_old",
        "winrm_services_auto_stopped",
        "tailscale_device_offline",
        "tailscale_key_expiring",
        "tailscale_device_unapproved",
        "tailscale_update_available",
        "fortigate_ipsec_tunnel_down",
        "fortigate_interface_down",
        "fortigate_ha_out_of_sync",
        "fortigate_ha_member_lost",
        "fortigate_memory_high",
        "fortigate_cpu_high",
        "fortigate_license_expiring",
        "fortigate_firmware_update",
        "sophos_interface_down",
        "omada_device_offline",
        "omada_cpu_high",
        "omada_memory_high",
        "omada_firmware_update",
        "qnap_volume_full",
        "qnap_disk_unhealthy",
        "qnap_disk_hot",
        "qnap_cpu_hot",
        "qnap_firmware_update",
        "ceph_health_error",
        "ceph_health_warn",
        "ceph_osd_down",
        "ceph_pg_unclean",
        "ceph_objects_unfound",
        "ceph_capacity_high",
        "ceph_osd_nearly_full",
        "ceph_pool_nearly_full",
        "ceph_mon_out_of_quorum",
        "minio_node_offline",
        "minio_drive_offline",
        "minio_capacity_high",
        "minio_heal_failures",
        "minio_replication_failed",
        "minio_s3_server_errors",
        "kopia_snapshot_errors",
        "kopia_snapshot_old",
        "kopia_repo_disconnected",
        "duplicati_backup_failed",
        "duplicati_backup_old",
        "libvirt_domain_crashed",
        "libvirt_pool_almost_full",
        "dsm_raid_degraded",
        "dsm_volume_read_only",
        "dsm_disk_hot",
        "dsm_disk_bad_sectors_growing",
        "dsm_ssd_wearout",
        "dsm_package_down",
        "xcpng_host_offline",
        "xcpng_sr_almost_full",
        "xcpng_vm_ha_protected_down",
        "xcpng_pool_ha_overcommitted",
        "nutanix_host_degraded",
        "nutanix_storage_container_almost_full",
        "nutanix_alerts_critical",
        "hyperv_vm_critical",
        "hyperv_host_cpu_high",
        "hyperv_vhd_errors",
        // Profondeur Proxmox VE, PBS et TrueNAS (`rules/{pve,pbs,truenas}.rs`).
        "pve_storage_inactive",
        "pve_reboot_required",
        "pve_subscription_expired",
        "pve_replication_overdue",
        "pve_backup_job_stale",
        "pve_guest_not_backed_up",
        "pve_tasks_failed",
        "pbs_datastore_unavailable",
        "pbs_datastore_maintenance",
        "pbs_rootfs_almost_full",
        "pbs_job_stale",
        "pbs_subscription_expired",
        "truenas_certificate_expiring",
        "truenas_certificate_expired",
        "truenas_boot_pool_degraded",
        "truenas_boot_pool_almost_full",
        "truenas_backup_task_failed",
        "truenas_app_crashed",
        "truenas_vm_down",
        // Sauvegarde locale de l'instance (`backup/local.rs`).
        "instance_backup_missing",
        // Auto-surveillance de l'instance (`selfmon.rs`).
        "instance_data_volume_low",
        "instance_vm_restarting",
        "instance_scheduler_stalled",
        "instance_scheduler_behind",
        "instance_write_errors",
        "instance_database_growth",
        "instance_api_token_expiring",
        // Note de sécurité (`security/`).
        "security_score_drop",
        "security_score_low",
        // Failles connues (`vulns/`).
        "cve_exploited_vulnerable",
        "cve_critical_vulnerable",
    ] {
        assert!(uids.contains(&attendu), "missing built-in rule: {attendu}");
    }
}

/// Une règle qui vise une métrique inexistante ne déclenche jamais et ne se
/// signale pas : elle donne l'illusion d'une surveillance en place. Ce test fige
/// donc la correspondance avec les noms réellement produits par les collecteurs.
#[test]
fn les_regles_livrees_ne_visent_que_des_metriques_produites() {
    for rule in builtin_rules() {
        for mot in rule.query.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if mot.starts_with("dumbmonit_") {
                assert!(
                    PRODUITES.contains(&mot)
                        || cloud::produced().contains(&mot)
                        || pve::PRODUCED.contains(&mot)
                        || pbs::PRODUCED.contains(&mot)
                        || truenas::PRODUCED.contains(&mot)
                        || unifi::PRODUCED.contains(&mot)
                        || pfsense::PRODUCED.contains(&mot)
                        || unraid::PRODUCED.contains(&mot)
                        || veeam::PRODUCED.contains(&mot)
                        || vsphere::PRODUCED.contains(&mot)
                        || opnsense::PRODUCED.contains(&mot)
                        || tailscale::PRODUCED.contains(&mot)
                        || pihole::PRODUCED.contains(&mot)
                        || adguard::PRODUCED.contains(&mot)
                        || homeassistant::PRODUCED.contains(&mot)
                        || immich::PRODUCED.contains(&mot)
                        || jellyfin::PRODUCED.contains(&mot)
                        || plex::PRODUCED.contains(&mot)
                        || nextcloud::PRODUCED.contains(&mot)
                        || hyperv::PRODUCED.contains(&mot)
                        || mysql::PRODUCED.contains(&mot)
                        || postgres::PRODUCED.contains(&mot)
                        || synology::PRODUCED.contains(&mot),
                    "rule \"{}\" targets \"{mot}\", which no collector produces",
                    rule.uid
                );
            }
        }
    }
}

#[test]
fn la_regle_d_expiration_du_support_veeam() {
    let regles = builtin_rules();
    let regle = regles.iter().find(|r| r.uid == "veeam_support_expiring").unwrap();
    assert!(regle.query.contains("dumbmonit_veeam_support_expiry_seconds"), "{}", regle.query);
    assert_eq!(regle.operator, Operator::Lt);
    assert_eq!(regle.threshold, 30.0 * 24.0 * 3600.0);
    assert!(PRODUITES.contains(&"dumbmonit_veeam_support_expiry_seconds"));
}

#[test]
fn les_identifiants_des_regles_livrees_sont_uniques() {
    let rules = builtin_rules();
    let mut uids: Vec<&str> = rules.iter().map(|r| r.uid.as_str()).collect();
    uids.sort_unstable();
    let total = uids.len();
    uids.dedup();
    assert_eq!(uids.len(), total, "two built-in rules share a uid");
}

#[test]
fn les_regles_livrees_sont_actives_et_sans_canal_impose() {
    for rule in builtin_rules() {
        // Les règles d'inventaire du réseau sont les seules livrées éteintes.
        let off = rule.uid == RULE_NETSCAN_NEW_DEVICE
            || rule.uid == RULE_NETSCAN_NEW_PORT
            || rule.uid == RULE_NETSCAN_MAC_CHANGE;
        assert_eq!(rule.enabled, !off, "{} enabled flag", rule.uid);
        assert!(rule.builtin);
        assert!(rule.channels.is_empty(), "{} must not impose any channel", rule.uid);
        assert!(matches!(rule.selector, TargetSelector::All));
    }
}

#[test]
fn seule_la_regle_injoignable_porte_l_uid_reserve() {
    let host_down: Vec<_> = builtin_rules().into_iter().filter(|r| r.is_host_down()).collect();
    assert_eq!(host_down.len(), 1);
    assert_eq!(host_down[0].severity, Severity::Critical);
}

/// Une sonde en échec dure son délai : la lenteur ne se juge que sur les
/// sondes réussies.
#[test]
fn la_lenteur_ne_juge_que_les_sondes_reussies() {
    let rule = builtin_rules().into_iter().find(|r| r.uid == "service_slow").unwrap();
    assert_eq!(rule.query, SERVICE_SLOW_QUERY);
    assert!(rule.query.contains("and dumbmonit_probe_success == 1"));
}

/// Après un redémarrage de VictoriaMetrics, la fenêtre mêle compteurs d'avant
/// et d'après : la règle attend que le serveur tourne depuis plus que sa
/// fenêtre.
#[test]
fn les_insertions_lentes_attendent_un_serveur_installe() {
    let rule =
        builtin_rules().into_iter().find(|r| r.uid == "victoriametrics_slow_inserts").unwrap();
    assert_eq!(rule.query, VM_SLOW_INSERTS_QUERY);
    assert!(rule.query.contains("[15m]"));
    assert!(rule.query.ends_with("dumbmonit_victoriametrics_uptime_seconds > 900"));
}

/// Le redémarrage d'un nœud ajoute un redémarrage par conteneur : la boucle
/// exige un compteur qui progresse encore.
#[test]
fn la_boucle_de_redemarrage_exige_des_redemarrages_recents() {
    let rule = builtin_rules().into_iter().find(|r| r.uid == "k8s_pod_crashlooping").unwrap();
    assert_eq!(rule.query, K8S_CRASHLOOP_QUERY);
    assert!(rule.query.contains("increase_prometheus(dumbmonit_k8s_pod_restarts[15m]) >= 2"));
}

/// Les règles de fraîcheur jugent une cible, pas chacune de ses variantes :
/// la fenêtre de sept jours garde des séries que plus personne n'écrit, et
/// sans réduction par cible leur âge croissant fait sonner une cible vivante.
#[test]
fn la_regle_injoignable_ne_garde_qu_une_serie_par_cible() {
    let rule = builtin_rules().into_iter().find(|r| r.is_host_down()).expect("host_down");
    assert_eq!(rule.query, HOST_DOWN_QUERY);
    assert!(rule.query.starts_with("bottomk by (target) (1, "), "{}", rule.query);
    assert!(rule.query.contains("tlast_over_time(dumbmonit_up[7d])"));
}

/// Une règle d'absence de progrès (`changes`, `increase` sous un seuil) sur
/// une longue fenêtre doit se restreindre aux séries encore écrites : une
/// variante figée n'y progresse plus par définition.
#[test]
fn les_regles_d_absence_de_progres_ignorent_les_series_figees() {
    for rule in builtin_rules() {
        let absence = matches!(rule.operator, Operator::Lt | Operator::Le)
            && ["changes", "increase", "delta"].iter().any(|f| rule.query.contains(f))
            && rule.query.contains('[');
        if absence {
            assert!(rule.query.contains(" and "), "{} also fires on stale series", rule.uid);
        }
    }
}

#[test]
fn la_requete_predictive_reste_du_metricsql() {
    let query = predict_full_query("dumbmonit_fs_used_percent", 6, 4);
    assert_eq!(
        query,
        "predict_linear(dumbmonit_fs_used_percent[6h], 345600) \
         and deriv(dumbmonit_fs_used_percent[6h]) > 0"
    );
}

#[test]
fn les_regles_derivees_sont_livrees_avec_des_reglages_senses() {
    let rules = builtin_rules();
    let forecast = rules.iter().find(|r| r.uid == RULE_CAPACITY_FORECAST).unwrap();
    assert_eq!(forecast.kind, RuleKind::Forecast);
    assert_eq!(forecast.operator, Operator::Lt);
    assert_eq!(forecast.threshold, 30.0);
    assert_eq!(forecast.params.forecast_warning_days, 14.0);
    assert_eq!(forecast.params.forecast_critical_days, 7.0);
    assert!(matches!(
        crate::alerting::capacity::select(&forecast.query),
        crate::alerting::capacity::Selection::Families(ref all) if all.len() > 20
    ));

    let burn = rules.iter().find(|r| r.uid == RULE_SLO_BURN).unwrap();
    assert_eq!(burn.kind, RuleKind::SloBurn);
    assert_eq!((burn.operator, burn.threshold), (Operator::Ge, 1.0), "slow burn opens it");
    assert_eq!(burn.severity, Severity::Warning, "slow burn: an advisory on screen");

    for uid in ["latency_drift", "http_5xx_drift"] {
        let drift = rules.iter().find(|r| r.uid == uid).unwrap();
        assert_eq!(drift.kind, RuleKind::Drift);
        assert_eq!(drift.operator, Operator::Ge);
        assert!(drift.clear_threshold.is_some_and(|clear| clear < drift.threshold));
        assert!(drift.params.drift_min_value > 0.0, "{uid}: tiny values must not alert");
        assert!(drift.for_duration >= Duration::from_secs(600), "{uid}: minimal duration");
    }
}

#[test]
fn toutes_les_regles_livrees_ont_un_for_non_nul_sauf_justification() {
    for rule in builtin_rules() {
        assert!(
            rule.for_duration > Duration::ZERO,
            "{} would fire on the first point, with no noise filter",
            rule.uid
        );
    }
}
