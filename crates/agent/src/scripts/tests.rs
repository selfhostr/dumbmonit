//! Liste blanche, exécution et planification des contrôles personnalisés, sur
//! de vrais fichiers : des dossiers jetables et de petits scripts `sh`.

use std::collections::BTreeMap;

use dumbmonit_proto::script::{
    FORMAT_JSON, FORMAT_NAGIOS, STATUS_CRITICAL, STATUS_OK, STATUS_UNKNOWN, STATUS_WARNING,
};

use super::*;

fn check(id: i64, command: &str) -> ScriptCheck {
    ScriptCheck {
        id,
        command: command.into(),
        args: Vec::new(),
        env: BTreeMap::new(),
        timeout_secs: 10,
        interval_secs: 60,
        format: FORMAT_NAGIOS.into(),
        secrets: BTreeMap::new(),
    }
}

fn result(id: i64, ran_at_ms: i64) -> ScriptResult {
    ScriptResult::failure(id, ran_at_ms, ERROR_FAILED, "x")
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    /// Un dossier autorisé jetable, réservé à son propriétaire.
    fn scripts_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("dossier jetable");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        dir
    }

    /// Un script `sh` exécutable, en `0755`.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("écriture du script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn config(dir: &Path) -> ScriptsConfig {
        ScriptsConfig { enabled: true, dirs: vec![dir.to_path_buf()], ..ScriptsConfig::default() }
    }

    async fn run(config: &ScriptsConfig, check: &ScriptCheck) -> ScriptResult {
        run_check(config, &Ok(None), check).await
    }

    // -------------------------------------------------------- liste blanche

    #[test]
    fn a_bare_name_or_an_absolute_path_inside_the_directory_is_found() {
        let dir = scripts_dir();
        let path = script(dir.path(), "check_ok", "echo OK");
        let real = std::fs::canonicalize(&path).unwrap();
        let dirs = vec![dir.path().to_path_buf()];
        assert_eq!(allow::resolve(&dirs, "check_ok").unwrap().path, real);
        assert_eq!(allow::resolve(&dirs, path.to_str().unwrap()).unwrap().path, real);
    }

    #[test]
    fn the_first_directory_that_holds_the_name_wins() {
        let (first, second) = (scripts_dir(), scripts_dir());
        script(second.path(), "check_x", "echo second");
        let dirs = vec![first.path().to_path_buf(), second.path().to_path_buf()];
        let found = allow::resolve(&dirs, "check_x").unwrap().path;
        assert!(found.starts_with(std::fs::canonicalize(second.path()).unwrap()));
    }

    #[test]
    fn anything_outside_the_directories_is_refused() {
        let dir = scripts_dir();
        let elsewhere = scripts_dir();
        let outside = script(elsewhere.path(), "check_out", "echo OK");
        let dirs = vec![dir.path().to_path_buf()];
        for command in ["missing", "../check_out", "/bin/sh", "/etc/passwd"] {
            assert!(allow::resolve(&dirs, command).is_err(), "{command}");
        }
        let error = allow::resolve(&dirs, outside.to_str().unwrap()).unwrap_err();
        assert!(error.contains("not directly inside"), "{error}");
        assert!(allow::resolve(&[], "check_out").is_err(), "aucun dossier");
    }

    #[test]
    fn a_symbolic_link_leading_out_of_the_directory_is_refused() {
        let dir = scripts_dir();
        let elsewhere = scripts_dir();
        let target = script(elsewhere.path(), "real", "echo OK");
        std::os::unix::fs::symlink(&target, dir.path().join("check_link")).unwrap();
        std::os::unix::fs::symlink("/bin/sh", dir.path().join("check_shell")).unwrap();
        let dirs = vec![dir.path().to_path_buf()];
        let error = allow::resolve(&dirs, "check_link").unwrap_err();
        assert!(error.contains("symbolic links"), "{error}");
        assert!(allow::resolve(&dirs, "check_shell").is_err());
    }

    #[test]
    fn a_symbolic_link_within_the_directory_is_followed() {
        let dir = scripts_dir();
        let target = script(dir.path(), "check_real", "echo OK");
        std::os::unix::fs::symlink(&target, dir.path().join("check_alias")).unwrap();
        let dirs = vec![dir.path().to_path_buf()];
        let found = allow::resolve(&dirs, "check_alias").unwrap().path;
        assert_eq!(found, std::fs::canonicalize(target).unwrap());
    }

    #[test]
    fn a_subdirectory_is_not_an_allowed_directory() {
        let dir = scripts_dir();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = script(&sub, "check_deep", "echo OK");
        let dirs = vec![dir.path().to_path_buf()];
        assert!(allow::resolve(&dirs, "sub/check_deep").is_err());
        assert!(allow::resolve(&dirs, path.to_str().unwrap()).is_err());
    }

    #[test]
    fn a_file_that_is_not_executable_or_writable_by_others_is_refused() {
        let dir = scripts_dir();
        let dirs = vec![dir.path().to_path_buf()];
        let plain = script(dir.path(), "check_plain", "echo OK");
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        let error = allow::resolve(&dirs, "check_plain").unwrap_err();
        assert!(error.contains("not executable"), "{error}");
        let open = script(dir.path(), "check_open", "echo OK");
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        let error = allow::resolve(&dirs, "check_open").unwrap_err();
        assert!(error.contains("chmod go-w"), "{error}");
        std::fs::create_dir(dir.path().join("check_dir")).unwrap();
        assert!(allow::resolve(&dirs, "check_dir").is_err(), "un dossier n'est pas un fichier");
    }

    #[test]
    fn a_directory_writable_by_others_is_refused_with_the_reason() {
        let dir = scripts_dir();
        script(dir.path(), "check_ok", "echo OK");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let error = allow::resolve(&[dir.path().to_path_buf()], "check_ok").unwrap_err();
        assert!(error.contains("chmod go-w"), "{error}");
    }

    #[test]
    fn shells_interpreters_and_wrapper_plugins_are_never_run() {
        let dir = scripts_dir();
        let dirs = vec![dir.path().to_path_buf()];
        let denied = ["bash", "sh", "python3.12", "perl", "negate", "urlize", "check_by_ssh"];
        for name in denied {
            script(dir.path(), name, "echo OK");
            let error = allow::resolve(&dirs, name).unwrap_err();
            assert!(error.contains("never run"), "{name}: {error}");
        }
        for name in ["check_python_jobs", "pythonic", "check_disk"] {
            script(dir.path(), name, "echo OK");
            assert!(allow::resolve(&dirs, name).is_ok(), "{name}");
        }
    }

    #[test]
    fn passwd_entries_give_uid_and_gid() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n\
                      broken:x:abc:1::/:/bin/false\n";
        assert_eq!(allow::parse_passwd(passwd, "nobody"), Some((65534, 65534)));
        assert_eq!(allow::parse_passwd(passwd, "root"), Some((0, 0)));
        assert_eq!(allow::parse_passwd(passwd, "broken"), None);
        assert_eq!(allow::parse_passwd(passwd, "nob"), None, "nom exact seulement");
        assert_eq!(allow::lookup_user("1000:1001"), Ok((1000, 1001)));
    }

    // -------------------------------------------------------- exécution

    #[tokio::test]
    async fn nothing_runs_while_the_feature_is_off() {
        let dir = scripts_dir();
        let marker = dir.path().join("ran");
        script(dir.path(), "check_touch", &format!("touch {}", marker.display()));
        let config = ScriptsConfig { enabled: false, ..config(dir.path()) };
        let result = run(&config, &check(1, "check_touch")).await;
        assert_eq!(result.error.as_deref(), Some(ERROR_REFUSED));
        assert_eq!(result.message, DISABLED);
        assert!(!marker.exists(), "le script n'a pas tourné");
    }

    #[tokio::test]
    async fn a_nagios_plugin_gives_status_message_and_perfdata() {
        let dir = scripts_dir();
        script(dir.path(), "check_load", "echo 'LOAD WARNING - 4.2 | load1=4.2;4;8;0'\nexit 1");
        let result = run(&config(dir.path()), &check(7, "check_load")).await;
        assert_eq!(result.id, 7);
        assert_eq!(result.status, STATUS_WARNING);
        assert_eq!(result.exit_code, Some(1));
        assert_eq!(result.message, "LOAD WARNING - 4.2");
        assert_eq!(result.metrics.len(), 1);
        assert_eq!(result.metrics[0].value, 4.2);
        assert_eq!(result.metrics[0].crit, Some(8.0));
        assert!(result.error.is_none());

        script(dir.path(), "check_down", "echo 'CRITICAL - down'\nexit 2");
        let result = run(&config(dir.path()), &check(8, "check_down")).await;
        assert_eq!(result.status, STATUS_CRITICAL);
    }

    #[tokio::test]
    async fn a_json_script_is_read() {
        let dir = scripts_dir();
        let body = r#"echo '{"status":"ok","message":"fine","metrics":{"n":3}}'"#;
        script(dir.path(), "jobs.sh", body);
        let check = ScriptCheck { format: FORMAT_JSON.into(), ..check(2, "jobs.sh") };
        let result = run(&config(dir.path()), &check).await;
        assert_eq!(result.status, STATUS_OK);
        assert_eq!(result.message, "fine");
        assert_eq!(result.metrics[0].name, "n");
    }

    #[tokio::test]
    async fn arguments_reach_the_script_verbatim_without_a_shell() {
        let dir = scripts_dir();
        script(dir.path(), "check_echo", r#"printf 'OK %s|%s\n' "$1" "$2""#);
        let check = ScriptCheck {
            args: vec!["$(touch /tmp/never); echo pwned".into(), "{{secret.password}}".into()],
            secrets: BTreeMap::from([("password".to_string(), "p@ss".to_string())]),
            ..check(3, "check_echo")
        };
        let result = run(&config(dir.path()), &check).await;
        assert_eq!(result.message, "OK $(touch /tmp/never); echo pwned");
        // `|` ouvre les données de performance : « p@ss » n'en est pas une.
        assert!(result.metrics.is_empty());
    }

    #[tokio::test]
    async fn the_environment_is_minimal_plus_what_the_check_sets() {
        let dir = scripts_dir();
        let body = r#"echo "OK [$MY_VAR] [$DUMBMONIT_CHECK_ID] [$DUMBMONIT_SECRET_TOKEN] [$LANG]""#;
        script(dir.path(), "check_env", body);
        let check = ScriptCheck {
            env: BTreeMap::from([("MY_VAR".to_string(), "hello world".to_string())]),
            secrets: BTreeMap::from([("token".to_string(), "t0k".to_string())]),
            ..check(42, "check_env")
        };
        let result = run(&config(dir.path()), &check).await;
        assert_eq!(result.message, "OK [hello world] [42] [t0k] [C]");
    }

    #[tokio::test]
    async fn a_dangerous_variable_is_refused_before_anything_runs() {
        let dir = scripts_dir();
        script(dir.path(), "check_ok", "echo OK");
        let check = ScriptCheck {
            env: BTreeMap::from([("LD_PRELOAD".to_string(), "/tmp/evil.so".to_string())]),
            ..check(5, "check_ok")
        };
        let result = run(&config(dir.path()), &check).await;
        assert_eq!(result.error.as_deref(), Some(ERROR_REFUSED));
        assert!(result.message.contains("LD_PRELOAD"), "{}", result.message);
    }

    #[tokio::test]
    async fn a_check_past_its_timeout_is_killed() {
        let dir = scripts_dir();
        script(dir.path(), "check_slow", "sleep 30\necho OK");
        let check = ScriptCheck { timeout_secs: 1, ..check(6, "check_slow") };
        let started = std::time::Instant::now();
        let result = run(&config(dir.path()), &check).await;
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "{:?}", started.elapsed());
        assert_eq!(result.error.as_deref(), Some(ERROR_TIMEOUT));
        assert_eq!(result.status, STATUS_UNKNOWN);
    }

    /// Le délai tue tout le groupe : un descendant lancé en arrière-plan ne
    /// survit pas au contrôle.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn the_whole_process_group_is_killed_on_timeout() {
        let dir = scripts_dir();
        let pid_file = dir.path().join("child.pid");
        script(
            dir.path(),
            "check_spawner",
            &format!("sleep 60 &\necho $! > {}\nsleep 60", pid_file.display()),
        );
        let check = ScriptCheck { timeout_secs: 1, ..check(9, "check_spawner") };
        let result = run(&config(dir.path()), &check).await;
        assert_eq!(result.error.as_deref(), Some(ERROR_TIMEOUT));
        let pid = std::fs::read_to_string(&pid_file).expect("pid du descendant");
        let proc = PathBuf::from(format!("/proc/{}", pid.trim()));
        let mut gone = false;
        for _ in 0..50 {
            // Tué, il peut rester un instant zombie avant que init ne le récolte.
            let state = std::fs::read_to_string(proc.join("stat")).unwrap_or_default();
            if state.is_empty() || state.contains(") Z") {
                gone = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(gone, "le descendant {pid} a survécu au délai");
    }

    #[tokio::test]
    async fn a_flood_of_output_is_capped_and_does_not_block() {
        let dir = scripts_dir();
        // 1 Mio de sortie : bien plus que le tampon d'un tuyau et que la borne.
        let body = "echo 'OK flood'\nhead -c 1048576 /dev/zero | tr '\\0' x";
        script(dir.path(), "check_flood", body);
        let started = std::time::Instant::now();
        let result = run(&config(dir.path()), &check(10, "check_flood")).await;
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert_eq!(result.status, STATUS_OK);
        assert_eq!(result.message, "OK flood");
    }

    #[tokio::test]
    async fn stderr_explains_a_silent_failure() {
        let dir = scripts_dir();
        script(dir.path(), "check_err", "echo 'database unreachable' >&2\nexit 2");
        let result = run(&config(dir.path()), &check(11, "check_err")).await;
        assert_eq!(result.status, STATUS_CRITICAL);
        assert_eq!(result.message, "database unreachable");
    }

    #[tokio::test]
    async fn a_misconfigured_user_refuses_every_check() {
        let dir = scripts_dir();
        script(dir.path(), "check_ok", "echo OK");
        let error = Err("scripts_user: no account named 'ghost'".to_string());
        let result = run_check(&config(dir.path()), &error, &check(12, "check_ok")).await;
        assert_eq!(result.error.as_deref(), Some(ERROR_REFUSED));
        assert!(result.message.contains("ghost"));
    }

    // -------------------------------------------------------- planification

    #[tokio::test]
    async fn the_runner_runs_new_checks_and_keeps_their_verdict_until_acknowledged() {
        let dir = scripts_dir();
        script(dir.path(), "check_ok", "echo 'OK fine'");
        let mut runner = ScriptRunner::new(config(dir.path()));
        assert!(runner.pending().is_none());
        runner.set_checks(vec![check(1, "check_ok")]);
        let mut pending = None;
        for _ in 0..100 {
            pending = runner.pending();
            if pending.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let pending = pending.expect("un verdict en quelques secondes");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].message, "OK fine");
        runner.acknowledge(Some(pending.as_slice()));
        assert!(runner.pending().is_none(), "emporté par le lot");
    }
}

