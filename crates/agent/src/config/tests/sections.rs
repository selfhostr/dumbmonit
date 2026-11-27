//! Tests de la configuration (sections).

use super::*;

#[test]
fn the_hyperv_preset_is_automatic_by_default_and_can_be_forced() {
    let config = Config::merge(file_with_url_and_token(), env(&[])).expect("configuration");
    assert_eq!(config.perf_counters.hyperv, PresetMode::Auto);
    let config = Config::merge(from_yaml("hyperv: false\n"), env(&[])).expect("configuration");
    assert_eq!(config.perf_counters.hyperv, PresetMode::Off);
    let config =
        Config::merge(from_yaml("hyperv: false\n"), env(&[("DUMBMONIT_AGENT_HYPERV", "yes")]))
            .expect("configuration");
    assert_eq!(config.perf_counters.hyperv, PresetMode::On);
    assert_eq!(config.perf_counters.mdaemon, PresetMode::Auto, "independent presets");
    assert!(Config::merge(from_yaml("hyperv: maybe\n"), env(&[])).is_err());
}

#[test]
fn a_malformed_counter_path_or_mdaemon_value_is_refused_at_startup() {
    let error = Config::merge(from_yaml("perf_counters:\n  - 'Memory\\Available'\n"), env(&[]))
        .unwrap_err();
    assert!(format!("{error:#}").contains("perf_counters"), "{error:#}");
    assert!(Config::merge(from_yaml("mdaemon: sometimes\n"), env(&[])).is_err());
    assert!(
        Config::merge(file_with_url_and_token(), env(&[("DUMBMONIT_AGENT_MDAEMON", "2")])).is_err()
    );
}

#[test]
fn restic_and_borg_repositories_come_from_the_file() {
    let yaml = r#"
server_url: https://serveur:8080
token: dmon_abc
wireguard_peer_names:
  "zonmO992nrIL4FgJ3B/wccmu/SBkoX2kDhSYr8KpgRU=": laptop
restic_repos:
  - name: nas
    repository: sftp:backup@192.0.2.10:/srv/restic
    password_file: /etc/dumbmonit/restic-nas.pass
    host: web-01
  - repository: "rest:https://user:s3cret@backup.example.net/repo"
    password_command: pass show restic
    env:
      RESTIC_REST_USERNAME: user
borg_repos:
  - name: offsite
    repository: ssh://borg@192.0.2.20/./repo
    password_file: /etc/dumbmonit/borg.pass
    glob_archives: "web-01-*"
backup_repos_interval_secs: 900
"#;
    let file: FileConfig = dumbmonit_collectors::yaml::from_str(yaml).expect("yaml");
    let config = Config::merge(file, env(&[("DUMBMONIT_AGENT_BORG_BIN", "/opt/borg")]))
        .expect("configuration");
    let repos = &config.backup_repos.repos;
    assert_eq!(repos.len(), 3);
    assert_eq!((repos[0].tool, repos[0].name.as_str()), (Tool::Restic, "nas"));
    assert_eq!(repos[0].host.as_deref(), Some("web-01"));
    assert_eq!(
        repos[1].name, "rest:https://backup.example.net/repo",
        "the default name never carries the password of the URL"
    );
    assert_eq!(repos[1].env["RESTIC_REST_USERNAME"], "user");
    assert_eq!((repos[2].tool, repos[2].name.as_str()), (Tool::Borg, "offsite"));
    assert_eq!(config.backup_repos.borg_bin, "/opt/borg");
    assert_eq!(config.backup_repos.interval, Duration::from_secs(900));
    assert_eq!(config.wireguard.peer_names.len(), 1);
    assert!(config.wireguard.enabled);
    let shown = format!("{config:?}");
    assert!(!shown.contains("pass show restic"), "commands stay out of the logs");
}

#[test]
fn a_repository_entry_must_be_complete_and_unique() {
    let missing: FileConfig = dumbmonit_collectors::yaml::from_str(
        "server_url: https://s\ntoken: t\nrestic_repos:\n  - name: nas\n",
    )
    .expect("yaml");
    assert!(Config::merge(missing, env(&[])).is_err());
    let twice: FileConfig = dumbmonit_collectors::yaml::from_str(
        "server_url: https://s\ntoken: t\nborg_repos:\n  - {name: a, repository: /r1}\n  - {name: a, repository: /r2}\n",
    )
    .expect("yaml");
    assert!(Config::merge(twice, env(&[])).is_err());
    let too_fast: FileConfig = dumbmonit_collectors::yaml::from_str(
        "server_url: https://s\ntoken: t\nbackup_repos_interval_secs: 5\n",
    )
    .expect("yaml");
    assert!(Config::merge(too_fast, env(&[])).is_err());
    let off =
        Config::merge(file_with_url_and_token(), env(&[("DUMBMONIT_AGENT_WIREGUARD", "false")]))
            .expect("configuration");
    assert!(!off.wireguard.enabled);
}

#[test]
fn credentials_are_stripped_from_repository_urls() {
    assert_eq!(
        without_credentials("s3:https://s3.example.net/bucket"),
        "s3:https://s3.example.net/bucket"
    );
    assert_eq!(without_credentials("rest:https://u:p@h:8000/r"), "rest:https://h:8000/r");
    assert_eq!(without_credentials("ssh://borg@host/./repo"), "ssh://host/./repo");
    assert_eq!(without_credentials("/srv/restic"), "/srv/restic");
    assert_eq!(without_credentials("sftp:user@host:/srv/r"), "sftp:user@host:/srv/r");
}

