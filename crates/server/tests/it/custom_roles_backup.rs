//! Les rôles personnalisés et leurs attributions voyagent avec la sauvegarde.
//! Un compte créé par la restauration retrouve son rôle ; un compte qui
//! existait déjà n'est pas touché ; restaurer deux fois ne change rien ; une
//! permission inconnue est laissée de côté, pas fatale.

use dumbmonit_server::auth::custom_roles::{self, RoleInput};
use dumbmonit_server::auth::permissions::Permission;
use dumbmonit_server::auth::users::{self, AccountChanges, NewUser, Role};
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

async fn viewer(pool: &SqlitePool, username: &str) -> i64 {
    let new =
        NewUser { username, display_name: "", role: Role::Viewer, password_hash: None, oidc: None };
    users::insert(pool, new).await.unwrap().expect("compte")
}

async fn editors(pool: &SqlitePool) -> i64 {
    let input = RoleInput {
        name: "Monitor editors".into(),
        description: "Edit monitors, nothing else.".into(),
        permissions: vec!["monitors.manage".into(), "reports.view".into()],
    };
    let checked = input.check().expect("rôle valide");
    custom_roles::create(pool, &checked).await.unwrap().expect("rôle créé")
}

#[tokio::test]
async fn custom_roles_and_their_accounts_are_restored() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, source_cipher) = instance(&source_dir).await;
    let role = editors(&source).await;
    let jane = viewer(&source, "jane").await;
    let changes = AccountChanges { custom_role: Some(role), ..AccountChanges::default() };
    users::apply_changes(&source, jane, &changes).await.unwrap();
    // `bob` existe déjà sur l'instance restaurée : il ne prend pas le rôle.
    let bob = viewer(&source, "bob").await;
    users::apply_changes(&source, bob, &changes).await.unwrap();

    let jane_view = users::get(&source, jane).await.unwrap().expect("jane");
    assert_eq!(jane_view.role, Role::Viewer);
    let assigned = jane_view.custom_role.expect("rôle personnalisé");
    assert_eq!(assigned.name, "Monitor editors");
    assert_eq!(
        assigned.permissions,
        vec![Permission::View, Permission::MonitorsManage, Permission::ReportsView]
    );
    // Un rôle attribué ne se supprime pas.
    assert_eq!(
        custom_roles::delete(&source, role).await.unwrap(),
        Err(custom_roles::WriteError::InUse(2))
    );

    let lot = collect(&source, &source_cipher, ExportOptions::default()).await.expect("export");
    assert_eq!(lot.accounts.roles.len(), 1);
    assert_eq!(lot.accounts.role_assignments.len(), 2);
    assert_eq!(lot.summary().get("custom_roles"), Some(&1));

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    viewer(&pool, "bob").await;
    let dry = restore(&pool, &cipher, &lot, false).await.expect("simulation");
    assert_eq!(counts(&dry, "custom_roles"), (1, 0, 0));
    assert!(custom_roles::list(&pool).await.unwrap().is_empty(), "a dry run writes nothing");

    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "custom_roles"), (1, 0, 0));
    let restored = custom_roles::by_name(&pool, "monitor editors").await.unwrap().expect("rôle");
    assert_eq!(restored.description, "Edit monitors, nothing else.");
    assert_eq!(
        restored.permissions,
        vec![Permission::View, Permission::MonitorsManage, Permission::ReportsView]
    );
    let jane = users::by_username(&pool, "jane").await.unwrap().expect("jane");
    assert_eq!(jane.custom_role.map(|r| r.id), Some(restored.id));
    let bob = users::by_username(&pool, "bob").await.unwrap().expect("bob");
    assert!(bob.custom_role.is_none(), "an existing account is never changed by a file");

    let again = restore(&pool, &cipher, &lot, true).await.expect("seconde restauration");
    assert_eq!(counts(&again, "custom_roles"), (0, 0, 1));
}

#[tokio::test]
async fn unknown_permissions_and_bad_names_are_reported_not_fatal() {
    let lot: Bundle = serde_json::from_value(serde_json::json!({
        "version": 1,
        "created_at": "2026-11-01 00:00:00",
        "accounts": {
            "roles": [
                { "name": "Pagers", "permissions": ["alerts.operate", "time.travel"] },
                { "name": "admin", "permissions": ["view"] }
            ],
            "role_assignments": [ { "user": "ghost", "role": "Pagers" } ]
        }
    }))
    .expect("lot");
    let dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&dir).await;
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "custom_roles"), (1, 0, 1));
    let section = report.sections.iter().find(|s| s.section == "custom_roles").unwrap();
    assert_eq!(section.notes.len(), 2, "{:?}", section.notes);
    let pagers = custom_roles::by_name(&pool, "Pagers").await.unwrap().expect("rôle");
    assert_eq!(pagers.permissions, vec![Permission::View, Permission::AlertsOperate]);
}
