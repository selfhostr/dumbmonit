//! La météo du mur : la température et le temps qu'il fait dehors, à côté de
//! la date. C'est le serveur qui interroge Open-Meteo (gratuit, sans clé) ; le
//! navigateur n'appelle jamais personne.
//!
//! - Rien n'est demandé tant qu'aucun lieu n'est choisi dans les réglages du mur.
//! - Le résultat est gardé [`CURRENT_TTL`] (dix minutes) : quel que soit le
//!   nombre de murs, Open-Meteo reçoit au plus une requête par lieu et par
//!   période. En cas d'échec, la dernière valeur connue sert encore une heure,
//!   puis le mur n'affiche plus rien (jamais de message d'erreur à l'écran).
//! - Adresses fixes, sans surcharge par l'environnement (comme
//!   `music::spotify`) ; les tests les injectent par [`Endpoints`].
//!
//! Le réglage (lieu et unité) vit dans la table `settings`, clé
//! [`SETTINGS_KEY`], et voyage avec la sauvegarde.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::Mutex;

/// Clé du réglage dans `settings`.
pub const SETTINGS_KEY: &str = "wall.weather";

/// Durée pendant laquelle la météo d'un lieu est resservie sans redemander.
pub const CURRENT_TTL: Duration = Duration::from_secs(10 * 60);
/// Au-delà, une valeur ancienne (Open-Meteo en panne) n'est plus montrée.
const STALE_LIMIT: Duration = Duration::from_secs(60 * 60);
/// Après un échec, on ne redemande pas avant ce délai.
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// Délai d'attente d'une requête vers Open-Meteo.
const TIMEOUT: Duration = Duration::from_secs(8);

/// Les adresses d'Open-Meteo. Aucune surcharge par l'environnement.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub forecast: String,
    pub geocoding: String,
}

impl Endpoints {
    pub fn official() -> Self {
        Self {
            forecast: "https://api.open-meteo.com".into(),
            geocoding: "https://geocoding-api.open-meteo.com".into(),
        }
    }
}

/// Unité de température affichée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    C,
    F,
}

/// Un lieu choisi dans les réglages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    /// Région ou département, pour lever les homonymes.
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// Contrôle d'un lieu venu d'un client (ou d'une sauvegarde éditée à la main).
    pub fn validate(&self) -> Result<(), &'static str> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
            return Err("The place needs a name of at most 100 characters.");
        }
        let short = |v: &Option<String>| {
            v.as_deref().is_none_or(|s| s.chars().count() <= 100 && !s.chars().any(char::is_control))
        };
        if !short(&self.region) || !short(&self.country) {
            return Err("The region and the country are at most 100 characters.");
        }
        if !(self.latitude.is_finite() && (-90.0..=90.0).contains(&self.latitude))
            || !(self.longitude.is_finite() && (-180.0..=180.0).contains(&self.longitude))
        {
            return Err("The coordinates are out of range.");
        }
        Ok(())
    }
}

/// Le réglage : pas de lieu, pas de météo.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub place: Option<Place>,
    #[serde(default)]
    pub unit: Unit,
}

pub async fn load_settings(pool: &SqlitePool) -> Result<Settings> {
    Ok(crate::auth::settings::get::<Settings>(pool, SETTINGS_KEY).await?.unwrap_or_default())
}

pub async fn save_settings(pool: &SqlitePool, settings: &Settings) -> Result<()> {
    crate::auth::settings::set(pool, SETTINGS_KEY, settings).await
}

/// Le temps qu'il fait, réduit à ce que le mur sait dessiner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Rain,
    Snow,
    Thunder,
}

impl Condition {
    /// Code météo WMO d'Open-Meteo (`weather_code`) ; `None` pour un code inconnu.
    pub fn from_wmo(code: i64) -> Option<Self> {
        Some(match code {
            0 | 1 => Self::Clear,
            2 => Self::PartlyCloudy,
            3 => Self::Cloudy,
            45 | 48 => Self::Fog,
            51..=57 | 61..=67 | 80..=82 => Self::Rain,
            71..=77 | 85 | 86 => Self::Snow,
            95..=99 => Self::Thunder,
            _ => return None,
        })
    }
}

/// La météo du moment, dans l'unité du réglage.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Current {
    /// Température extérieure, arrondie au degré.
    pub temperature: i32,
    pub condition: Condition,
    /// Faux la nuit : l'icône prend sa variante nocturne.
    pub is_day: bool,
}

#[derive(Debug, Clone, Copy)]
struct Reading {
    celsius: f64,
    condition: Condition,
    is_day: bool,
}

struct CacheEntry {
    key: (i64, i64),
    reading: Option<Reading>,
    /// Dernière lecture réussie, gardée tant qu'elle n'est pas trop vieille.
    good: Option<(Reading, Instant)>,
    at: Instant,
}

struct Inner {
    endpoints: Endpoints,
    http: reqwest::Client,
    cache: Mutex<Option<CacheEntry>>,
}

/// Le client Open-Meteo et son cache, partagés par les gestionnaires HTTP.
#[derive(Clone)]
pub struct WeatherHub(Arc<Inner>);