#[test]
fn an_acknowledgement_keeps_a_newer_verdict() {
    let runner = ScriptRunner::new(ScriptsConfig::default());
    runner.shared.lock().unwrap().results.insert(1, result(1, 2_000));
    runner.acknowledge(Some(&[result(1, 1_000)][..]));
    assert_eq!(runner.pending().unwrap()[0].ran_at_ms, 2_000, "plus récent que l'envoi");
    runner.acknowledge(Some(&[result(1, 2_000)][..]));
    assert!(runner.pending().is_none());
    runner.acknowledge(None);
}

#[tokio::test]
async fn new_checks_replace_the_old_ones_and_drop_their_verdicts() {
    let mut runner = ScriptRunner::new(ScriptsConfig::default());
    runner.set_checks(vec![check(1, "a"), check(2, "b")]);
    runner.shared.lock().unwrap().results.insert(1, result(1, 5));
    runner.shared.lock().unwrap().results.insert(2, result(2, 5));
    runner.set_checks(vec![check(2, "b")]);
    let shared = runner.shared.lock().unwrap();
    assert_eq!(shared.checks.keys().copied().collect::<Vec<_>>(), [2]);
    assert_eq!(shared.results.keys().copied().collect::<Vec<_>>(), [2]);
}

#[tokio::test]
async fn an_unchanged_check_keeps_its_schedule_and_a_changed_one_runs_again() {
    let mut runner = ScriptRunner::new(ScriptsConfig::default());
    runner.set_checks(vec![check(1, "a")]);
    let later = Instant::now() + Duration::from_secs(600);
    runner.shared.lock().unwrap().checks.get_mut(&1).unwrap().next_due = later;
    runner.set_checks(vec![check(1, "a")]);
    assert_eq!(runner.shared.lock().unwrap().checks[&1].next_due, later);
    runner.set_checks(vec![ScriptCheck { args: vec!["-v".into()], ..check(1, "a") }]);
    assert!(runner.shared.lock().unwrap().checks[&1].next_due < later);
}

#[tokio::test]
async fn a_disabled_agent_answers_every_check_with_the_reason() {
    let result = run_check(&ScriptsConfig::default(), &Ok(None), &check(3, "check_disk")).await;
    assert_eq!(result.error.as_deref(), Some(ERROR_REFUSED));
    assert_eq!(result.message, DISABLED);
    assert_eq!(result.status, STATUS_UNKNOWN);
}

#[test]
fn the_default_directory_sits_next_to_the_configuration() {
    let dirs = default_dirs(Some(Path::new("/etc/dumbmonit/agent.yaml")));
    let expected = if cfg!(windows) { "scripts" } else { "scripts.d" };
    assert_eq!(dirs, [PathBuf::from("/etc/dumbmonit").join(expected)]);
}
