//! Post-mortem d'un incident : brouillon à tout moment, publication une fois
//! l'incident résolu.
//!
//! Le texte est du texte brut, rendu par paragraphes comme les messages du
//! fil : ni HTML ni Markdown n'en sortent interprétés. La page publique ne le
//! montre qu'une fois publié, et seulement pour un incident résolu.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use super::{
    IncidentView, format_timestamp, incident_not_found, incident_views, invalidate_cache,
    is_closing, parse_timestamp,
};
use crate::api::{ApiError, ApiResult};
use crate::db;
use crate::db::status_pages::{ActionItem, Incident, Postmortem, PostmortemInput, TimelineEntry};
use crate::state::AppState;

/// Longueur maximale du résumé, de l'impact et de la cause.
pub const MAX_TEXT_LEN: usize = 20_000;
/// Étapes de la chronologie et longueur de chacune.
pub const MAX_TIMELINE: usize = 100;
pub const MAX_ENTRY_LEN: usize = 2_000;
/// Actions et longueur de chacune.
pub const MAX_ACTIONS: usize = 50;
pub const MAX_ACTION_LEN: usize = 500;

#[derive(Debug, Deserialize)]
pub struct TimelinePayload {
    pub at: String,
    pub text: String,
}

/// Post-mortem soumis : un remplacement complet, chaque champ absent vaut vide.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PostmortemPayload {
    pub summary: String,
    pub impact: String,
    pub root_cause: String,
    pub timeline: Vec<TimelinePayload>,
    pub actions: Vec<ActionItem>,
    pub published: bool,
}

fn text(raw: &str, field: &str) -> ApiResult<String> {
    let value = raw.trim().to_string();
    if value.chars().count() > MAX_TEXT_LEN {
        return Err(ApiError::BadRequest(format!(
            "{field} is limited to {MAX_TEXT_LEN} characters."
        )));
    }
    Ok(value)
}

/// Valide un post-mortem pour cet incident.
pub fn validate(payload: PostmortemPayload, incident: &Incident) -> ApiResult<PostmortemInput> {
    if incident.kind != "incident" {
        return Err(ApiError::BadRequest(
            "Post-mortems are written for incidents, not for maintenance windows.".into(),
        ));
    }
    let summary = text(&payload.summary, "The summary")?;
    let impact = text(&payload.impact, "The impact")?;
    let root_cause = text(&payload.root_cause, "The root cause")?;

    if payload.timeline.len() > MAX_TIMELINE {
        return Err(ApiError::BadRequest(format!(
            "A timeline holds at most {MAX_TIMELINE} entries."
        )));
    }
    let mut timeline = Vec::with_capacity(payload.timeline.len().min(MAX_TIMELINE));
    for entry in payload.timeline {
        let body = entry.text.trim().to_string();
        if body.is_empty() {
            return Err(ApiError::BadRequest("Each timeline entry needs a text.".into()));
        }
        if body.chars().count() > MAX_ENTRY_LEN {
            return Err(ApiError::BadRequest(format!(
                "A timeline entry is limited to {MAX_ENTRY_LEN} characters."
            )));
        }
        let at = parse_timestamp(&entry.at, "A timeline date")?;
        timeline.push((at, body));
    }
    // Dans l'ordre du temps, quel que soit l'ordre de saisie ; deux étapes à la
    // même minute gardent l'ordre donné.
    timeline.sort_by_key(|(at, _)| *at);
    let timeline: Vec<TimelineEntry> = timeline
        .into_iter()
        .map(|(at, text)| TimelineEntry { at: format_timestamp(at), text })
        .collect();

    if payload.actions.len() > MAX_ACTIONS {
        return Err(ApiError::BadRequest(format!(
            "A post-mortem lists at most {MAX_ACTIONS} action items."
        )));
    }
    let mut actions = Vec::with_capacity(payload.actions.len().min(MAX_ACTIONS));
    for item in payload.actions {
        let body = item.text.trim().to_string();
        if body.is_empty() {
            return Err(ApiError::BadRequest("Each action item needs a text.".into()));
        }
        if body.chars().count() > MAX_ACTION_LEN {
            return Err(ApiError::BadRequest(format!(
                "An action item is limited to {MAX_ACTION_LEN} characters."
            )));
        }
        actions.push(ActionItem { text: body, done: item.done });
    }

    if payload.published {
        if !is_closing(&incident.status) {
            return Err(ApiError::BadRequest(
                "Publish the post-mortem once the incident is resolved: until then, keep it as \
                 a draft."
                    .into(),
            ));
        }
        if summary.is_empty() {
            return Err(ApiError::BadRequest("Write a summary before publishing.".into()));
        }
    }
    Ok(PostmortemInput {
        summary,
        impact,
        root_cause,
        timeline,
        actions,
        published: payload.published,
    })
}

/// Le post-mortem tel que le public le lit : sans identifiant ni date
/// d'écriture.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PublicPostmortem {
    pub summary: String,
    pub impact: String,
    pub root_cause: String,
    pub timeline: Vec<TimelineEntry>,
    pub actions: Vec<ActionItem>,
    pub published_at: Option<String>,
}

