//! Contrôles personnalisés : lecture des options, remise aux agents, verdicts.

use std::collections::BTreeMap;

use dumbmonit_proto::CheckMetric;
use dumbmonit_proto::script::{ERROR_FAILED, ERROR_REFUSED, FORMAT_JSON};

use super::*;

fn target(tags: &[(&str, &str)]) -> Target {
    Target {
        id: 12,
        name: "nas · disk".into(),
        address: "disk".into(),
        kind: KIND.into(),
        profile_id: None,
        parent_id: None,
        interval: Duration::from_secs(60),
        enabled: true,
        tags: tags.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        credential: Credential::None,
        group_name: String::new(),
        position: 0,
    }
}

fn stored(status: u8, message: &str, error: Option<&str>, received_at_ms: i64) -> Stored {
    Stored {
        agent_id: 3,
        offered_at_ms: 0,
        received_at_ms: Some(received_at_ms),
        status: Some(status),
        message: message.into(),
        error: error.map(str::to_string),
    }
}

const DEADLINE: Duration = Duration::from_secs(300);
const TIMEOUT: Duration = Duration::from_secs(10);
const NOW: i64 = 10_000_000;

fn judge(stored: Option<&Stored>) -> Result<Vec<Sample>, ProbeError> {
    verdict(stored, 3, NOW - 1_000, DEADLINE, TIMEOUT, NOW)
}

// ------------------------------------------------------------ options

