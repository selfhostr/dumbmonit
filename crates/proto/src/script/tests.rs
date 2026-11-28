use super::*;

fn metric<'a>(metrics: &'a [CheckMetric], name: &str) -> &'a CheckMetric {
    metrics.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name} absent: {metrics:?}"))
}

fn check() -> ScriptCheck {
    ScriptCheck {
        id: 4,
        command: "check_disk".into(),
        args: vec!["-w".into(), "20%".into()],
        env: BTreeMap::new(),
        timeout_secs: 10,
        interval_secs: 60,
        format: FORMAT_NAGIOS.into(),
        secrets: BTreeMap::new(),
    }
}

// ------------------------------------------------------------ Nagios

#[test]
fn the_exit_code_gives_the_status() {
    for (code, status) in [(0, 0), (1, 1), (2, 2), (3, 3), (4, 3), (-1, 3), (127, 3)] {
        assert_eq!(parse_nagios(Some(code), "X").status, status, "code {code}");
    }
    assert_eq!(parse_nagios(None, "killed").status, STATUS_UNKNOWN, "tué par un signal");
}

#[test]
fn a_check_disk_line_gives_its_message_and_metrics() {
    let out = parse_nagios(
        Some(0),
        "DISK OK - free space: / 3326 MB (56% inode=82%); | /=2643MB;5948;5958;0;5968\n",
    );
    assert_eq!(out.message, "DISK OK - free space: / 3326 MB (56% inode=82%);");
    let root = metric(&out.metrics, "/");
    let mib = 1024.0 * 1024.0;
    assert_eq!(root.value, 2643.0 * mib);
    assert_eq!(root.unit, "B");
    assert_eq!(root.warn, Some(5948.0 * mib));
    assert_eq!(root.crit, Some(5958.0 * mib));
    assert_eq!(root.min, Some(0.0));
    assert_eq!(root.max, Some(5968.0 * mib));
}

#[test]
fn long_output_and_multiline_perfdata_are_read() {
    let out = parse_nagios(
        Some(1),
        "DISK WARNING | /=2643MB;5948;5958;0;5968\n\
         / 15272 MB (77%);\n\
         /boot 68 MB (69%); | /boot=68MB;88;93;0;98\n\
         /home=69357MB;253404;253409;0;253414\n\
         /var/log=818MB;970;975;0;980\n",
    );
    assert_eq!(out.status, STATUS_WARNING);
    assert_eq!(out.message, "DISK WARNING");
    let names: Vec<&str> = out.metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["/", "/boot", "/home", "/var/log"]);
}

#[test]
fn quoted_labels_keep_spaces_equals_and_apostrophes() {
    let metrics = parse_perfdata("'C:\\ Label: Used Space'=12.5GB;;;0;100 'it''s=odd'=3");
    let gib = 1024.0 * 1024.0 * 1024.0;
    assert_eq!(metric(&metrics, "C:\\ Label: Used Space").value, 12.5 * gib);
    assert_eq!(metric(&metrics, "it's=odd").value, 3.0);
}

#[test]
fn units_are_normalised() {
    let metrics = parse_perfdata("time=250ms;500;1000 t2=3s fast=20us pct=99.5% c=1234c raw=7");
    assert_eq!(metric(&metrics, "time").value, 0.25);
    assert_eq!(metric(&metrics, "time").unit, "s");
    assert_eq!(metric(&metrics, "time").warn, Some(0.5));
    assert_eq!(metric(&metrics, "time").crit, Some(1.0));
    assert_eq!(metric(&metrics, "t2").value, 3.0);
    assert!((metric(&metrics, "fast").value - 20e-6).abs() < 1e-12);
    assert_eq!(metric(&metrics, "pct").unit, "%");
    assert!(metric(&metrics, "c").counter);
    assert_eq!(metric(&metrics, "c").value, 1234.0);
    assert_eq!(metric(&metrics, "raw").unit, "");
    assert_eq!(metric(&metrics, "raw").value, 7.0);
}