#[test]
fn without_a_module_list_everything_is_auto_detected() {
    let config = Config::merge(file_with_url_and_token(), env(&[])).expect("configuration");
    assert_eq!(config.modules, None);
    assert!(config.docker && config.plakar.enabled && config.sensors);
    assert!(config.smart.enabled && config.zfs.enabled && config.wireguard.enabled);
    assert!(config.libvirt.enabled);
    assert!(config.system_health.enabled);
    assert_eq!(config.perf_counters.mdaemon, PresetMode::Auto);
}

#[test]
fn a_module_list_keeps_only_the_modules_it_names() {
    let config = Config::merge(with_yaml("modules: [docker, plakar]\n"), env(&[])).expect("config");
    assert!(config.docker);
    assert!(config.plakar.enabled);
    assert!(!config.sensors, "non demandé : pas de sondes");
    assert!(!config.smart.enabled);
    assert!(!config.zfs.enabled);
    assert!(!config.libvirt.enabled);
    assert!(!config.wireguard.enabled);
    assert!(!config.system_health.enabled, "ni mises à jour, ni unités en échec");
    assert_eq!(config.perf_counters.mdaemon, PresetMode::Off);
    assert_eq!(config.perf_counters.hyperv, PresetMode::Off);
    assert_eq!(
        config.modules.as_ref().map(|m| m.iter().cloned().collect::<Vec<_>>()),
        Some(vec!["docker".to_string(), "plakar".to_string()])
    );
}

#[test]
fn the_module_list_reads_a_string_the_environment_and_none() {
    let config = Config::merge(with_yaml("modules: \"smart, System-Health\"\n"), env(&[]))
        .expect("configuration");
    assert!(config.smart.enabled && config.system_health.enabled);
    assert!(!config.docker);

    // L'environnement remplace la liste du fichier.
    let config = Config::merge(
        with_yaml("modules: [smart]\n"),
        env(&[("DUMBMONIT_AGENT_MODULES", "docker")]),
    )
    .expect("configuration");
    assert!(config.docker && !config.smart.enabled);

    // Vide dans l'environnement : ignorée, la liste du fichier tient.
    let config =
        Config::merge(with_yaml("modules: [smart]\n"), env(&[("DUMBMONIT_AGENT_MODULES", "")]))
            .expect("configuration");
    assert!(config.smart.enabled && !config.docker);

    // `none` : la collecte système de base, rien d'autre.
    let config =
        Config::merge(file_with_url_and_token(), env(&[("DUMBMONIT_AGENT_MODULES", "none")]))
            .expect("configuration");
    assert_eq!(config.modules.as_ref().map(BTreeSet::len), Some(0));
    assert!(!config.docker && !config.plakar.enabled && !config.system_health.enabled);
}

#[test]
fn an_explicit_flag_still_wins_over_the_module_list() {
    let config = Config::merge(with_yaml("modules: [docker]\nsmart: true\n"), env(&[]))
        .expect("configuration");
    assert!(config.smart.enabled);
    assert!(!config.zfs.enabled);
}

#[test]
fn an_unknown_module_is_refused_with_the_known_names() {
    let error = Config::merge(with_yaml("modules: [docker, plakr]\n"), env(&[]))
        .expect_err("module inconnu")
        .to_string();
    assert!(error.contains("plakr"), "{error}");
    assert!(error.contains("plakar"), "{error}");
}

#[test]
fn custom_checks_are_off_by_default_and_read_their_directories() {
    let config = Config::merge(file_with_url_and_token(), env(&[])).expect("configuration");
    assert!(!config.scripts.enabled, "refusés tant que la machine ne les demande pas");
    assert_eq!(config.scripts.dirs.len(), 1, "le dossier par défaut");
    assert!(config.scripts.user.is_none());

    let yaml = "scripts: true\n\
                scripts_dirs: [/etc/dumbmonit/scripts.d, ' /usr/lib/nagios/plugins ', '']\n\
                scripts_user: nagios\n\
                scripts_max_concurrent: 99\n";
    let config = Config::merge(from_yaml(yaml), env(&[])).expect("configuration");
    assert!(config.scripts.enabled);
    let dirs: Vec<&str> = config.scripts.dirs.iter().filter_map(|d| d.to_str()).collect();
    assert_eq!(dirs, ["/etc/dumbmonit/scripts.d", "/usr/lib/nagios/plugins"]);
    assert_eq!(config.scripts.user.as_deref(), Some("nagios"));
    assert_eq!(config.scripts.max_concurrent, 16, "borné");

    let variables = env(&[
        ("DUMBMONIT_AGENT_SCRIPTS", "no"),
        ("DUMBMONIT_AGENT_SCRIPTS_DIRS", "/opt/checks,/srv/checks"),
    ]);
    let config = Config::merge(from_yaml(yaml), variables).expect("configuration");
    assert!(!config.scripts.enabled, "la variable prime");
    assert_eq!(config.scripts.dirs.len(), 2);
    let invalid = env(&[("DUMBMONIT_AGENT_SCRIPTS", "perhaps")]);
    assert!(Config::merge(from_yaml(yaml), invalid).is_err());
}