#[test]
fn the_form_rows_become_arguments_and_environment() {
    let check = check_from_target(&target(&[
        (OPTION_AGENT, "3"),
        (OPTION_COMMAND, "check_disk"),
        (OPTION_ARGS, r#"[{"value":"-w"},{"value":"20%"},{"value":"-p"},{"value":"/"}]"#),
        (OPTION_ENV, r#"[{"name":"LC_NUMERIC","value":"C"}]"#),
        (OPTION_TIMEOUT, "30"),
        (OPTION_FORMAT, "nagios"),
    ]))
    .expect("contrôle valide");
    assert_eq!(check.id, 12);
    assert_eq!(check.command, "check_disk");
    assert_eq!(check.args, ["-w", "20%", "-p", "/"]);
    assert_eq!(check.env, BTreeMap::from([("LC_NUMERIC".to_string(), "C".to_string())]));
    assert_eq!(check.timeout_secs, 30);
    assert_eq!(check.interval_secs, 60);
}

#[test]
fn the_api_may_send_plain_lists_and_objects() {
    assert_eq!(parse_args(r#"["-H", "db.lan"]"#).unwrap(), ["-H", "db.lan"]);
    assert_eq!(parse_env(r#"{"A": "1"}"#).unwrap()["A"], "1");
    assert!(parse_args("").unwrap().is_empty());
    assert!(parse_env("  ").unwrap().is_empty());
    for bad in ["-w 20%", "[1]", r#"[{"x": "y"}]"#, "{}"] {
        assert!(parse_args(bad).is_err(), "{bad}");
    }
    for bad in ["A=1", r#"[{"value": "x"}]"#, r#"{"A": 1}"#] {
        assert!(parse_env(bad).is_err(), "{bad}");
    }
}

#[test]
fn defaults_and_bad_settings() {
    let check = check_from_target(&target(&[(OPTION_COMMAND, "check_load")])).unwrap();
    assert_eq!(check.timeout_secs, CHECK_DEFAULT_TIMEOUT_SECS);
    assert_eq!(check.format, FORMAT_NAGIOS);
    for tags in [
        vec![],
        vec![(OPTION_COMMAND, "../bin/sh")],
        vec![(OPTION_COMMAND, "check_load"), (OPTION_TIMEOUT, "0")],
        vec![(OPTION_COMMAND, "check_load"), (OPTION_TIMEOUT, "121")],
        vec![(OPTION_COMMAND, "check_load"), (OPTION_TIMEOUT, "soon")],
        vec![(OPTION_COMMAND, "check_load"), (OPTION_FORMAT, "xml")],
        vec![(OPTION_COMMAND, "check_load"), (OPTION_ENV, r#"{"LD_PRELOAD": "/x.so"}"#)],
    ] {
        let error = check_from_target(&target(&tags)).unwrap_err();
        assert!(matches!(error, ProbeError::Config(_)), "{tags:?}: {error}");
    }
    assert!(agent_of(&target(&[])).is_err());
    assert_eq!(agent_of(&target(&[(OPTION_AGENT, " 7 ")])).unwrap(), 7);
}

#[test]
fn the_credential_travels_as_secrets_only() {
    let mut with_login = target(&[(OPTION_COMMAND, "check_mysql")]);
    with_login.credential =
        Credential::UsernamePassword { username: "mon".into(), password: "pw".into() };
    let check = check_from_target(&with_login).unwrap();
    assert_eq!(check.secrets["username"], "mon");
    assert_eq!(check.secrets["password"], "pw");
    let mut with_token = target(&[(OPTION_COMMAND, "check_api")]);
    with_token.credential = Credential::ApiToken { token: "tok".into() };
    assert_eq!(check_from_target(&with_token).unwrap().secrets["token"], "tok");
}

// ------------------------------------------------------------ verdict

#[test]
fn ok_and_warning_are_up() {
    for status in [STATUS_OK, STATUS_WARNING] {
        let samples = judge(Some(&stored(status, "fine", None, NOW - 30_000))).expect("en service");
        assert_eq!(samples[0].metric, "script_last_result_age_seconds");
        assert_eq!(samples[0].value, 30.0);
    }
}

#[test]
fn critical_is_down_with_the_message_of_the_check() {
    let error = judge(Some(&stored(STATUS_CRITICAL, "DISK CRITICAL - / 98%", None, NOW)));
    let error = error.unwrap_err();
    assert!(error.means_down());
    assert!(error.to_string().contains("DISK CRITICAL - / 98%"), "{error}");
    let error = judge(Some(&stored(STATUS_CRITICAL, "3 jobs late", None, NOW))).unwrap_err();
    assert!(error.to_string().contains("CRITICAL: 3 jobs late"), "{error}");
    let error = judge(Some(&stored(STATUS_CRITICAL, "", None, NOW))).unwrap_err();
    assert!(error.to_string().contains("returned CRITICAL"), "{error}");
}

#[test]
fn unknown_and_refusals_are_configuration_errors() {
    for (status, error) in [
        (STATUS_UNKNOWN, None),
        (STATUS_UNKNOWN, Some(ERROR_REFUSED)),
        (STATUS_UNKNOWN, Some(ERROR_FAILED)),
        (STATUS_OK, Some(ERROR_REFUSED)),
    ] {
        let verdict = judge(Some(&stored(status, "not in scripts_dirs", error, NOW))).unwrap_err();
        assert!(matches!(verdict, ProbeError::Config(_)), "{status} {error:?}: {verdict}");
        assert!(!verdict.means_down());
    }
}

#[test]
fn a_timeout_is_down() {
    let error = judge(Some(&stored(STATUS_UNKNOWN, "killed", Some(ERROR_TIMEOUT), NOW)));
    assert!(matches!(error, Err(ProbeError::Timeout(t)) if t == TIMEOUT));
}

#[test]
fn a_silent_check_is_unreachable() {
    let error = judge(Some(&stored(STATUS_OK, "ok", None, NOW - 301_000))).unwrap_err();
    assert!(error.means_down());
    assert!(error.to_string().contains("no result"), "{error}");
}

#[test]
fn a_fresh_check_waits_for_its_agent_then_says_why_it_is_silent() {
    // Jamais remis : on attend un lot de l'agent.
    assert!(verdict(None, 3, NOW - 1_000, DEADLINE, TIMEOUT, NOW).unwrap().is_empty());
    let error = verdict(None, 3, NOW - 400_000, DEADLINE, TIMEOUT, NOW).unwrap_err();
    assert!(error.to_string().contains("not collected"), "{error}");
    // Remis, pas encore de verdict.
    let offered = Stored {
        received_at_ms: None,
        status: None,
        offered_at_ms: NOW - 1_000,
        ..stored(0, "", None, 0)
    };
    assert!(judge(Some(&offered)).unwrap().is_empty());
    let late = Stored { offered_at_ms: NOW - 400_000, ..offered.clone() };
    let error = judge(Some(&late)).unwrap_err();
    assert!(error.to_string().contains("update the agent"), "{error}");
    // Remis à un autre agent : comme jamais remis à celui-ci.
    let other = Stored { agent_id: 99, ..stored(STATUS_OK, "", None, NOW) };
    assert!(verdict(Some(&other), 3, NOW - 1_000, DEADLINE, TIMEOUT, NOW).unwrap().is_empty());
}

#[test]
fn the_deadline_covers_three_periods_and_the_timeout() {
    let deadline = deadline(Duration::from_secs(60), Duration::from_secs(10));
    assert_eq!(deadline, Duration::from_secs(180 + 10) + DELIVERY_SLACK);
}

// ------------------------------------------------------------ mesures

#[test]
fn a_verdict_becomes_labelled_series_without_the_settings() {
    let target = target(&[
        (OPTION_AGENT, "3"),
        (OPTION_COMMAND, "check_disk"),
        (OPTION_ARGS, r#"["-w","20%"]"#),
        ("room", "cellar"),
    ]);
    let mut root = CheckMetric::gauge("/", 2048.0);
    root.unit = "B".into();
    root.warn = Some(4096.0);
    let mut errors = CheckMetric::gauge("errors", 7.0);
    errors.counter = true;
    let result = ScriptResult {
        id: 12,
        ran_at_ms: NOW - 5_000,
        duration_ms: 1_500,
        status: STATUS_WARNING,
        message: "DISK WARNING".into(),
        exit_code: Some(1),
        metrics: vec![root, errors, CheckMetric::gauge("bad", f64::NAN)],
        error: None,
    };
    let samples = samples_for(&target, &result, NOW);
    let find = |metric: &str, name: Option<&str>| {
        samples
            .iter()
            .find(|s| s.metric == metric && s.labels.get("name").map(String::as_str) == name)
            .unwrap_or_else(|| panic!("{metric} {name:?} absent: {samples:?}"))
    };
    assert_eq!(find("script_status", None).value, 1.0);
    assert_eq!(find("script_warning", None).value, 1.0);
    assert_eq!(find("script_duration_seconds", None).value, 1.5);
    let value = find("script_value", Some("/"));
    assert_eq!(value.value, 2048.0);
    assert_eq!(value.labels["unit"], "B");
    assert_eq!(value.ts_ms, NOW - 5_000);
    assert_eq!(find("script_value_warning", Some("/")).value, 4096.0);
    assert_eq!(find("script_value", Some("errors")).kind, MetricKind::Counter);
    assert!(samples.iter().all(|s| s.value.is_finite()));
    for sample in &samples {
        assert_eq!(sample.labels["target"], "12");
        assert_eq!(sample.labels.get("tag_room").map(String::as_str), Some("cellar"));
        for option in ["tag_command", "tag_args", "tag_agent_id"] {
            assert!(!sample.labels.contains_key(option), "{option} : {sample:?}");
        }
    }
}

#[test]
fn a_refusal_writes_its_status_but_no_duration() {
    let result = ScriptResult::failure(12, NOW, ERROR_REFUSED, "disabled");
    let samples = samples_for(&target(&[]), &result, NOW);
    let names: Vec<&str> = samples.iter().map(|s| s.metric.as_str()).collect();
    assert_eq!(names, ["script_status", "script_warning"]);
    assert_eq!(samples[0].value, 3.0);
}

#[test]
fn a_clock_far_off_is_brought_back_into_the_accepted_window() {
    let result = ScriptResult::failure(1, NOW + 10 * 3_600_000, ERROR_FAILED, "");
    let samples = samples_for(&target(&[]), &result, NOW);
    assert!(samples.iter().all(|s| s.ts_ms <= NOW + MAX_SKEW_AHEAD_MS));
}

// ------------------------------------------------------------ base

struct TestDb {
    pool: SqlitePool,
    cipher: Cipher,
    _dir: tempfile::TempDir,
}

async fn setup() -> TestDb {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let pool = db::open(&dir.path().join("test.db")).await.expect("base");
    let cipher =
        db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("chiffrement");
    TestDb { pool, cipher, _dir: dir }
}

async fn create(db: &TestDb, name: &str, kind: &str, tags: &[(&str, String)]) -> TargetId {
    let input = db::targets::TargetInput {
        name: name.into(),
        address: name.into(),
        kind: kind.into(),
        profile_id: None,
        parent_id: None,
        via_agent: None,
        interval: Duration::from_secs(60),
        enabled: true,
        tags: tags.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        credential: Some(Credential::None),
        group_name: String::new(),
    };
    db::targets::create(&db.pool, &db.cipher, &input).await.expect("cible")
}

fn ok_result(id: TargetId, status: u8, ran_at_ms: i64) -> ScriptResult {
    ScriptResult {
        id,
        ran_at_ms,
        duration_ms: 20,
        status,
        message: "all good".into(),
        exit_code: Some(i32::from(status)),
        metrics: vec![CheckMetric::gauge("n", 1.0)],
        error: None,
    }
}

#[tokio::test]
async fn checks_go_to_their_agent_only_and_verdicts_cannot_be_forged() {
    let db = setup().await;
    let nas = create(&db, "nas", "agent", &[]).await;
    let web = create(&db, "web", "agent", &[]).await;
    let on_nas = |command: &str| {
        vec![(OPTION_AGENT, nas.to_string()), (OPTION_COMMAND, command.to_string())]
    };
    let disk = create(&db, "nas-disk", KIND, &on_nas("check_disk")).await;
    let broken = create(&db, "nas-broken", KIND, &on_nas("../evil")).await;
    let json = {
        let mut tags = on_nas("jobs.sh");
        tags.push((OPTION_FORMAT, FORMAT_JSON.to_string()));
        create(&db, "nas-jobs", KIND, &tags).await
    };

    let now = chrono::Utc::now().timestamp_millis();
    let for_nas = offered(&db.pool, &db.cipher, nas, now).await.unwrap();
    let ids: Vec<TargetId> = for_nas.iter().map(|o| o.check.id).collect();
    assert_eq!(ids, [disk, json], "le contrôle mal réglé ({broken}) n'est pas remis");

    // La machine « web » ne peut pas écrire dans le contrôle de « nas ».
    let for_web = offered(&db.pool, &db.cipher, web, now).await.unwrap();
    assert!(for_web.is_empty());
    let forged = record(&db.pool, web, &for_web, vec![ok_result(disk, 2, now)], now).await.unwrap();
    assert!(forged.is_empty());
    let collector = AgentScriptCollector::new(db.pool.clone());
    let disk_target = db::targets::get(&db.pool, &db.cipher, disk).await.unwrap().unwrap();
    assert!(collector.probe(&disk_target).await.unwrap().is_empty(), "toujours en attente");

    // Le vrai verdict de « nas » est rangé et lu.
    let verdicts = vec![ok_result(disk, 2, now), ok_result(999, 0, now)];
    let samples = record(&db.pool, nas, &for_nas, verdicts, now).await.unwrap();
    assert!(samples.iter().any(|s| s.metric == "script_value"));
    let stored = super::stored(&db.pool, disk).await.unwrap().unwrap();
    assert_eq!(stored.status, Some(STATUS_CRITICAL));
    assert_eq!(stored.message, "all good");
}

#[tokio::test]
async fn the_collector_reads_the_last_verdict_and_follows_a_change_of_agent() {
    let db = setup().await;
    let nas = create(&db, "nas", "agent", &[]).await;
    let web = create(&db, "web", "agent", &[]).await;
    let tags = [(OPTION_AGENT, nas.to_string()), (OPTION_COMMAND, "check_load".to_string())];
    let load = create(&db, "nas-load", KIND, &tags).await;
    let now = chrono::Utc::now().timestamp_millis();
    let for_nas = offered(&db.pool, &db.cipher, nas, now).await.unwrap();
    record(&db.pool, nas, &for_nas, vec![ok_result(load, STATUS_OK, now)], now).await.unwrap();
    let collector = AgentScriptCollector::new(db.pool.clone());
    let target = db::targets::get(&db.pool, &db.cipher, load).await.unwrap().unwrap();
    let samples = collector.probe(&target).await.expect("en service");
    assert_eq!(samples[0].metric, "script_last_result_age_seconds");

    // Confié à « web » : le verdict de « nas » est effacé.
    offered(&db.pool, &db.cipher, web, now).await.unwrap();
    let stored = super::stored(&db.pool, load).await.unwrap().unwrap();
    assert_eq!((stored.agent_id, stored.received_at_ms), (web, None));

    // L'agent supprimé : une erreur de réglage, pas une panne.
    let mut orphan = target.clone();
    orphan.tags.insert(OPTION_AGENT.into(), "424242".into());
    let error = collector.probe(&orphan).await.unwrap_err();
    assert!(matches!(error, ProbeError::Config(_)), "{error}");
}
