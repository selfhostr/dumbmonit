//! Flux réseau (bêta) : réglage du récepteur et plus gros échanges d'un
//! équipement (`crate::flows`).
//!
//! - `GET` / `PUT /api/flows/settings` : activation et ports, état des
//!   récepteurs, compteurs reçus / écartés, exportateurs reconnus
//!   (administrateurs).
//! - `GET /api/targets/{id}/flows?range=1h` : plus gros émetteurs, échanges et
//!   ports de la cible sur la fenêtre, lus dans VictoriaMetrics.

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use dumbmonit_proto::TargetId;
use serde::{Deserialize, Serialize};

use crate::api::{ApiError, ApiResult};
use crate::auth::audit;
use crate::auth::client_ip::ClientIp;
use crate::auth::middleware::AdminIdentity;
use crate::db;
use crate::flows::{
    self, CountersView, Exporter, ListenerStatus, Protocol, Settings, UnknownSource,
};
use crate::state::AppState;
use crate::tsdb::InstantSeries;

/// Fenêtres proposées ; les flux suivent la rétention brute des métriques.
pub const RANGES: [&str; 4] = ["1h", "6h", "24h", "7d"];
/// Lignes par tableau.
const ROWS: usize = 10;
const BYTES: &str = "dumbmonit_flow_bytes_total";
const PACKETS: &str = "dumbmonit_flow_packets_total";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/flows/settings", get(overview).put(save))
        .route("/targets/{id}/flows", get(target_flows))
}

#[derive(Debug, Serialize)]
pub struct Overview {
    pub settings: Settings,
    pub listeners: Vec<ListenerStatus>,
    pub counters: CountersView,
    pub exporters: Vec<Exporter>,
    pub unknown_sources: Vec<UnknownSource>,
}

fn overview_of(settings: Settings) -> Overview {
    let hub = flows::hub();
    let mut listeners = hub.listeners();
    // Récepteur pas encore lancé (démarrage, démo, tests) : l'état se déduit
    // du réglage.
    for (protocol, port) in
        [(Protocol::Netflow, settings.netflow_port), (Protocol::Sflow, settings.sflow_port)]
    {
        if !listeners.iter().any(|held| held.protocol == protocol) {
            listeners.push(ListenerStatus { protocol, port, listening: false, error: None });
        }
    }
    Overview {
        settings,
        listeners,
        counters: hub.counters(),
        exporters: hub.exporters(),
        unknown_sources: hub.unknown_sources(),
    }
}

async fn overview(
    State(state): State<AppState>,
    AdminIdentity(_): AdminIdentity,
) -> ApiResult<Json<Overview>> {
    Ok(Json(overview_of(flows::load(&state.pool).await?)))
}

async fn save(
    State(state): State<AppState>,
    AdminIdentity(me): AdminIdentity,
    ClientIp(ip): ClientIp,
    Json(settings): Json<Settings>,
) -> ApiResult<Json<Overview>> {
    settings.validate().map_err(ApiError::BadRequest)?;
    flows::save(&state.pool, &settings).await?;
    flows::hub().apply(&settings);
    let subject = if settings.enabled {
        format!("on, NetFlow/IPFIX {}, sFlow {}", settings.netflow_port, settings.sflow_port)
    } else {
        "off".to_string()
    };
    audit::record(&state.pool, Some(&me.label()), "flows.settings", Some(&subject), ip).await;
    Ok(Json(overview_of(settings)))
}

// --------------------------------------------------------------------------
// Échanges d'une cible
// --------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct FlowsQuery {
    pub range: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Talker {
    pub address: String,
    pub bytes: f64,
}

#[derive(Debug, Serialize)]
pub struct Conversation {
    pub src: String,
    pub dst: String,
    pub proto: String,
    pub port: String,
    pub bytes: f64,
    pub packets: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct PortRow {
    pub proto: String,
    pub port: String,
    pub bytes: f64,
}

#[derive(Debug, Serialize)]
pub struct TargetFlows {
    pub range: String,
    /// Vrai si la cible a exporté des flux sur les dernières 24 h (ou sur la
    /// fenêtre, si elle est plus longue) : l'interface n'affiche la section
    /// qu'à cette condition.
    pub available: bool,
    pub total_bytes: f64,
    pub total_packets: f64,
    /// Plus gros émetteurs (adresse source).
    pub talkers: Vec<Talker>,
    pub conversations: Vec<Conversation>,
    pub ports: Vec<PortRow>,
}

fn value(series: &InstantSeries) -> f64 {
    series.value.1.parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0)
}

fn label(series: &InstantSeries, name: &str) -> String {
    series.metric.get(name).cloned().unwrap_or_default()
}

