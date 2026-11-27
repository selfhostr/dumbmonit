//! La migration 0245 active l'astreinte sur une instance qui s'en servait déjà
//! et la laisse coupée sur les autres, sans toucher au reste du réglage.

use dumbmonit_server::db;
use dumbmonit_server::instance;

async fn upgraded(with_schedule: bool, instance_row: Option<&str>) -> instance::InstanceSettings {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let path = dir.path().join(db::DATABASE_FILE);
    let old = db::open_up_to(&path, 244).await.expect("base ancienne");
    if with_schedule {
        sqlx::query("INSERT INTO oncall_schedules (name) VALUES ('Weekly')")
            .execute(&old)
            .await
            .expect("planning");
    }
    if let Some(json) = instance_row {
        sqlx::query("INSERT INTO settings (key, value) VALUES ('instance', ?)")
            .bind(json.as_bytes())
            .execute(&old)
            .await
            .expect("réglage d'instance");
    }
    old.close().await;

    let pool = db::open(&path).await.expect("migration");
    let value = instance::load(&pool).await;
    pool.close().await;
    value
}

#[tokio::test]
async fn an_instance_with_a_schedule_arrives_with_on_call_enabled() {
    let value =
        upgraded(true, Some(r#"{"setup_completed":true,"name":"Lab","language":"fr"}"#)).await;
    assert!(value.oncall_enabled);
    assert!(value.setup_completed);
    assert_eq!(value.name, "Lab");
    assert_eq!(value.language, "fr");
}

#[tokio::test]
async fn an_instance_without_any_schedule_keeps_it_off() {
    let value = upgraded(false, Some(r#"{"setup_completed":true,"name":"Lab"}"#)).await;
    assert!(!value.oncall_enabled);
    assert_eq!(value.name, "Lab");

    let fresh = upgraded(false, None).await;
    assert!(!fresh.oncall_enabled);
    assert!(!fresh.setup_completed, "a new database still gets its wizard");
}

#[tokio::test]
async fn a_schedule_without_an_instance_record_creates_it_enabled() {
    let value = upgraded(true, None).await;
    assert!(value.oncall_enabled);
    assert!(value.setup_completed);
}
