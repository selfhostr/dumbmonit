//! Réglages de l'instance : sauvegardes, mur, messagerie, agents.

use super::*;

/// Planification des sauvegardes locales : remplacée par celle du lot, après
/// contrôle et mise dans les bornes. Un lot sans section ne touche à rien.
pub(super) async fn restore_backup_schedule(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<SectionReport> {
    use crate::backup::local::{SETTINGS_KEY, ScheduleOverride};
    let mut report = SectionReport::new("backup_schedule");
    let Some(raw) = &bundle.backup_schedule else { return Ok(report) };
    let Ok(stored) = serde_json::from_value::<ScheduleOverride>(raw.clone()) else {
        report.skipped += 1;
        report.notes.push("The backup schedule of the backup is unreadable".into());
        return Ok(report);
    };
    let value = serde_json::to_vec(&stored.clamped())?;
    let existing = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture de la planification des sauvegardes")?;
    if let Some(row) = &existing
        && row.try_get::<Vec<u8>, _>("value")? == value
    {
        report.skipped += 1;
        return Ok(report);
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(SETTINGS_KEY)
    .bind(&value)
    .execute(&mut **tx)
    .await
    .context("écriture de la planification des sauvegardes")?;
    report.count(if existing.is_some() {
        RestoreOutcome::Updated
    } else {
        RestoreOutcome::Created
    });
    Ok(report)
}

/// Météo du mur (lieu et unité) : remplacée par celle du lot, après contrôle.
/// Un lot sans section ne touche pas au réglage de l'instance.
pub(super) async fn restore_wall_weather(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<SectionReport> {
    use crate::weather::{SETTINGS_KEY, Settings};
    let mut report = SectionReport::new("wall_weather");
    let Some(raw) = &bundle.wall_weather else { return Ok(report) };
    let settings = match serde_json::from_value::<Settings>(raw.clone()) {
        Ok(settings) if settings.place.as_ref().is_none_or(|p| p.validate().is_ok()) => settings,
        _ => {
            report.skipped += 1;
            report.notes.push("The wall weather settings of the backup are unreadable".into());
            return Ok(report);
        }
    };
    let value = serde_json::to_vec(&settings)?;
    let existing = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture de la météo du mur")?;
    if let Some(row) = &existing
        && row.try_get::<Vec<u8>, _>("value")? == value
    {
        report.skipped += 1;
        return Ok(report);
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(SETTINGS_KEY)
    .bind(&value)
    .execute(&mut **tx)
    .await
    .context("écriture de la météo du mur")?;
    report.count(if existing.is_some() {
        RestoreOutcome::Updated
    } else {
        RestoreOutcome::Created
    });
    Ok(report)
}

/// Serveur de courriel de l'instance : remplacé par celui du lot, mot de
/// passe compris (rechiffré par la clé de cette instance). Un lot sans section
/// ne touche pas au réglage ; un réglage illisible est signalé et laissé de côté.
pub(super) async fn restore_mail_server(
    tx: &mut Transaction<'_, Sqlite>,
    cipher: &Cipher,
    bundle: &Bundle,
) -> Result<SectionReport> {
    use crate::notify::mail_server::{PASSWORD_KEY, SETTINGS_KEY, check_password};
    let mut report = SectionReport::new("mail_server");
    let Some(saved) = &bundle.mail_server else { return Ok(report) };
    let server = saved.server.clone().trimmed();
    let password = saved.password.as_deref().filter(|password| !password.is_empty());
    if !server.is_configured()
        || server.validate().is_err()
        || password.is_some_and(|password| check_password(password).is_err())
    {
        report.skipped += 1;
        report.notes.push("The email server settings of the backup are unreadable".into());
        return Ok(report);
    }
    let value = serde_json::to_vec(&server)?;
    let existing: Option<Vec<u8>> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture du serveur de courriel")?;
    let sealed: Option<Vec<u8>> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(PASSWORD_KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture du mot de passe du serveur de courriel")?;
    let stored_password = sealed.and_then(|sealed| cipher.decrypt(&sealed).ok());
    if existing.as_deref() == Some(value.as_slice())
        && stored_password.as_deref() == password.map(str::as_bytes)
    {
        report.skipped += 1;
        return Ok(report);
    }
    let write = "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')";
    sqlx::query(write)
        .bind(SETTINGS_KEY)
        .bind(&value)
        .execute(&mut **tx)
        .await
        .context("écriture du serveur de courriel")?;
    match password {
        Some(password) => {
            sqlx::query(write)
                .bind(PASSWORD_KEY)
                .bind(cipher.encrypt(password.as_bytes())?)
                .execute(&mut **tx)
                .await
                .context("écriture du mot de passe du serveur de courriel")?;
        }
        None => {
            sqlx::query("DELETE FROM settings WHERE key = ?")
                .bind(PASSWORD_KEY)
                .execute(&mut **tx)
                .await
                .context("effacement du mot de passe du serveur de courriel")?;
        }
    }
    report.count(if existing.is_some() {
        RestoreOutcome::Updated
    } else {
        RestoreOutcome::Created
    });
    Ok(report)
}

/// Garde de l'enrôlement des agents (validation, débit, mise en veille) :
/// remplacée par celle du lot, après contrôle. Un réglage illisible est
/// signalé et laissé de côté, l'instance garde le sien.
///
/// Un lot d'avant la garde n'en a pas : s'il ramène des jetons d'agent et que
/// l'instance n'a encore rien réglé, elle prend l'acceptation automatique,
/// comme l'a fait la migration 0233 sur l'instance d'origine. Sans cela, tout
/// le parc restauré attendrait une validation.
pub(super) async fn restore_agent_enrolment(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<SectionReport> {
    use crate::collectors::agent::enrolment::{EnrolmentSettings, SETTINGS_KEY};
    let mut report = SectionReport::new("agent_enrolment");
    let existing = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture de la garde de l'enrôlement")?;
    let settings = match &bundle.agent_enrolment {
        Some(raw) => match serde_json::from_value::<EnrolmentSettings>(raw.clone()) {
            Ok(settings) if settings.validate().is_ok() => settings,
            _ => {
                report.skipped += 1;
                let note = "The agent enrolment settings of the backup are unreadable";
                report.notes.push(note.into());
                return Ok(report);
            }
        },
        None if !bundle.agent_tokens.is_empty() && existing.is_none() => {
            EnrolmentSettings::legacy()
        }
        None => return Ok(report),
    };
    let value = serde_json::to_vec(&settings)?;
    if let Some(row) = &existing
        && row.try_get::<Vec<u8>, _>("value")? == value
    {
        report.skipped += 1;
        return Ok(report);
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(SETTINGS_KEY)
    .bind(&value)
    .execute(&mut **tx)
    .await
    .context("écriture de la garde de l'enrôlement")?;
    report.count(if existing.is_some() {
        RestoreOutcome::Updated
    } else {
        RestoreOutcome::Created
    });
    Ok(report)
}

/// Déploiement par vagues des mises à jour des agents : remplacé par celui du
/// lot, après contrôle. Un réglage illisible ou invalide est signalé et laissé
/// de côté, l'instance garde le sien ; un lot d'avant cette section n'y touche pas.
pub(super) async fn restore_agent_update_rollout(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<SectionReport> {
    use crate::collectors::agent::rollout::{Rollout, SETTINGS_KEY};
    let mut report = SectionReport::new("agent_update_rollout");
    let Some(raw) = &bundle.agent_update_rollout else { return Ok(report) };
    let rollout = match serde_json::from_value::<Rollout>(raw.clone()) {
        Ok(rollout) if rollout.validate().is_ok() => rollout,
        _ => {
            report.skipped += 1;
            let note = "The agent update rollout settings of the backup are unreadable";
            report.notes.push(note.into());
            return Ok(report);
        }
    };
    let existing = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture du déploiement par vagues des agents")?;
    let value = serde_json::to_vec(&rollout)?;
    match &existing {
        Some(row) if row.try_get::<Vec<u8>, _>("value")? == value => {
            report.skipped += 1;
            return Ok(report);
        }
        // Rien de rangé et la politique par défaut : rien à restaurer.
        None if rollout == Rollout::default() => {
            report.skipped += 1;
            return Ok(report);
        }
        _ => {}
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(SETTINGS_KEY)
    .bind(&value)
    .execute(&mut **tx)
    .await
    .context("écriture du déploiement par vagues des agents")?;
    report.count(if existing.is_some() {
        RestoreOutcome::Updated
    } else {
        RestoreOutcome::Created
    });
    Ok(report)
}

/// Réglages d'instance (`crate::instance`). L'assistant de démarrage reste
/// terminé s'il l'était ici : restaurer une instance n'oblige pas à le refaire,
/// et un lot d'une instance déjà installée le marque terminé.
pub(super) async fn restore_instance(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<SectionReport> {
    let mut report = SectionReport::new("instance");
    let Some(incoming) = &bundle.instance else { return Ok(report) };

    let existing = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(crate::instance::KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture des réglages d'instance")?;
    let current: Option<crate::instance::InstanceSettings> = match &existing {
        Some(row) => serde_json::from_slice(&row.try_get::<Vec<u8>, _>("value")?).ok(),
        None => None,
    };

    let mut next = incoming.clone();
    // Un lot ne fait pas entrer une couleur qui n'en est pas une.
    if !crate::instance::is_valid_accent(&next.accent) {
        next.accent.clear();
    }
    if let Some(current) = &current
        && current.setup_completed
        && !next.setup_completed
    {
        next.setup_completed = true;
        next.setup_completed_at.clone_from(&current.setup_completed_at);
    }
    if current.as_ref() == Some(&next) {
        report.skipped += 1;
        return Ok(report);
    }

    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(crate::instance::KEY)
    .bind(serde_json::to_vec(&next)?)
    .execute(&mut **tx)
    .await
    .context("écriture des réglages d'instance")?;
    report.count(if existing.is_some() {
        RestoreOutcome::Updated
    } else {
        RestoreOutcome::Created
    });
    Ok(report)
}

/// Modules facultatifs (`crate::features`) : ceux du lot remplacent ceux de
/// l'instance. Un lot d'avant cette section ne touche à rien.
pub(super) async fn restore_features(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<SectionReport> {
    let mut report = SectionReport::new("features");
    let Some(incoming) = &bundle.features else { return Ok(report) };

    let existing = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(crate::features::KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture des modules facultatifs")?;
    let current: Option<crate::features::Features> = match &existing {
        Some(row) => serde_json::from_slice(&row.try_get::<Vec<u8>, _>("value")?).ok(),
        None => None,
    };
    if current.as_ref() == Some(incoming) {
        report.skipped += 1;
        return Ok(report);
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(crate::features::KEY)
    .bind(serde_json::to_vec(incoming)?)
    .execute(&mut **tx)
    .await
    .context("écriture des modules facultatifs")?;
    report.count(if existing.is_some() {
        RestoreOutcome::Updated
    } else {
        RestoreOutcome::Created
    });
    Ok(report)
}

/// Lot d'avant les modules facultatifs (sans section `features`) : chaque module
/// dont les données viennent d'être restaurées est rallumé, comme le fait la
/// migration 0309 pour une instance existante. Rien n'est jamais éteint ici.
pub(super) async fn enable_restored_features(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<()> {
    if bundle.features.is_some() {
        return Ok(());
    }
    let existing = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(crate::features::KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture des modules facultatifs")?;
    let mut features: crate::features::Features = match &existing {
        Some(row) => {
            serde_json::from_slice(&row.try_get::<Vec<u8>, _>("value")?).unwrap_or_default()
        }
        None => crate::features::Features::default(),
    };
    let before = features;
    let used = sqlx::query(
        "SELECT EXISTS (SELECT 1 FROM users WHERE role = 'wall') AS wall,
                EXISTS (SELECT 1 FROM report_schedules) AS sla_reports,
                EXISTS (SELECT 1 FROM status_pages) AS status_pages,
                EXISTS (SELECT 1 FROM slos) AS slo",
    )
    .fetch_one(&mut **tx)
    .await
    .context("modules utilisés")?;
    features.wall |= used.try_get::<i64, _>("wall")? != 0;
    features.sla_reports |= used.try_get::<i64, _>("sla_reports")? != 0;
    features.status_pages |= used.try_get::<i64, _>("status_pages")? != 0;
    features.slo |= used.try_get::<i64, _>("slo")? != 0;
    if existing.is_some() && features == before {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(crate::features::KEY)
    .bind(serde_json::to_vec(&features)?)
    .execute(&mut **tx)
    .await
    .context("écriture des modules facultatifs")?;
    Ok(())
}

/// Politique d'authentification (`auth::policy`) : remplacée par celle du lot.
/// Un réglage illisible est signalé et laissé de côté ; un lot d'avant cette
/// section, ou qui n'en a pas, ne touche pas à celle de l'instance.
pub(super) async fn restore_auth_policy(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<SectionReport> {
    use crate::auth::policy::{Policy, SETTINGS_KEY};
    let mut report = SectionReport::new("auth_policy");
    let Some(raw) = &bundle.auth_policy else { return Ok(report) };
    let Ok(policy) = serde_json::from_value::<Policy>(raw.clone()) else {
        report.skipped += 1;
        report.notes.push("The authentication policy of the backup is unreadable".into());
        return Ok(report);
    };
    let value = serde_json::to_vec(&policy)?;
    let existing = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture de la politique d'authentification")?;
    match &existing {
        Some(row) if row.try_get::<Vec<u8>, _>("value")? == value => {
            report.skipped += 1;
            return Ok(report);
        }
        // Rien de rangé et la politique par défaut : rien à restaurer.
        None if policy == Policy::default() => {
            report.skipped += 1;
            return Ok(report);
        }
        _ => {}
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(SETTINGS_KEY)
    .bind(&value)
    .execute(&mut **tx)
    .await
    .context("écriture de la politique d'authentification")?;
    report.count(if existing.is_some() {
        RestoreOutcome::Updated
    } else {
        RestoreOutcome::Created
    });
    Ok(report)
}

/// Adresse du registre de paquets : remplacée par celle du lot si elle est lisible.
pub(super) async fn restore_pack_registry(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<SectionReport> {
    use crate::packs::registry::{SETTINGS_KEY, Settings, validate_url};
    let mut report = SectionReport::new("pack_registry");
    let Some(raw) = &bundle.pack_registry else { return Ok(report) };
    let settings = match serde_json::from_value::<Settings>(raw.clone()) {
        Ok(settings) if validate_url(&settings.url).is_ok() => settings,
        _ => {
            report.skipped += 1;
            report.notes.push("The pack registry address of the backup is unreadable".into());
            return Ok(report);
        }
    };
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(SETTINGS_KEY)
    .bind(serde_json::to_vec(&settings)?)
    .execute(&mut **tx)
    .await
    .context("réglage du registre de paquets")?;
    report.updated += 1;
    Ok(report)
}

/// Récepteur de flux réseau (`flows`) : remplacé par celui du lot. Même règle
/// que la politique d'authentification : illisible, il est signalé et laissé
/// de côté ; absent du lot, celui de l'instance ne bouge pas. Le récepteur en
/// service prend le réglage restauré au prochain démarrage.
pub(super) async fn restore_flows(
    tx: &mut Transaction<'_, Sqlite>,
    bundle: &Bundle,
) -> Result<SectionReport> {
    use crate::flows::{SETTINGS_KEY, Settings};
    let mut report = SectionReport::new("flows");
    let Some(raw) = &bundle.flows else { return Ok(report) };
    let Some(settings) =
        serde_json::from_value::<Settings>(raw.clone()).ok().filter(|s| s.validate().is_ok())
    else {
        report.skipped += 1;
        report.notes.push("The flow receiver settings of the backup are unreadable".into());
        return Ok(report);
    };
    let value = serde_json::to_vec(&settings)?;
    let existing = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(&mut **tx)
        .await
        .context("lecture du réglage des flux")?;
    match &existing {
        Some(row) if row.try_get::<Vec<u8>, _>("value")? == value => {
            report.skipped += 1;
            return Ok(report);
        }
        None if settings == Settings::default() => {
            report.skipped += 1;
            return Ok(report);
        }
        _ => {}
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(SETTINGS_KEY)
    .bind(&value)
    .execute(&mut **tx)
    .await
    .context("écriture du réglage des flux")?;
    report.count(if existing.is_some() {
        RestoreOutcome::Updated
    } else {
        RestoreOutcome::Created
    });
    Ok(report)
}
