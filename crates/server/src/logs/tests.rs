use super::*;
use tracing_subscriber::layer::SubscriberExt;

/// Couche de test : capture les enregistrements construits.
struct Capture(Arc<Mutex<Vec<Record>>>);

impl<S: Subscriber> tracing_subscriber::Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        self.0.lock().unwrap().push(build_record(event));
    }
}

fn capture(emit: impl FnOnce()) -> Vec<Record> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(Capture(seen.clone()));
    tracing::subscriber::with_default(subscriber, emit);
    seen.lock().unwrap().clone()
}

#[test]
fn un_evenement_devient_une_ligne_json_complete() {
    let records = capture(|| {
        tracing::warn!(target: "dumbmonit_server::scheduler", target_id = 4_i64, ok = false, kind = %"snmp", "probe failed");
    });
    let line: Value = serde_json::from_str(&records[0].to_line()).unwrap();
    assert_eq!(line["level"], "WARN");
    assert_eq!(line["msg"], "probe failed");
    assert_eq!(line["fields"]["target_id"], 4);
    assert_eq!(line["fields"]["ok"], false);
    assert_eq!(line["fields"]["kind"], "snmp");
    assert_eq!(line["kind"], "app");
    assert!(line["ts"].as_str().unwrap().ends_with('Z'));
}

#[test]
fn l_audit_est_marque_et_la_console_seule_masquee() {
    let records = capture(|| {
        tracing::info!(target: AUDIT_TARGET, action = "login", "audit");
        tracing::warn!(target: CONSOLE_ONLY_TARGET, "First-run setup code: ABCD-EFGH");
    });
    assert_eq!(records[0].kind(), "audit");
    assert!(!records[1].to_line().contains("ABCD-EFGH"));
}

#[test]
fn aucun_secret_ne_sort_d_un_evenement() {
    let records = capture(|| {
        tracing::warn!(
            error = %"error sending request for url (https://api.telegram.org/bot123456:AAH-secretpart/sendMessage)",
            password = "hunter2-very-secret",
            url = "https://admin:s3cr3t-pass@example.com/api?token=abcdef123456&x=1",
            "request failed with Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload"
        );
    });
    let line = records[0].to_line();
    for secret in [
        "AAH-secretpart",
        "hunter2-very-secret",
        "s3cr3t-pass",
        "abcdef123456",
        "eyJhbGciOiJIUzI1NiJ9",
    ] {
        assert!(!line.contains(secret), "{secret} présent dans {line}");
    }
    assert!(line.contains("example.com"));
}

#[test]
fn seuils_de_niveau() {
    assert!(Level::Error.passes(Level::Info));
    assert!(Level::Info.passes(Level::Info));
    assert!(!Level::Debug.passes(Level::Info));
    assert_eq!(Level::parse("WARNING"), Some(Level::Warn));
}