#[test]
fn byte_units_use_powers_of_1024() {
    let metrics = parse_perfdata("a=1KB b=1kB c=1MB d=1GB e=1TB f=10B");
    assert_eq!(metric(&metrics, "a").value, 1024.0);
    assert_eq!(metric(&metrics, "b").value, 1024.0);
    assert_eq!(metric(&metrics, "c").value, 1024.0 * 1024.0);
    assert_eq!(metric(&metrics, "d").value, 1024f64.powi(3));
    assert_eq!(metric(&metrics, "e").value, 1024f64.powi(4));
    assert_eq!(metric(&metrics, "f").value, 10.0);
}

#[test]
fn undetermined_unreadable_or_unknown_unit_values_are_skipped() {
    let metrics = parse_perfdata("a=U b= c=abc d=5parsecs e=NaN f=inf g=1 h=12;;;;");
    let names: Vec<&str> = metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["g", "h"]);
}

#[test]
fn ranges_give_a_threshold() {
    let metrics = parse_perfdata("a=1;10:20;~:30 b=1;@5:15;40: c=1;;;; d=1;x;y");
    assert_eq!(metric(&metrics, "a").warn, Some(20.0));
    assert_eq!(metric(&metrics, "a").crit, Some(30.0));
    assert_eq!(metric(&metrics, "b").warn, Some(15.0));
    assert_eq!(metric(&metrics, "b").crit, Some(40.0));
    assert_eq!(metric(&metrics, "c").warn, None);
    assert_eq!(metric(&metrics, "d").warn, None);
    assert_eq!(metric(&metrics, "d").crit, None);
}

#[test]
fn negative_exponent_and_comma_decimals_are_numbers() {
    let metrics = parse_perfdata("temp=-4.5 big=1e3 small=2.5E-2 fr=0,75 neg=-12MB");
    assert_eq!(metric(&metrics, "temp").value, -4.5);
    assert_eq!(metric(&metrics, "big").value, 1000.0);
    assert_eq!(metric(&metrics, "small").value, 0.025);
    assert_eq!(metric(&metrics, "fr").value, 0.75);
    assert_eq!(metric(&metrics, "neg").value, -12.0 * 1024.0 * 1024.0);
}

#[test]
fn garbage_between_entries_does_not_stop_the_parse() {
    let metrics = parse_perfdata("  junk  a=1 more junk b=2   ");
    let names: Vec<&str> = metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["a", "b"]);
}

#[test]
fn an_unterminated_quote_ends_the_parse_without_panicking() {
    assert!(parse_perfdata("'never closed=1").is_empty());
    assert!(parse_perfdata("'closed' =1").is_empty(), "espace avant le =");
    let metrics = parse_perfdata("a=1 'broken");
    assert_eq!(metrics.len(), 1);
}

#[test]
fn duplicate_labels_keep_the_first_and_the_count_is_capped() {
    let metrics = parse_perfdata("a=1 a=2");
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].value, 1.0);
    let many: String = (0..500).map(|i| format!("m{i}=1 ")).collect();
    assert_eq!(parse_perfdata(&many).len(), MAX_CHECK_METRICS);
}

#[test]
fn a_huge_amount_of_noise_does_not_overflow_the_stack() {
    let noise = "word ".repeat(20_000);
    assert!(parse_perfdata(&noise).is_empty());
}

#[test]
fn no_output_and_no_perfdata_are_fine() {
    let out = parse_nagios(Some(0), "");
    assert_eq!(out.message, "");
    assert!(out.metrics.is_empty());
    let out = parse_nagios(Some(2), "CRITICAL - socket timeout\n");
    assert_eq!(out.message, "CRITICAL - socket timeout");
    assert!(out.metrics.is_empty());
}

