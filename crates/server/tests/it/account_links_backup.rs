//! Les réglages des liens de compte, les invitations ouvertes et les
//! notifications personnelles voyagent avec la sauvegarde. Le lien d'une
//! invitation déjà envoyé fonctionne encore après restauration ; restaurer
//! deux fois ne change rien ; une entrée illisible est signalée, pas fatale.

use chrono::Utc;
use dumbmonit_server::alerting::model::Severity;
use dumbmonit_server::auth::invitations::{self, NewInvitation};
use dumbmonit_server::auth::recovery::{self, RecoverySettings};
use dumbmonit_server::auth::teams::{self, TeamInput};
use dumbmonit_server::auth::users::{self, NewUser, Role};
use dumbmonit_server::backup::{Bundle, ExportOptions, RestoreReport, collect, restore};
use dumbmonit_server::db;
use dumbmonit_server::notify::personal::{self, Prefs, Scope};
use sqlx::SqlitePool;

async fn instance(dir: &tempfile::TempDir) -> (SqlitePool, dumbmonit_server::crypto::Cipher) {
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("clé");
    (pool, cipher)
}

fn counts(report: &RestoreReport, name: &str) -> (usize, usize, usize) {
    let section = report.sections.iter().find(|s| s.section == name).expect("section");
    (section.created, section.updated, section.skipped)
}

fn chosen() -> Prefs {
    Prefs {
        email: true,
        webpush: true,
        min_severity: Severity::Critical,
        scope: Scope::Pick,
        folders: vec!["Paris".into()],
        tags: vec!["room=cave".into()],
        min_interval_secs: 900,
        ..Prefs::default()
    }
}

#[tokio::test]
async fn account_links_and_personal_notifications_are_restored() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, source_cipher) = instance(&source_dir).await;
    let new = NewUser {
        username: "jane",
        display_name: "Jane",
        role: Role::Operator,
        password_hash: None,
        oidc: None,
    };
    let jane = users::insert(&source, new).await.unwrap().expect("compte");
    let input = TeamInput { name: Some("NOC".into()), ..TeamInput::default() };
    let noc = teams::create(&source, &input).await.unwrap();
    personal::save(&source, jane, &chosen()).await.unwrap();
    let settings = RecoverySettings { self_service: false, reset_minutes: 60, invite_hours: 48 };
    recovery::save(&source, &settings).await.unwrap();
    let invite = NewInvitation {
        email: "ada@example.org",
        role: Role::Viewer,
        teams: &[noc],
        invited_by: "admin",
        hours: 48,
    };
    let (_, token) = invitations::create(&source, &invite, Utc::now()).await.unwrap();
    // Une invitation retirée ne part pas.
    let gone = NewInvitation { email: "old@example.org", ..invite };
    let (revoked, _) = invitations::create(&source, &gone, Utc::now()).await.unwrap();
    invitations::revoke(&source, revoked.id, Utc::now()).await.unwrap();

    let lot = collect(&source, &source_cipher, ExportOptions::default()).await.expect("export");
    assert_eq!(lot.accounts.invitations.len(), 1);
    assert_eq!(lot.accounts.invitations[0].teams, ["NOC"]);
    assert_eq!(lot.accounts.notification_prefs.len(), 1);
    assert!(lot.accounts.recovery.is_some());
    let text = serde_json::to_string(&lot).unwrap();
    let value = token.value();
    let secret = value.split_once('.').unwrap().1;
    assert!(!text.contains(secret), "the link secret never travels");

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    let dry = restore(&pool, &cipher, &lot, false).await.expect("simulation");
    assert_eq!(counts(&dry, "invitations"), (1, 0, 0));
    assert_eq!(counts(&dry, "notification_prefs"), (1, 0, 0));
    assert_eq!(counts(&dry, "account_links"), (1, 0, 0));
    assert!(invitations::list_open(&pool).await.unwrap().is_empty(), "a dry run writes nothing");

    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "invitations"), (1, 0, 0));
    assert_eq!(recovery::load(&pool).await.unwrap(), settings);
    let jane = users::by_username(&pool, "jane").await.unwrap().expect("jane");
    assert_eq!(personal::load(&pool, jane.id).await.unwrap(), chosen());
    // Le lien envoyé avant la sauvegarde ouvre toujours l'invitation, et ses
    // équipes sont celles de l'instance restaurée.
    let open = invitations::find_open(&pool, &token, Utc::now()).await.unwrap().expect("open");
    assert_eq!(open.email, "ada@example.org");
    let restored_noc = teams::list(&pool).await.unwrap();
    assert_eq!(open.teams, [restored_noc[0].id]);

    let again = restore(&pool, &cipher, &lot, true).await.expect("seconde restauration");
    assert_eq!(counts(&again, "invitations"), (0, 0, 1));
    assert_eq!(counts(&again, "notification_prefs"), (0, 0, 1));
    assert_eq!(counts(&again, "account_links"), (0, 0, 1));
}

#[tokio::test]
async fn unreadable_or_orphan_entries_are_reported_not_fatal() {
    let lot: Bundle = serde_json::from_value(serde_json::json!({
        "version": 1,
        "created_at": "2026-11-01 00:00:00",
        "accounts": {
            "recovery": { "reset_minutes": 1 },
            "invitations": [
                { "email": "x@example.org", "role": "wall", "created_at": "2026-11-01 00:00:00",
                  "expires_at": "2999-01-01 00:00:00", "token_id": "0".repeat(32),
                  "token_hash": "AAAA" },
                { "email": "y@example.org", "role": "viewer", "created_at": "2026-11-01 00:00:00",
                  "expires_at": "2001-01-01 00:00:00", "token_id": "1".repeat(32),
                  "token_hash": "MTExMTExMTExMTExMTExMTExMTExMTExMTExMTExMTE=" }
            ],
            "notification_prefs": [
                { "user": "ghost", "prefs": { "email": true } }
            ]
        }
    }))
    .expect("lot");
    let dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&dir).await;
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "account_links"), (0, 0, 1));
    assert_eq!(counts(&report, "invitations"), (0, 0, 2));
    assert_eq!(counts(&report, "notification_prefs"), (0, 0, 1));
    assert!(report.sections.iter().any(|s| s.section == "invitations" && s.notes.len() == 2));
    assert_eq!(recovery::load(&pool).await.unwrap(), RecoverySettings::default());

    // Un lot d'avant cette section ne touche à rien.
    let old: Bundle =
        serde_json::from_str(r#"{ "version": 1, "created_at": "2026-10-01T00:00:00Z" }"#).unwrap();
    assert!(old.accounts.is_empty());
    let report = restore(&pool, &cipher, &old, true).await.expect("restauration");
    assert_eq!(counts(&report, "account_links"), (0, 0, 0));
}
