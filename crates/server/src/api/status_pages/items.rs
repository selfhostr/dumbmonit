//! Remplacement de la liste des services d'une page.

use super::*;

#[derive(Debug, Deserialize)]
pub struct ItemPayload {
    pub target_id: TargetId,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub group_name: Option<String>,
}

/// Remplace la liste des services d'une page. L'ordre du tableau est l'ordre
/// d'affichage ; un libellé vide reprend le nom de la cible.
pub async fn set_items(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<Vec<ItemPayload>>,
) -> ApiResult<Json<Vec<PageItem>>> {
    db::status_pages::get_page(&state.pool, id).await?.ok_or_else(|| page_not_found(id))?;
    if payload.len() > MAX_ITEMS {
        return Err(ApiError::BadRequest(format!("A page shows at most {MAX_ITEMS} services.")));
    }

    let targets: HashMap<TargetId, Target> = db::targets::list(&state.pool, &state.cipher)
        .await?
        .into_iter()
        .map(|target| (target.id, target))
        .collect();

    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for entry in payload {
        let Some(target) = targets.get(&entry.target_id) else {
            return Err(ApiError::BadRequest(format!(
                "Monitor {} does not exist.",
                entry.target_id
            )));
        };
        if !seen.insert(entry.target_id) {
            return Err(ApiError::BadRequest(format!(
                "Monitor \"{}\" is listed twice.",
                target.name
            )));
        }
        let label = match entry.label.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(label) => label.to_string(),
            None => target.name.clone(),
        };
        if label.chars().count() > MAX_LABEL_LEN {
            return Err(ApiError::BadRequest(format!(
                "Labels are limited to {MAX_LABEL_LEN} characters."
            )));
        }
        let group_name = entry.group_name.unwrap_or_default().trim().to_string();
        if group_name.chars().count() > MAX_GROUP_LEN {
            return Err(ApiError::BadRequest(format!(
                "Group names are limited to {MAX_GROUP_LEN} characters."
            )));
        }
        items.push(PageItemInput { target_id: entry.target_id, label, group_name });
    }

    // Mode MSP : la page d'un client ne montre que des équipements de ce client.
    if let Some(Some(client)) =
        crate::auth::clients::owner_of(&state.pool, crate::auth::clients::Kind::StatusPage, id)
            .await?
    {
        let own = crate::auth::clients::targets_of(&state.pool, client).await?;
        if let Some(foreign) = items.iter().find(|item| !own.contains(&item.target_id)) {
            return Err(ApiError::BadRequest(format!(
                "Monitor {} belongs to another client than this status page.",
                foreign.target_id
            )));
        }
    }

    db::status_pages::set_items(&state.pool, id, &items).await?;
    invalidate_cache();
    Ok(Json(db::status_pages::list_items(&state.pool, id).await?))
}
