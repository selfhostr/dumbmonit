//! Restauration d'un lot dans une base.
//!
//! Trois principes, et ils tiennent tous les trois dans le compte rendu que
//! l'interface montre avant d'appliquer quoi que ce soit :
//!
//! * **rien n'est jamais supprimé**. Restaurer, c'est faire en sorte que ce que
//!   le lot décrit existe ; ce que l'instance a en plus reste ;
//! * **rien n'est jamais dupliqué**. Chaque chose a une clé naturelle — type +
//!   adresse pour un équipement, `uid` pour une règle, nom pour un canal — et
//!   c'est sur elle que l'on retrouve ce qui est déjà là. Restaurer deux fois
//!   le même lot ne crée rien la seconde fois ;
//! * **la simulation est le geste réel, annulé**. Le même code tourne dans les
//!   deux cas, dans une transaction que l'on valide ou que l'on annule. Le
//!   compte rendu d'une simulation ne peut donc pas mentir sur ce que
//!   l'application ferait.
//!
//! Les comptes font exception à « mettre à jour » : un compte qui existe déjà
//! est laissé tel quel. Écraser le rôle ou le mot de passe de l'administrateur
//! connecté au moyen d'un fichier serait le plus court chemin vers une instance
//! dont plus personne n'a la clé.

use std::collections::HashMap;

use anyhow::{Context, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use serde_json::Value;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::api::topology_edits::{self, CorrectionKind, RuleKind, RuleSpec, RuleStatus};
use crate::backup::bundle::*;
use crate::backup::monitor_templates as templates;
use crate::collectors::agent;
use crate::crypto::Cipher;

/// Sort d'une ligne du lot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreOutcome {
    Created,
    Updated,
    Skipped,
}

/// Ce qu'une section du lot a donné.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SectionReport {
    pub section: String,
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    /// Ce qui mérite d'être lu ligne par ligne : une référence introuvable, une
    /// ligne refusée par la base.
    pub notes: Vec<String>,
}

impl SectionReport {
    pub(crate) fn new(section: &str) -> Self {
        Self { section: section.to_string(), ..Self::default() }
    }

    pub(crate) fn count(&mut self, outcome: RestoreOutcome) {
        match outcome {
            RestoreOutcome::Created => self.created += 1,
            RestoreOutcome::Updated => self.updated += 1,
            RestoreOutcome::Skipped => self.skipped += 1,
        }
    }

    pub(crate) fn refused(&mut self, what: &str, error: &anyhow::Error) {
        self.skipped += 1;
        self.notes.push(format!("{what} was refused by the database: {error}"));
    }
}

/// Compte rendu complet d'une restauration, simulée ou appliquée.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RestoreReport {
    /// Faux : rien n'a été écrit, la transaction a été annulée.
    pub applied: bool,
    /// Date d'écriture du lot et version du serveur qui l'a produit.
    pub created_at: String,
    pub source_version: String,
    pub sections: Vec<SectionReport>,
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    /// Ce que l'opérateur doit savoir avant, ou juste après, d'appliquer.
    pub warnings: Vec<String>,
}

impl RestoreReport {
    fn push(&mut self, section: SectionReport) {
        self.created += section.created;
        self.updated += section.updated;
        self.skipped += section.skipped;
        self.sections.push(section);
    }
}