#[test]
fn the_message_is_bounded_and_without_control_characters() {
    let out = parse_nagios(Some(0), &format!("OK \u{1b}[31m{}", "x".repeat(5000)));
    assert_eq!(out.message.chars().count(), MAX_CHECK_MESSAGE);
    assert!(!out.message.chars().any(char::is_control));
}

#[test]
fn stderr_stands_in_for_an_empty_message() {
    let out = parse_output(FORMAT_NAGIOS, Some(3), "", "\n  check_foo: command not found\n")
        .expect("format connu");
    assert_eq!(out.message, "check_foo: command not found");
    assert!(parse_output("xml", Some(0), "", "").is_none(), "format inconnu");
}

// ------------------------------------------------------------ JSON

#[test]
fn json_output_gives_status_message_and_metrics() {
    let text = r#"{"status": "warning", "message": "3 jobs late",
                   "metrics": {"late_jobs": 3, "ratio": 0.5}}"#;
    let out = parse_json(Some(0), text);
    assert_eq!(out.status, STATUS_WARNING);
    assert_eq!(out.message, "3 jobs late");
    assert_eq!(metric(&out.metrics, "late_jobs").value, 3.0);
    assert_eq!(metric(&out.metrics, "ratio").value, 0.5);
}

#[test]
fn json_status_words_and_codes() {
    for (status, expected) in [
        ("\"ok\"", 0),
        ("\"UP\"", 0),
        ("\"degraded\"", 1),
        ("\"down\"", 2),
        ("\"critical\"", 2),
        ("\"unknown\"", 3),
        ("2", 2),
        ("0", 0),
    ] {
        let out = parse_json(Some(0), &format!(r#"{{"status": {status}}}"#));
        assert_eq!(out.status, expected, "{status}");
    }
    let out = parse_json(Some(0), r#"{"status": "maybe", "message": "hm"}"#);
    assert_eq!(out.status, STATUS_UNKNOWN);
    assert!(out.message.contains("unknown status"), "{}", out.message);
    assert_eq!(parse_json(Some(0), r#"{"status": 9}"#).status, STATUS_UNKNOWN);
}

#[test]
fn without_a_status_the_exit_code_decides() {
    assert_eq!(parse_json(Some(2), r#"{"message": "x"}"#).status, STATUS_CRITICAL);
    assert_eq!(parse_json(Some(0), r#"{"status": null}"#).status, STATUS_OK);
}

#[test]
fn json_metric_objects_carry_units_and_thresholds() {
    let out = parse_json(
        Some(0),
        r#"{"metrics": {"age": {"value": 1500, "unit": "ms", "warn": 1000, "crit": 2000},
            "bad": "text", "none": null, "list": [1], "n": {"unit": "s"}}}"#,
    );
    let age = metric(&out.metrics, "age");
    assert_eq!(age.value, 1.5);
    assert_eq!(age.unit, "s");
    assert_eq!(age.warn, Some(1.0));
    assert_eq!(age.crit, Some(2.0));
    assert_eq!(out.metrics.len(), 1, "le reste est écarté : {:?}", out.metrics);
}

#[test]
fn invalid_json_is_unknown_with_a_reason() {
    for text in ["", "not json", "[1, 2]", "{\"status\": \"ok\""] {
        let out = parse_json(Some(0), text);
        assert_eq!(out.status, STATUS_UNKNOWN, "{text}");
        assert!(!out.message.is_empty(), "{text}");
    }
}

// ------------------------------------------------------------ Prometheus

#[test]
fn prometheus_text_is_read_with_labels_and_counters() {
    let out = parse_prometheus(
        Some(0),
        "# HELP requests_total Requests.\n\
         # TYPE requests_total counter\n\
         requests_total{code=\"200\",path=\"/a b\"} 1027 1700000000000\n\
         requests_total{code=\"500\"} 3\n\
         temperature_celsius 21.5\n\
         weird{msg=\"a } \\\" b\"} 1\n\
         bad line here\n\
         nan_value NaN\n",
    );
    assert_eq!(out.status, STATUS_OK);
    let ok = metric(&out.metrics, "requests_total{code=\"200\",path=\"/a b\"}");
    assert_eq!(ok.value, 1027.0);
    assert!(ok.counter);
    assert_eq!(metric(&out.metrics, "temperature_celsius").value, 21.5);
    assert!(!metric(&out.metrics, "temperature_celsius").counter);
    assert_eq!(metric(&out.metrics, "weird{msg=\"a } \\\" b\"}").value, 1.0);
    assert_eq!(out.metrics.len(), 4);
    assert!(out.message.contains("2 unreadable"), "{}", out.message);
}

#[test]
fn prometheus_status_comes_from_the_exit_code() {
    assert_eq!(parse_prometheus(Some(2), "up 0\n").status, STATUS_CRITICAL);
}

// ------------------------------------------------------------ règles partagées

#[test]
fn commands_are_a_bare_name_or_an_absolute_path() {
    for good in [
        "check_disk",
        "backup-age.sh",
        "check.ps1",
        "/usr/lib/nagios/plugins/check_disk",
        "/etc/dumbmonit/scripts.d/my check",
        "C:\\ProgramData\\DumbMonit\\scripts\\check.ps1",
        "c:/scripts/check.exe",
    ] {
        assert!(validate_command(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        "  ",
        " check_disk",
        "plugins/check_disk",
        "./check_disk",
        "../check_disk",
        "/usr/lib/nagios/../../../bin/sh",
        "C:\\scripts\\..\\Windows\\System32\\cmd.exe",
        "\\\\server\\share\\evil.exe",
        "//server/share/evil",
        "check.exe:stream",
        "C:check.exe",
        "-rf",
        ".",
        "check\0disk",
        "check\ndisk",
    ] {
        assert!(validate_command(bad).is_err(), "{bad:?}");
    }
    assert!(validate_command(&"a".repeat(MAX_COMMAND_LEN + 1)).is_err());
}

#[test]
fn arguments_are_bounded_and_never_carry_nul_or_line_breaks() {
    let ok: Vec<String> = ["-H", "db; rm -rf /", "$(id)", "|", "a b"].map(String::from).to_vec();
    assert!(validate_args(&ok).is_ok(), "aucun shell : ce n'est que du texte");
    assert!(validate_args(&["a\0b".to_string()]).is_err());
    assert!(validate_args(&["a\nb".to_string()]).is_err());
    assert!(validate_args(&["x".repeat(MAX_CHECK_ARG_LEN + 1)]).is_err());
    let too_many: Vec<String> = std::iter::repeat_n("a".to_string(), MAX_CHECK_ARGS + 1).collect();
    assert!(validate_args(&too_many).is_err());
}

#[test]
fn dangerous_environment_variables_are_refused() {
    for name in [
        "LD_PRELOAD",
        "ld_library_path",
        "DYLD_INSERT_LIBRARIES",
        "PATH",
        "Path",
        "BASH_ENV",
        "IFS",
        "PYTHONPATH",
        "PERL5OPT",
        "NODE_OPTIONS",
        "COMSPEC",
        "DUMBMONIT_SECRET_TOKEN",
        "GLIBC_TUNABLES",
        "1ABC",
        "A-B",
        "",
    ] {
        assert!(!env_name_allowed(name), "{name}");
    }
    for name in ["MYSQL_HOST", "backup_dir", "_X", "LANG", "TZ", "HOME"] {
        assert!(env_name_allowed(name), "{name}");
    }
    let too_many: BTreeMap<String, String> =
        (0..=MAX_CHECK_ENV).map(|i| (format!("V{i}"), "x".to_string())).collect();
    assert!(validate_env(&too_many).is_err());
    let nul = BTreeMap::from([("V".to_string(), "a\0".to_string())]);
    assert!(validate_env(&nul).is_err());
}

#[test]
fn a_check_validates_as_a_whole() {
    assert!(check().validate().is_ok());
    assert!(ScriptCheck { timeout_secs: 0, ..check() }.validate().is_err());
    let too_long = CHECK_MAX_TIMEOUT_SECS + 1;
    assert!(ScriptCheck { timeout_secs: too_long, ..check() }.validate().is_err());
    assert!(ScriptCheck { format: "xml".into(), ..check() }.validate().is_err());
    let secrets = BTreeMap::from([("api".to_string(), "x".to_string())]);
    assert!(ScriptCheck { secrets, ..check() }.validate().is_err(), "secret inconnu");
}

#[test]
fn secrets_are_expanded_in_arguments_and_never_debug_printed() {
    let check = ScriptCheck {
        args: vec![
            "-u".into(),
            "{{secret.username}}".into(),
            "--password={{secret.password}}".into(),
            "{{secret.token}}".into(),
        ],
        secrets: BTreeMap::from([
            ("username".to_string(), "monitor".to_string()),
            ("password".to_string(), "s3cr3t".to_string()),
        ]),
        ..check()
    };
    assert_eq!(check.expanded_args(), ["-u", "monitor", "--password=s3cr3t", ""]);
    let shown = format!("{check:?}");
    assert!(!shown.contains("s3cr3t") && !shown.contains("monitor"), "{shown}");
}

#[test]
fn interval_and_timeout_are_clamped() {
    let check = ScriptCheck { interval_secs: 1, timeout_secs: 999, ..check() };
    assert_eq!(check.interval().as_secs(), CHECK_MIN_INTERVAL_SECS);
    assert_eq!(check.timeout().as_secs(), CHECK_MAX_TIMEOUT_SECS);
}

#[test]
fn an_old_agent_or_server_reads_the_new_fields_without_error() {
    // Format inconnu : la lecture réussit, l'agent refusera ce contrôle seul.
    let raw = r#"{"id": 1, "command": "c", "timeout_secs": 5, "interval_secs": 60,
                  "format": "future", "later_field": true}"#;
    let parsed: ScriptCheck = serde_json::from_str(raw).expect("lecture");
    assert_eq!(parsed.format, "future");
    let raw = r#"{"id": 1, "command": "c", "timeout_secs": 5, "interval_secs": 60}"#;
    let minimal: ScriptCheck = serde_json::from_str(raw).unwrap();
    assert_eq!(minimal.format, FORMAT_NAGIOS);
    let raw = r#"{"id": 1, "ran_at_ms": 5, "status": 7, "error": "later"}"#;
    let result: ScriptResult = serde_json::from_str(raw).unwrap();
    let clean = result.sanitized();
    assert_eq!(clean.status, STATUS_UNKNOWN);
    assert_eq!(clean.error.as_deref(), Some(ERROR_FAILED));
}

#[test]
fn a_received_result_is_bounded() {
    let mut metrics: Vec<CheckMetric> =
        (0..200).map(|i| CheckMetric::gauge(format!("m{i}"), 1.0)).collect();
    metrics.insert(0, CheckMetric::gauge("nan", f64::NAN));
    metrics.insert(0, CheckMetric::gauge("", 1.0));
    metrics.insert(0, CheckMetric::gauge("x".repeat(MAX_METRIC_NAME + 1), 1.0));
    let result = ScriptResult {
        id: 1,
        ran_at_ms: 0,
        duration_ms: 0,
        status: 1,
        message: "m".repeat(10_000),
        exit_code: Some(1),
        metrics,
        error: None,
    }
    .sanitized();
    assert_eq!(result.metrics.len(), MAX_CHECK_METRICS);
    assert_eq!(result.metrics[0].name, "m0");
    assert_eq!(result.message.chars().count(), MAX_CHECK_MESSAGE);
}