/// Ce que la page publique montre d'un post-mortem : rien tant qu'il n'est pas
/// publié, ni pour un incident rouvert depuis.
pub fn public(postmortem: Option<Postmortem>, incident_status: &str) -> Option<PublicPostmortem> {
    let postmortem = postmortem.filter(|p| p.published && is_closing(incident_status))?;
    Some(PublicPostmortem {
        summary: postmortem.summary,
        impact: postmortem.impact,
        root_cause: postmortem.root_cause,
        timeline: postmortem.timeline,
        actions: postmortem.actions,
        published_at: postmortem.published_at,
    })
}

pub async fn get_postmortem(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Postmortem>> {
    db::status_pages::get_incident(&state.pool, id).await?.ok_or_else(|| incident_not_found(id))?;
    db::status_pages::get_postmortem(&state.pool, id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("Incident {id} has no post-mortem yet.")))
}

/// Crée ou remplace le post-mortem ; rend l'incident complet.
pub async fn put_postmortem(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<PostmortemPayload>,
) -> ApiResult<Json<IncidentView>> {
    let incident = db::status_pages::get_incident(&state.pool, id)
        .await?
        .ok_or_else(|| incident_not_found(id))?;
    let input = validate(payload, &incident)?;
    db::status_pages::save_postmortem(&state.pool, id, &input).await?;
    invalidate_cache();
    tracing::info!(incident = id, published = input.published, "post-mortem saved");
    let mut views = incident_views(&state, vec![incident]).await?;
    Ok(Json(views.remove(0)))
}

pub async fn delete_postmortem(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if db::status_pages::delete_postmortem(&state.pool, id).await? {
        invalidate_cache();
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("Incident {id} has no post-mortem.")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incident(kind: &str, status: &str) -> Incident {
        Incident {
            id: 1,
            page_id: None,
            title: "NAS down".to_string(),
            kind: kind.to_string(),
            status: status.to_string(),
            severity: "major".to_string(),
            starts_at: "2026-11-01T10:00:00Z".to_string(),
            ends_at: None,
            created_at: "2026-11-01T10:00:00Z".to_string(),
            updated_at: "2026-11-01T10:00:00Z".to_string(),
        }
    }

    fn accepted(result: ApiResult<PostmortemInput>) -> PostmortemInput {
        match result {
            Ok(input) => input,
            Err(error) => panic!("refused: {}", error.into_parts().1),
        }
    }

    fn payload(published: bool) -> PostmortemPayload {
        PostmortemPayload {
            summary: " The NAS lost a disk. ".to_string(),
            timeline: vec![
                TimelinePayload { at: "2026-11-01T11:00:00Z".to_string(), text: "Fixed".into() },
                TimelinePayload { at: "2026-11-01T10:05".to_string(), text: "Paged".into() },
            ],
            actions: vec![ActionItem { text: " Add a spare ".to_string(), done: false }],
            published,
            ..PostmortemPayload::default()
        }
    }

    #[test]
    fn la_chronologie_est_triee_et_normalisee() {
        let input = accepted(validate(payload(false), &incident("incident", "investigating")));
        assert_eq!(input.summary, "The NAS lost a disk.");
        assert_eq!(input.timeline[0].at, "2026-11-01T10:05:00Z");
        assert_eq!(input.timeline[0].text, "Paged");
        assert_eq!(input.timeline[1].text, "Fixed");
        assert_eq!(input.actions[0].text, "Add a spare");
    }

    #[test]
    fn la_publication_attend_la_resolution() {
        assert!(validate(payload(true), &incident("incident", "monitoring")).is_err());
        assert!(validate(payload(true), &incident("incident", "resolved")).is_ok());
        let empty = PostmortemPayload { published: true, ..PostmortemPayload::default() };
        assert!(validate(empty, &incident("incident", "resolved")).is_err(), "summary first");
        assert!(validate(payload(false), &incident("maintenance", "completed")).is_err());
    }

    #[test]
    fn les_lignes_vides_ou_les_dates_illisibles_sont_refusees() {
        let mut bad = payload(false);
        bad.timeline[0].text = "  ".to_string();
        assert!(validate(bad, &incident("incident", "resolved")).is_err());
        let mut bad = payload(false);
        bad.timeline[0].at = "yesterday".to_string();
        assert!(validate(bad, &incident("incident", "resolved")).is_err());
        let mut bad = payload(false);
        bad.actions[0].text = String::new();
        assert!(validate(bad, &incident("incident", "resolved")).is_err());
    }

    #[test]
    fn le_public_ne_voit_qu_un_post_mortem_publie_d_un_incident_resolu() {
        let stored = Postmortem {
            incident_id: 1,
            summary: "Disk".to_string(),
            impact: String::new(),
            root_cause: String::new(),
            timeline: Vec::new(),
            actions: Vec::new(),
            published: true,
            published_at: Some("2026-11-02T08:00:00Z".to_string()),
            created_at: String::new(),
            updated_at: String::new(),
        };
        assert!(public(Some(stored.clone()), "resolved").is_some());
        assert!(public(Some(stored.clone()), "investigating").is_none(), "reopened");
        let draft = Postmortem { published: false, ..stored };
        assert!(public(Some(draft), "resolved").is_none());
        assert!(public(None, "resolved").is_none());
    }
}
