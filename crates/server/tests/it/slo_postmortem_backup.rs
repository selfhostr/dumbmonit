//! Sauvegarde des SLO (migration 0260) et des post-mortems d'incident
//! (migration 0261) : ils partent dans le lot, reviennent à l'identique sur
//! une instance neuve (le périmètre retrouvé par référence d'équipement), et
//! relire le même lot ne change rien. La règle livrée de consommation des
//! budgets (migration 0262) s'installe malgré la contrainte sur `kind`.

use dumbmonit_server::backup::{ExportOptions, collect, restore};
use dumbmonit_server::db;
use dumbmonit_server::db::status_pages::{
    ActionItem, IncidentInput, PostmortemInput, TimelineEntry,
};
use dumbmonit_server::reports::layout::Scope;
use dumbmonit_server::slo::{self, SloInput};

async fn instance() -> (sqlx::SqlitePool, dumbmonit_server::crypto::Cipher, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("clé");
    (pool, cipher, dir)
}

async fn target(pool: &sqlx::SqlitePool, name: &str, address: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO targets (name, address, kind, group_name) VALUES (?, ?, 'dummy', 'Web')
         RETURNING id",
    )
    .bind(name)
    .bind(address)
    .fetch_one(pool)
    .await
    .expect("équipement")
}

#[tokio::test]
async fn slos_and_postmortems_survive_a_backup() {
    let (source, cipher, _dir) = instance().await;
    // Un premier équipement décale les identifiants : la restauration doit
    // retrouver le bon par sa référence, pas par son numéro.
    target(&source, "Spare", "10.0.0.9").await;
    let api = target(&source, "API", "10.0.0.2").await;
    slo::store::insert(
        &source,
        &SloInput {
            name: "API availability".to_string(),
            description: "Public API".to_string(),
            objective: 99.9,
            window_days: 28,
            scope: Scope { targets: vec![api], groups: vec!["Web".to_string()] },
            enabled: true,
        },
    )
    .await
    .expect("SLO");

    let incident = db::status_pages::create_incident(
        &source,
        &IncidentInput {
            page_id: None,
            title: "API down".to_string(),
            kind: "incident".to_string(),
            status: "resolved".to_string(),
            severity: "major".to_string(),
            starts_at: "2026-11-01T10:00:00Z".to_string(),
            ends_at: Some("2026-11-01T11:00:00Z".to_string()),
        },
    )
    .await
    .expect("incident");
    let postmortem = PostmortemInput {
        summary: "The API was down for an hour.".to_string(),
        impact: "Every client.".to_string(),
        root_cause: "An expired certificate.".to_string(),
        timeline: vec![TimelineEntry {
            at: "2026-11-01T10:00:00Z".to_string(),
            text: "Alert fired.".to_string(),
        }],
        actions: vec![ActionItem { text: "Watch certificate expiry".to_string(), done: true }],
        published: true,
    };
    db::status_pages::save_postmortem(&source, incident, &postmortem).await.expect("post-mortem");

    let exported = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    assert_eq!(exported.slos.len(), 1);
    assert_eq!(exported.slos[0].scope_targets, vec!["dummy|10.0.0.2".to_string()]);
    assert_eq!(exported.summary().get("slos"), Some(&1));
    let carried = exported.incidents[0].postmortem.clone().expect("post-mortem in the backup");
    assert!(carried.published && carried.published_at.is_some());

    let (target_pool, cipher, _other) = instance().await;
    let new_api = target(&target_pool, "API", "10.0.0.2").await;
    let report = restore(&target_pool, &cipher, &exported, true).await.expect("restauration");
    let section = report.sections.iter().find(|s| s.section == "slos").expect("section");
    assert_eq!(section.created, 1, "{section:?}");

    let restored = slo::store::list(&target_pool).await.expect("lecture");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].name, "API availability");
    assert_eq!(restored[0].objective, 99.9);
    assert_eq!(restored[0].window_days, 28);
    assert_eq!(restored[0].scope.targets, vec![new_api]);
    assert_eq!(restored[0].scope.groups, vec!["Web".to_string()]);

    let incidents = db::status_pages::list_incidents(&target_pool).await.expect("incidents");
    let back = db::status_pages::get_postmortem(&target_pool, incidents[0].id)
        .await
        .expect("lecture")
        .expect("post-mortem restored");
    assert_eq!(back.summary, postmortem.summary);
    assert_eq!(back.timeline, postmortem.timeline);
    assert_eq!(back.actions, postmortem.actions);
    assert!(back.published);
    assert_eq!(back.published_at, carried.published_at, "the publication date is kept");

    // Relire le même lot ne change rien.
    let again = restore(&target_pool, &cipher, &exported, true).await.expect("relecture");
    let section = again.sections.iter().find(|s| s.section == "slos").expect("section");
    assert_eq!((section.created, section.updated, section.skipped), (0, 0, 1), "{section:?}");
    let section = again.sections.iter().find(|s| s.section == "incidents").expect("section");
    assert_eq!((section.created, section.updated), (0, 0), "{section:?}");
}

#[tokio::test]
async fn the_burn_rate_rule_is_installed() {
    let (pool, _cipher, _dir) = instance().await;
    db::alerts::seed_builtin_rules(&pool).await.expect("règles livrées");
    let kind: String = sqlx::query_scalar("SELECT kind FROM alert_rules WHERE uid = ?")
        .bind(slo::RULE_SLO_BURN)
        .fetch_one(&pool)
        .await
        .expect("the SLO burn rule is in the base");
    assert_eq!(kind, "slo_burn");
}
