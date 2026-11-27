//! Meraki, PAN-OS, SonicWall et UniFi Protect sont devenus des paquets
//! communautaires : leurs cibles prennent le type du paquet sans perdre leur
//! identifiant chiffré ni leurs réglages, et disent quel paquet installer.

use dumbmonit_server::{db, packs};
use sqlx::Row;

async fn target(pool: &sqlx::SqlitePool, kind: &str, address: &str, tags: &str) -> i64 {
    sqlx::query(
        "INSERT INTO targets (name, address, kind, tags, credential_enc) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(format!("{kind} {address}"))
    .bind(address)
    .bind(kind)
    .bind(tags)
    .bind(vec![1u8, 2, 3, 4])
    .execute(pool)
    .await
    .expect("cible")
    .last_insert_rowid()
}

async fn builtin_rule(pool: &sqlx::SqlitePool, uid: &str, builtin: bool) {
    sqlx::query(
        "INSERT INTO alert_rules (uid, name, kind, query, operator, severity, builtin)
         VALUES (?, ?, 'threshold', 'dumbmonit_x', '>', 'warning', ?)",
    )
    .bind(uid)
    .bind(uid)
    .bind(i64::from(builtin))
    .execute(pool)
    .await
    .expect("règle");
}

#[tokio::test]
async fn les_cibles_des_types_devenus_paquets_prennent_le_type_du_paquet() {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let pool = db::open(&dir.path().join(db::DATABASE_FILE)).await.expect("base");

    let meraki = target(
        &pool,
        "meraki",
        "api.meraki.com",
        r#"{"organization":"549236","networks":"Main office","request_timeout_seconds":"20"}"#,
    )
    .await;
    let protect = target(&pool, "unifiprotect", "unvr.lan", "{}").await;
    let other = target(&pool, "omada", "omada.lan", "{}").await;
    // Une cible du paquet existe déjà à cette adresse : l'ancienne ne peut pas
    // prendre son type, elle reste et son erreur le dira.
    target(&pool, "pack.sonicwall", "fw.lan", "{}").await;
    let clash = target(&pool, "sonicwall", "fw.lan", "{}").await;
    builtin_rule(&pool, "meraki_device_offline", true).await;
    builtin_rule(&pool, "paloalto_cpu_high", true).await;
    builtin_rule(&pool, "pack:meraki:device_offline", false).await;
    sqlx::query("INSERT INTO rule_overrides (rule_uid, target_id, threshold) VALUES (?, ?, 5)")
        .bind("meraki_device_offline")
        .bind(meraki)
        .execute(&pool)
        .await
        .expect("seuil");

    assert_eq!(packs::convert_moved_kinds(&pool).await.expect("conversion"), 2);

    let row = |id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query("SELECT kind, tags, credential_enc, last_error FROM targets WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("cible relue")
        }
    };
    let converted = row(meraki).await;
    assert_eq!(converted.get::<String, _>("kind"), "pack.meraki");
    assert_eq!(converted.get::<Vec<u8>, _>("credential_enc"), [1, 2, 3, 4]);
    let tags: serde_json::Value =
        serde_json::from_str(&converted.get::<String, _>("tags")).expect("étiquettes");
    assert_eq!(
        tags,
        serde_json::json!({"organization": "549236", "request_timeout_seconds": "20"}),
        "le filtre de réseaux n'existe plus, le reste est gardé"
    );
    let error = converted.get::<Option<String>, _>("last_error").expect("message");
    assert!(error.contains("Install the community pack \"meraki\""), "{error}");

    assert_eq!(row(protect).await.get::<String, _>("kind"), "pack.unifiprotect");
    assert_eq!(row(other).await.get::<String, _>("kind"), "omada");
    assert_eq!(row(clash).await.get::<String, _>("kind"), "sonicwall");

    let uids: Vec<String> = sqlx::query("SELECT uid FROM alert_rules ORDER BY uid")
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get("uid"))
        .collect();
    assert_eq!(
        uids,
        ["pack:meraki:device_offline"],
        "les règles livrées des anciens types partent"
    );
    let overrides: i64 = sqlx::query("SELECT COUNT(*) AS n FROM rule_overrides")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("n");
    assert_eq!(overrides, 0);

    // Une seconde passe ne trouve plus rien.
    assert_eq!(packs::convert_moved_kinds(&pool).await.expect("seconde passe"), 0);
}
