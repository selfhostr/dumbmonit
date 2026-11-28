//! Modules facultatifs : mode mural, rapports SLA, pages de statut, SLO et
//! carte des menaces. Coupés sur une installation neuve ; la migration 0309
//! active ceux qu'une instance existante utilise déjà.
//!
//! Coupé, un module garde ses données en base mais son API répond 404 (le garde
//! [`guard`], une seule porte) et l'interface le masque. La page de statut
//! publique d'un module `status_pages` coupé répond 404 aussi.

use anyhow::Result;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::auth::settings;
use crate::state::AppState;

/// Clé du réglage dans la table `settings`.
pub const KEY: &str = "features";

/// Les modules activables. Un champ absent d'un enregistrement ancien vaut faux.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Features {
    #[serde(default)]
    pub wall: bool,
    #[serde(default)]
    pub sla_reports: bool,
    #[serde(default)]
    pub status_pages: bool,
    #[serde(default)]
    pub slo: bool,
    #[serde(default)]
    pub threat_map: bool,
}

/// Modification partielle : chaque champ absent garde sa valeur.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct FeaturesPatch {
    #[serde(default)]
    pub wall: Option<bool>,
    #[serde(default)]
    pub sla_reports: Option<bool>,
    #[serde(default)]
    pub status_pages: Option<bool>,
    #[serde(default)]
    pub slo: Option<bool>,
    #[serde(default)]
    pub threat_map: Option<bool>,
}

impl Features {
    /// Applique une modification et rend les noms des champs touchés (audit).
    pub fn apply(&mut self, patch: FeaturesPatch) -> Vec<&'static str> {
        let mut changed = Vec::new();
        for (name, slot, value) in [
            ("wall", &mut self.wall, patch.wall),
            ("sla_reports", &mut self.sla_reports, patch.sla_reports),
            ("status_pages", &mut self.status_pages, patch.status_pages),
            ("slo", &mut self.slo, patch.slo),
            ("threat_map", &mut self.threat_map, patch.threat_map),
        ] {
            if let Some(value) = value {
                if *slot != value {
                    changed.push(name);
                }
                *slot = value;
            }
        }
        changed
    }
}

/// Lit les modules. Réglage absent ou illisible : tout est coupé.
pub async fn load(pool: &SqlitePool) -> Features {
    match settings::get::<Features>(pool, KEY).await {
        Ok(Some(stored)) => stored,
        Ok(None) => Features::default(),
        Err(error) => {
            tracing::warn!(?error, "features unreadable, all modules off");
            Features::default()
        }
    }
}

/// Le réglage tel qu'il est en base (`None` : jamais écrit), pour la sauvegarde.
pub async fn stored(pool: &SqlitePool) -> Result<Option<Features>> {
    settings::get::<Features>(pool, KEY).await
}

pub async fn save(pool: &SqlitePool, value: &Features) -> Result<()> {
    settings::set(pool, KEY, value).await
}

/// Le module qui sert un chemin d'API (sans `/api`), s'il en dépend.
fn module_of(path: &str) -> Option<fn(&Features) -> bool> {
    let under = |prefix: &str| {
        path.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    if under("/slos") {
        Some(|f| f.slo)
    } else if under("/reports/schedules") || under("/reports/sla") {
        Some(|f| f.sla_reports)
    } else if under("/status-pages") || under("/incidents") || under("/public/status") {
        Some(|f| f.status_pages)
    } else if under("/wall") {
        Some(|f| f.wall)
    } else if under("/security/threats") {
        Some(|f| f.threat_map)
    } else {
        None
    }
}

/// Garde : 404 sur l'API d'un module coupé. Le réglage n'est lu que pour les
/// chemins concernés.
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let enabled = request.uri().path().strip_prefix("/api").and_then(module_of);
    if let Some(enabled) = enabled
        && !enabled(&load(&state.pool).await)
    {
        return crate::api::ApiError::NotFound("This module is off.".into()).into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATHS_OF_MODULES: [&str; 9] = [
        "/slos",
        "/slos/3",
        "/reports/schedules/1/pdf",
        "/reports/sla",
        "/status-pages",
        "/incidents/2/updates",
        "/public/status/x/rss",
        "/wall/weather",
        "/security/threats",
    ];
    const PATHS_OF_NONE: [&str; 5] =
        ["/slosh", "/reports/logo", "/targets", "/security/other", "/wallet"];

    #[test]
    fn a_new_install_has_every_module_off() {
        assert_eq!(Features::default(), serde_json::from_str::<Features>("{}").unwrap());
        assert!(!Features::default().wall);
    }

    #[test]
    fn a_patch_touches_only_what_it_names() {
        let mut features = Features { slo: true, ..Features::default() };
        let patch = FeaturesPatch { wall: Some(true), slo: Some(true), ..Default::default() };
        let changed = features.apply(patch);
        assert_eq!(changed, vec!["wall"]);
        assert!(features.wall && features.slo && !features.status_pages);
    }

    #[test]
    fn paths_map_to_their_module() {
        let off = Features::default();
        for path in PATHS_OF_MODULES {
            let enabled = module_of(path).unwrap_or_else(|| panic!("{path}"));
            assert!(!enabled(&off), "{path}");
        }
        for path in PATHS_OF_NONE {
            assert!(module_of(path).is_none(), "{path}");
        }
    }
}