fn total(series: &[InstantSeries]) -> f64 {
    series.first().map(value).unwrap_or(0.0)
}

fn by_bytes_desc<T>(rows: &mut [T], bytes: impl Fn(&T) -> f64) {
    rows.sort_by(|a, b| bytes(b).total_cmp(&bytes(a)));
}

/// Les requêtes d'une cible sur une fenêtre, dans l'ordre : octets, paquets,
/// émetteurs, échanges, paquets des échanges, ports.
pub fn queries(id: TargetId, range: &str) -> [String; 6] {
    let window = |metric: &str, extra: &str| {
        format!(r#"sum_over_time({metric}{{target="{id}"{extra}}}[{range}])"#)
    };
    let known = r#",src!="other""#;
    let group = "src, dst, proto, port";
    let conversations = format!("topk({ROWS}, sum by ({group}) ({}))", window(BYTES, known));
    // Les paquets des seuls échanges retenus par octets.
    let conversation_packets =
        format!("sum by ({group}) ({}) and on ({group}) {conversations}", window(PACKETS, known));
    [
        format!("sum({})", window(BYTES, "")),
        format!("sum({})", window(PACKETS, "")),
        format!("topk({ROWS}, sum by (src) ({}))", window(BYTES, known)),
        conversations,
        conversation_packets,
        format!(
            "topk({ROWS}, sum by (proto, port) ({}))",
            window(BYTES, r#",port!="other",port!="0""#)
        ),
    ]
}

async fn target_flows(
    State(state): State<AppState>,
    Path(id): Path<TargetId>,
    Query(query): Query<FlowsQuery>,
) -> ApiResult<Json<TargetFlows>> {
    db::targets::get(&state.pool, &state.cipher, id)
        .await?
        .ok_or_else(|| ApiError::monitor_not_found(&id))?;
    let range = query.range.unwrap_or_else(|| "1h".into());
    if !RANGES.contains(&range.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "Unknown range {range:?}: use one of {}.",
            RANGES.join(", ")
        )));
    }
    let [bytes, packets, talkers, conversations, conversation_packets, ports] = queries(id, &range);
    let victoria = &state.victoria;
    let (bytes, packets, talkers, conversations, conversation_packets, ports) = tokio::try_join!(
        victoria.query(&bytes),
        victoria.query(&packets),
        victoria.query(&talkers),
        victoria.query(&conversations),
        victoria.query(&conversation_packets),
        victoria.query(&ports),
    )?;
    let total_bytes = total(&bytes);
    let available = if total_bytes > 0.0 || matches!(range.as_str(), "24h" | "7d") {
        total_bytes > 0.0
    } else {
        let day = format!(r#"count(last_over_time({BYTES}{{target="{id}"}}[24h]))"#);
        total(&victoria.query(&day).await?) > 0.0
    };

    let key = |series: &InstantSeries| {
        ["src", "dst", "proto", "port"].map(|name| label(series, name)).join("|")
    };
    let packets_of: BTreeMap<String, f64> =
        conversation_packets.iter().map(|series| (key(series), value(series))).collect();
    let mut talkers: Vec<Talker> = talkers
        .iter()
        .map(|series| Talker { address: label(series, "src"), bytes: value(series) })
        .collect();
    by_bytes_desc(&mut talkers, |row| row.bytes);
    let mut conversations: Vec<Conversation> = conversations
        .iter()
        .map(|series| Conversation {
            src: label(series, "src"),
            dst: label(series, "dst"),
            proto: label(series, "proto"),
            port: label(series, "port"),
            bytes: value(series),
            packets: packets_of.get(&key(series)).copied(),
        })
        .collect();
    by_bytes_desc(&mut conversations, |row| row.bytes);
    let mut ports: Vec<PortRow> = ports
        .iter()
        .map(|series| PortRow {
            proto: label(series, "proto"),
            port: label(series, "port"),
            bytes: value(series),
        })
        .collect();
    by_bytes_desc(&mut ports, |row| row.bytes);

    Ok(Json(TargetFlows {
        range,
        available,
        total_bytes,
        total_packets: total(&packets),
        talkers,
        conversations,
        ports,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queries_stay_on_one_target_and_skip_the_other_row() {
        let [bytes, _, talkers, conversations, packets, ports] = queries(7, "1h");
        assert_eq!(bytes, r#"sum(sum_over_time(dumbmonit_flow_bytes_total{target="7"}[1h]))"#);
        assert!(talkers.contains(r#"target="7",src!="other""#), "{talkers}");
        assert!(conversations.starts_with("topk(10, sum by (src, dst, proto, port)"));
        assert!(packets.contains("and on (src, dst, proto, port) topk(10,"), "{packets}");
        assert!(ports.contains(r#"port!="0""#), "{ports}");
    }
}
