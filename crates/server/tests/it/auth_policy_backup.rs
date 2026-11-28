//! La politique d'authentification et l'adresse électronique des comptes
//! voyagent avec la sauvegarde. Un lot d'avant ces champs n'écrase rien, et
//! une valeur illisible est signalée au lieu de faire échouer la restauration.

use dumbmonit_server::auth::policy::{self, Policy, SETTINGS_KEY};
use dumbmonit_server::auth::users::{self, NewUser, Role};
use dumbmonit_server::backup::bundle::BundleUser;
use dumbmonit_server::backup::{Bundle, ExportOptions, RestoreReport, collect, restore};
use dumbmonit_server::db;
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

async fn stored(pool: &SqlitePool) -> bool {
    sqlx::query("SELECT 1 FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(pool)
        .await
        .expect("lecture")
        .is_some()
}

#[tokio::test]
async fn la_politique_est_sauvegardee_et_restauree() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, source_cipher) = instance(&source_dir).await;
    let wanted = Policy { require_totp: true };
    policy::save(&source, &wanted).await.unwrap();

    let lot = collect(&source, &source_cipher, ExportOptions::default()).await.expect("export");
    assert!(lot.auth_policy.is_some());

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    let dry = restore(&pool, &cipher, &lot, false).await.expect("simulation");
    assert_eq!(counts(&dry, "auth_policy"), (1, 0, 0));
    assert!(!stored(&pool).await, "une simulation n'écrit rien");

    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "auth_policy"), (1, 0, 0));
    assert_eq!(policy::load(&pool).await.unwrap(), wanted);

    let again = restore(&pool, &cipher, &lot, true).await.expect("seconde restauration");
    assert_eq!(counts(&again, "auth_policy"), (0, 0, 1), "rien ne change");

    // Coupée dans le lot, elle se coupe ici aussi.
    policy::save(&source, &Policy::default()).await.unwrap();
    let lot = collect(&source, &source_cipher, ExportOptions::default()).await.unwrap();
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "auth_policy"), (0, 1, 0));
    assert!(!policy::load(&pool).await.unwrap().require_totp);
}

#[tokio::test]
async fn un_lot_sans_politique_laisse_celle_de_l_instance() {
    let lot: Bundle =
        serde_json::from_str(r#"{ "version": 1, "created_at": "2026-10-01T00:00:00Z" }"#).unwrap();
    assert!(lot.auth_policy.is_none());

    let dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&dir).await;
    policy::save(&pool, &Policy { require_totp: true }).await.unwrap();
    restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert!(policy::load(&pool).await.unwrap().require_totp);
}

#[tokio::test]
async fn une_politique_illisible_est_signalee_et_ecartee() {
    let lot: Bundle = serde_json::from_value(serde_json::json!({
        "version": 1,
        "created_at": "2026-10-01T00:00:00Z",
        "auth_policy": { "require_totp": "oui" },
    }))
    .expect("lot");
    let dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&dir).await;
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "auth_policy"), (0, 0, 1));
    assert!(!stored(&pool).await);
}

#[tokio::test]
async fn l_adresse_des_comptes_voyage_avec_eux() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, cipher) = instance(&source_dir).await;
    let new = NewUser {
        username: "jane",
        display_name: "Jane",
        role: Role::Operator,
        password_hash: None,
        oidc: None,
    };
    let id = users::insert(&source, new).await.unwrap().expect("compte");
    users::set_email(&source, id, Some("jane@example.org")).await.unwrap();

    let mut lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    let exported = lot.users.iter().find(|u| u.username == "jane").expect("compte exporté");
    assert_eq!(exported.email.as_deref(), Some("jane@example.org"));

    // Une adresse invalide (lot édité à la main) est écartée, pas le compte.
    let paul = BundleUser {
        username: "paul".into(),
        email: Some("pas une adresse".into()),
        ..exported.clone()
    };
    lot.users.push(paul);

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "users").0, 2);
    let jane = users::by_username(&pool, "jane").await.unwrap().expect("jane");
    assert_eq!(jane.email.as_deref(), Some("jane@example.org"));
    let paul = users::by_username(&pool, "paul").await.unwrap().expect("paul");
    assert_eq!(paul.email, None);
}
