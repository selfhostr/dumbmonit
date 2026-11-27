//! Sauvegarde des rapports SLA personnalisés : gabarit, papier, sections,
//! titre, notes, logo et périmètre voyagent dans le lot, la page de statut par
//! son adresse et les moniteurs par leur référence, et reviennent à l'identique.

use dumbmonit_server::backup::{Bundle, ExportOptions, collect, restore};
use dumbmonit_server::db;
use dumbmonit_server::reports::layout::{Paper, Template};
use dumbmonit_server::reports::store;
use serde_json::json;

async fn instance() -> (sqlx::SqlitePool, dumbmonit_server::crypto::Cipher, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("clé");
    (pool, cipher, dir)
}

fn bundle() -> Bundle {
    serde_json::from_value(json!({
        "version": 1,
        "created_at": "2026-11-01 10:00:00",
        "targets": [
            { "kind": "dummy", "address": "10.0.0.1", "name": "Web", "interval_secs": 60,
              "enabled": true, "group_name": "Edge" }
        ],
        "status_pages": [
            { "slug": "acme", "title": "Acme", "published": false, "theme": "light",
              "show_uptime_days": 7 }
        ],
        "report_schedules": [
            { "name": "Board SLA", "enabled": false, "frequency": "monthly", "kind": "sla",
              "attach_csv": false, "objective": 99.9, "page": "acme",
              "layout": { "template": "summary", "paper": "letter", "title": "Board pack",
                          "notes": "Quiet month.", "show_logo": false,
                          "sections": { "chart": false },
                          "objectives": { "groups": { "Edge": 99.5 },
                                          "monitors": { "Web": 99.99 } } },
              "scope_targets": ["dummy|10.0.0.1", "dummy|fantome"],
              "scope_groups": ["Edge"] },
            { "name": "Weekly SLA", "enabled": false, "frequency": "weekly", "kind": "sla" },
            { "name": "Old digest", "enabled": false, "frequency": "weekly" }
        ]
    }))
    .expect("lot lisible")
}

#[tokio::test]
async fn the_layout_and_the_scope_of_a_report_survive_a_backup() {
    let (pool, cipher, _dir) = instance().await;
    let report = restore(&pool, &cipher, &bundle(), true).await.expect("restauration");
    let section =
        report.sections.iter().find(|s| s.section == "report_schedules").expect("section");
    assert_eq!(section.created, 2, "{section:?}");
    assert_eq!(section.skipped, 1, "an SLA report must be monthly");
    let notes = section.notes.join("\n");
    assert!(notes.contains("dummy|fantome"), "{notes}");

    let schedules = store::list(&pool).await.expect("liste");
    let board = schedules.iter().find(|s| s.name == "Board SLA").expect("Board SLA");
    assert_eq!(board.kind, "sla");
    assert!(!board.attach_csv && board.attach_pdf);
    assert_eq!(board.objective, Some(99.9));
    assert!(board.page_id.is_some(), "the status page is found by its slug");
    assert_eq!(board.layout.template, Template::Summary);
    assert_eq!(board.layout.paper, Paper::Letter);
    assert_eq!(board.layout.title.as_deref(), Some("Board pack"));
    assert_eq!(board.layout.notes, "Quiet month.");
    assert!(!board.layout.show_logo);
    assert!(!board.layout.sections.chart && board.layout.sections.incidents);
    assert_eq!(board.layout.objectives.groups.get("Edge"), Some(&99.5), "per-group objective");
    assert_eq!(board.layout.objectives.for_monitor("Web", "Edge"), Some(99.99));
    assert_eq!(board.scope.targets.len(), 1, "the unknown monitor is left out");
    assert_eq!(board.scope.groups, vec!["Edge".to_string()]);
    let old = schedules.iter().find(|s| s.name == "Old digest").expect("Old digest");
    assert_eq!(old.kind, "digest", "an old backup restores a digest");
    assert!(old.scope.is_empty());

    // Export : les mêmes réglages, les références à la place des identifiants.
    let exported = collect(&pool, &cipher, ExportOptions::default()).await.expect("export");
    let item = exported
        .report_schedules
        .iter()
        .find(|s| s.name == "Board SLA")
        .expect("exported Board SLA");
    assert_eq!(item.page.as_deref(), Some("acme"));
    assert_eq!(item.scope_targets, vec!["dummy|10.0.0.1".to_string()]);
    assert_eq!(item.layout, board.layout);

    // Relire le lot exporté ne change rien.
    let again = restore(&pool, &cipher, &exported, true).await.expect("relecture");
    let section = again.sections.iter().find(|s| s.section == "report_schedules").expect("section");
    assert_eq!((section.created, section.updated), (0, 0), "{section:?}");
}