/// Restaure un lot. `apply` faux : tout est calculé puis annulé.
pub async fn restore(
    pool: &SqlitePool,
    cipher: &Cipher,
    bundle: &Bundle,
    apply: bool,
) -> Result<RestoreReport> {
    let mut tx = pool.begin().await.context("ouverture de la transaction de restauration")?;

    let mut report = RestoreReport {
        applied: apply,
        created_at: bundle.created_at.clone(),
        source_version: bundle.source_version.clone(),
        ..RestoreReport::default()
    };

    // Les rôles personnalisés avant les comptes, qui les citent.
    report.push(crate::backup::accounts::restore_roles(&mut tx, &bundle.accounts).await?);
    report.push(restore_users(&mut tx, cipher, bundle).await?);
    // Les clients avant le reste : leurs membres sont des comptes, et les
    // rattachements, appliqués à la fin, les désignent par leur nom.
    let mut clients_report = restore_clients(&mut tx, bundle).await?;
    // Les paquets avant les équipements : ils en définissent les types.
    report.push(restore_packs(&mut tx, bundle).await?);
    report.push(restore_pack_registry(&mut tx, bundle).await?);
    report.push(restore_targets(&mut tx, cipher, bundle).await?);

    let refs = target_ids(&mut tx).await?;
    report.push(restore_locations(&mut tx, bundle, &refs).await?);
    report.push(restore_teams(&mut tx, bundle).await?);
    // Après les comptes et les équipes : les invitations citent des équipes,
    // les notifications personnelles des comptes.
    for section in crate::backup::accounts::restore(&mut tx, &bundle.accounts).await? {
        report.push(section);
    }
    report.push(restore_topology_corrections(&mut tx, bundle, &refs).await?);
    report.push(restore_topology_rules(&mut tx, bundle, &refs).await?);
    report.push(restore_lun_links(&mut tx, bundle, &refs).await?);
    report.push(restore_upgrade_notes(&mut tx, bundle).await?);

    report.push(restore_channels(&mut tx, cipher, bundle).await?);
    let channels = channel_ids(&mut tx).await?;

    report.push(restore_rules(&mut tx, bundle, &refs, &channels).await?);
    report.push(restore_overrides(&mut tx, bundle, &refs).await?);
    report.push(restore_notify_policy(&mut tx, bundle).await?);
    report.push(restore_agent_enrolment(&mut tx, bundle).await?);
    report.push(restore_backup_schedule(&mut tx, bundle).await?);
    report.push(restore_wall_weather(&mut tx, bundle).await?);
    report.push(restore_auth_policy(&mut tx, bundle).await?);
    report.push(restore_agent_update_rollout(&mut tx, bundle).await?);
    report.push(restore_mail_server(&mut tx, cipher, bundle).await?);
    let destinations = &bundle.backup_destinations;
    report.push(crate::backup::offsite::bundle::restore(&mut tx, cipher, destinations).await?);
    report.push(restore_instance(&mut tx, bundle).await?);
    report.push(restore_features(&mut tx, bundle).await?);
    report.push(restore_silences(&mut tx, bundle, &refs).await?);
    report.push(restore_status_pages(&mut tx, bundle, &refs).await?);
    report.push(restore_report_schedules(&mut tx, bundle, &channels, &refs).await?);
    report.push(restore_report_logo(&mut tx, bundle).await?);
    report.push(restore_slos(&mut tx, bundle, &refs).await?);
    report.push(templates::restore_templates(&mut tx, &bundle.monitor_templates, &channels).await?);
    report.push(
        templates::restore_target_channels(&mut tx, &bundle.target_channels, &refs, &channels)
            .await?,
    );
    report.push(crate::backup::annotations::restore(&mut tx, &bundle.annotations, &refs).await?);
    report.push(restore_incidents(&mut tx, bundle).await?);
    report.push(restore_agent_tokens(&mut tx, bundle).await?);
    report.push(restore_api_tokens(&mut tx, bundle).await?);
    report.push(restore_push_monitors(&mut tx, cipher, bundle, &refs).await?);
    report.push(restore_agent_sentinels(&mut tx, cipher, bundle, &refs).await?);
    report.push(crate::backup::oncall::restore(&mut tx, cipher, &bundle.oncall).await?);
    report.push(restore_runbooks(&mut tx, bundle).await?);
    report.push(restore_remediation_actions(&mut tx, cipher, bundle, &refs).await?);
    report.push(restore_remediation_policies(&mut tx, bundle, &refs).await?);
    report.push(restore_netconfig_host_keys(&mut tx, bundle, &refs).await?);
    report.push(restore_netconfig_versions(&mut tx, cipher, bundle, &refs).await?);
    report.push(restore_netscan_schedules(&mut tx, bundle, &refs).await?);
    restore_client_assignments(&mut tx, bundle, &mut clients_report).await?;
    report.push(clients_report);

    // Lot d'avant les modules facultatifs : rallume ceux dont les données reviennent.
    enable_restored_features(&mut tx, bundle).await?;

    warnings(bundle, &mut report);

    if apply {
        tx.commit().await.context("validation de la restauration")?;
        // Les domaines publics des pages ont pu changer.
        crate::status_host::invalidate();
        // Une sauvegarde ancienne peut ramener des en-têtes de webhook en clair.
        if let Err(error) = crate::db::alerts::lift_legacy_webhook_headers(pool, cipher).await {
            tracing::warn!(?error, "webhook headers of the backup left in settings");
        }
    } else {
        tx.rollback().await.context("annulation de la simulation")?;
    }
    Ok(report)
}

/// Applique, dans la transaction de l'appelant, les sections qu'un fichier de
/// configuration déclare (voir [`crate::gitops`]) : équipements, canaux, règles,
/// surcharges, fenêtres de maintenance et pages de statut. La validation et
/// l'annulation restent à l'appelant, qui doit aussi traiter les notes des
/// sections comme des refus.
pub(crate) async fn restore_config(
    tx: &mut Transaction<'_, Sqlite>,
    cipher: &Cipher,
    bundle: &Bundle,
) -> Result<Vec<SectionReport>> {
    let mut sections = vec![restore_targets(tx, cipher, bundle).await?];
    let refs = target_ids(tx).await?;
    sections.push(restore_channels(tx, cipher, bundle).await?);
    let channels = channel_ids(tx).await?;
    sections.push(restore_rules(tx, bundle, &refs, &channels).await?);
    sections.push(restore_overrides(tx, bundle, &refs).await?);
    sections.push(restore_silences(tx, bundle, &refs).await?);
    sections.push(restore_status_pages(tx, bundle, &refs).await?);
    Ok(sections)
}

fn warnings(bundle: &Bundle, report: &mut RestoreReport) {
    if !bundle.remediation_policies.is_empty() {
        report.warnings.push(
            "Automatic remediation policies are restored disarmed: an administrator must arm \
             each one again, in two steps."
                .to_string(),
        );
    }
    if bundle
        .users
        .iter()
        .any(|u| u.password_hash.is_none() && u.oidc_subject.is_none() && u.saml_subject.is_none())
    {
        report.warnings.push(
            "This backup holds no account passwords. Restored accounts cannot sign in until an \
             administrator sets a password for them, or until they sign in through SSO."
                .to_string(),
        );
    }
    if !bundle.targets.is_empty() {
        report.warnings.push(
            "Device credentials from the backup are now encrypted with this server's own \
             instance secret (/data/secret.key). Back that file up: without it they cannot be \
             read again."
                .to_string(),
        );
    }
    if report.sections.iter().any(|s| !s.notes.is_empty()) {
        report.warnings.push(
            "Some entries could not be restored as they were; read the notes of each section."
                .to_string(),
        );
    }
}

mod alerting;
mod clients;
mod incidents;
mod monitors;
mod netconfig;
mod notify;
mod remediation;
mod reports;
mod settings;
mod silences;
mod status_pages;
mod targets;
mod topology;
mod users;

use alerting::*;
use clients::*;
use incidents::*;
use monitors::*;
use netconfig::*;
use notify::*;
use remediation::*;
use reports::*;
use settings::*;
use silences::*;
use status_pages::*;
use targets::*;
use topology::*;
use users::*;