impl WeatherHub {
    pub fn new(endpoints: Endpoints) -> Self {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(4))
            .user_agent(concat!("DumbMonit/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("client HTTP Open-Meteo");
        Self(Arc::new(Inner { endpoints, http, cache: Mutex::new(None) }))
    }

    /// Le hub de service, sur les adresses officielles.
    pub fn official() -> Self {
        Self::new(Endpoints::official())
    }

    /// La météo du lieu, ou `None` (échec, valeur trop ancienne) : le mur
    /// n'affiche alors rien. Une requête à la fois : le verrou est tenu pendant
    /// l'appel, pour que dix murs n'en déclenchent qu'un.
    pub async fn current(&self, place: &Place, unit: Unit) -> Option<Current> {
        let key = ((place.latitude * 100.0).round() as i64, (place.longitude * 100.0).round() as i64);
        let mut cache = self.0.cache.lock().await;
        if let Some(entry) = cache.as_ref().filter(|e| e.key == key) {
            let ttl = if entry.reading.is_some() { CURRENT_TTL } else { RETRY_AFTER };
            if entry.at.elapsed() < ttl {
                return shown(entry, unit);
            }
        }
        let fresh = self.fetch(place).await;
        let previous = cache.take().filter(|e| e.key == key).and_then(|e| e.good);
        let good = match fresh {
            Some(reading) => Some((reading, Instant::now())),
            None => previous.filter(|(_, at)| at.elapsed() < STALE_LIMIT),
        };
        let entry = CacheEntry { key, reading: fresh, good, at: Instant::now() };
        let out = shown(&entry, unit);
        *cache = Some(entry);
        out
    }

    async fn fetch(&self, place: &Place) -> Option<Reading> {
        let url = format!("{}/v1/forecast", self.0.endpoints.forecast.trim_end_matches('/'));
        let response = self
            .0
            .http
            .get(url)
            .query(&[
                ("latitude", format!("{:.4}", place.latitude)),
                ("longitude", format!("{:.4}", place.longitude)),
                ("current", "temperature_2m,weather_code,is_day".to_string()),
                ("timezone", "auto".to_string()),
            ])
            .send()
            .await
            .map_err(|error| tracing::debug!(%error, "Open-Meteo injoignable"))
            .ok()?;
        if !response.status().is_success() {
            tracing::debug!(status = %response.status(), "Open-Meteo a refusé la demande");
            return None;
        }
        let body: ForecastBody = response.json().await.ok()?;
        let current = body.current?;
        Some(Reading {
            celsius: current.temperature_2m.filter(|t| t.is_finite())?,
            condition: Condition::from_wmo(current.weather_code?)?,
            is_day: current.is_day.unwrap_or(1) != 0,
        })
    }

    /// Recherche de ville par nom (réglages du mur, administrateurs). `None`
    /// quand Open-Meteo ne répond pas.
    pub async fn search(&self, query: &str, language: &str) -> Option<Vec<Place>> {
        let url = format!("{}/v1/search", self.0.endpoints.geocoding.trim_end_matches('/'));
        let response = self
            .0
            .http
            .get(url)
            .query(&[("name", query), ("count", "6"), ("language", language), ("format", "json")])
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body: GeocodingBody = response.json().await.ok()?;
        Some(
            body.results
                .unwrap_or_default()
                .into_iter()
                .filter_map(|hit| {
                    let place = Place {
                        name: hit.name?,
                        region: hit.admin1,
                        country: hit.country,
                        latitude: hit.latitude?,
                        longitude: hit.longitude?,
                    };
                    place.validate().is_ok().then_some(place)
                })
                .collect(),
        )
    }
}

fn shown(entry: &CacheEntry, unit: Unit) -> Option<Current> {
    let reading = entry.reading.or(entry.good.map(|(r, _)| r))?;
    let value = match unit {
        Unit::C => reading.celsius,
        Unit::F => reading.celsius * 9.0 / 5.0 + 32.0,
    };
    Some(Current {
        temperature: value.round() as i32,
        condition: reading.condition,
        is_day: reading.is_day,
    })
}

#[derive(Deserialize)]
struct ForecastBody {
    current: Option<ForecastCurrent>,
}

#[derive(Deserialize)]
struct ForecastCurrent {
    temperature_2m: Option<f64>,
    weather_code: Option<i64>,
    is_day: Option<i64>,
}

#[derive(Deserialize)]
struct GeocodingBody {
    results: Option<Vec<GeocodingHit>>,
}

#[derive(Deserialize)]
struct GeocodingHit {
    name: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    admin1: Option<String>,
    country: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmo_codes_fold_into_the_seven_conditions() {
        assert_eq!(Condition::from_wmo(0), Some(Condition::Clear));
        assert_eq!(Condition::from_wmo(2), Some(Condition::PartlyCloudy));
        assert_eq!(Condition::from_wmo(3), Some(Condition::Cloudy));
        assert_eq!(Condition::from_wmo(48), Some(Condition::Fog));
        assert_eq!(Condition::from_wmo(53), Some(Condition::Rain));
        assert_eq!(Condition::from_wmo(81), Some(Condition::Rain));
        assert_eq!(Condition::from_wmo(75), Some(Condition::Snow));
        assert_eq!(Condition::from_wmo(99), Some(Condition::Thunder));
        assert_eq!(Condition::from_wmo(42), None);
    }

    #[test]
    fn a_place_is_checked() {
        let ok = Place {
            name: "Paris".into(),
            region: None,
            country: Some("France".into()),
            latitude: 48.85,
            longitude: 2.35,
        };
        assert!(ok.validate().is_ok());
        assert!(Place { latitude: 91.0, ..ok.clone() }.validate().is_err());
        assert!(Place { name: "  ".into(), ..ok.clone() }.validate().is_err());
        assert!(Place { longitude: f64::NAN, ..ok }.validate().is_err());
    }
}
