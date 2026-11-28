//! IPMI v2.0 sur le réseau (RMCP+, UDP 623) : contrôleurs de gestion (BMC) des
//! serveurs. Collecteur en Rust pur (`ipmi-rs`, MIT OR Apache-2.0) : aucun
//! `ipmitool` n'est lancé, aucun programme externe n'est appelé.
//!
//! Lectures, une session par passage (ouverte puis abandonnée, le BMC les
//! expire) :
//!
//! * **état du châssis** (`Get Chassis Status`) : alimentation allumée, défaut,
//!   surcharge ;
//! * **capteurs** du dépôt SDR : températures, ventilateurs (tr/min), tensions,
//!   courants, puissances (valeur et état de seuil), et les alimentations
//!   (capteurs discrets : présence et défaut) ;
//! * **journal d'événements** (SEL) : nombre d'entrées, débordement.
//!
//! Les sessions IPMI sont bloquantes : toute la lecture se fait sur un fil
//! dédié (`spawn_blocking`) avec une échéance globale. Un BMC lent ou muet
//! est une panne (`Unreachable`) ; un refus d'identifiants est une erreur
//! d'authentification.
//!
//! Réglages, portés par les étiquettes de la cible : `request_timeout_seconds`
//! (délai par échange, 5 s par défaut), `port` (623).
//!
//! Beta : validé sur la spécification, pas sur du matériel. Un BMC moderne qui
//! sert Redfish est mieux lu par le collecteur `redfish`.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use dumbmonit_proto::{Collector, Credential, ProbeError, Sample, Target};
use ipmi_rs::Ipmi;
use ipmi_rs::connection::{Address, Channel, IpmiCommand, Message, NetFn, NotEnoughData};
use ipmi_rs::rmcp::Rmcp;
use ipmi_rs::storage::sdr::SensorType;
use ipmi_rs::storage::sdr::record::{
    IdentifiableSensor, InstancedSensor, RecordContents, SensorKey, WithSensorRecordCommon,
};
use ipmi_rs::storage::sel::GetSelInfo;

use crate::cloud;
use crate::rest::{MAX_NAMED, flag, gauge, now_ms};
use crate::selfhosted::options::tag;

pub const DEFAULT_PORT: u16 = 623;
const PREFIX: &str = "ipmi";
/// Échéance globale d'une sonde : le dépôt SDR d'un gros serveur compte des
/// centaines de capteurs, lus un par un.
const DEADLINE: Duration = Duration::from_secs(45);

#[derive(Default)]
pub struct IpmiCollector;

impl IpmiCollector {
    pub fn new() -> Self {
        Self
    }
}

/// `Get Sensor Reading` (0x2D) rendu brut : la bibliothèque n'expose pas les
/// états des capteurs discrets (alimentations), on lit donc les octets.
struct RawReading(SensorKey);

impl From<RawReading> for Message {
    fn from(value: RawReading) -> Self {
        Message::new_request(NetFn::SensorEvent, 0x2D, vec![value.0.sensor_number.get()])
    }
}

impl IpmiCommand for RawReading {
    type Output = Vec<u8>;
    type Error = NotEnoughData;

    fn parse_success_response(data: &[u8]) -> Result<Self::Output, Self::Error> {
        if data.len() < 2 { Err(NotEnoughData) } else { Ok(data.to_vec()) }
    }

    fn target(&self) -> Option<(Address, Channel)> {
        Some((Address(self.0.owner_id.into()), self.0.owner_channel))
    }
}

/// `Get Chassis Status` (0x00 0x01).
struct ChassisStatus;

impl From<ChassisStatus> for Message {
    fn from(_: ChassisStatus) -> Self {
        Message::new_request(NetFn::Chassis, 0x01, Vec::new())
    }
}

impl IpmiCommand for ChassisStatus {
    type Output = Vec<u8>;
    type Error = NotEnoughData;

    fn parse_success_response(data: &[u8]) -> Result<Self::Output, Self::Error> {
        if data.is_empty() { Err(NotEnoughData) } else { Ok(data.to_vec()) }
    }
}

/// Un capteur analogique lu.
#[derive(Debug, Clone, PartialEq)]
pub struct Analog {
    pub name: String,
    pub value: f64,
    pub unit: String,
    /// Faux si un seuil critique ou non récupérable est franchi.
    pub within_limits: bool,
}

/// Une alimentation (capteur discret de type « Power Supply »).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Psu {
    pub name: String,
    pub present: bool,
    pub faulty: bool,
}

/// Ce que la sonde a lu.
#[derive(Debug, Default)]
pub struct Readings {
    pub power_on: Option<bool>,
    pub power_fault: bool,
    pub power_overload: bool,
    pub analog: Vec<Analog>,
    pub psus: Vec<Psu>,
    pub sel_entries: Option<u16>,
    pub sel_overflow: bool,
    /// Vrai si l'échéance a coupé la lecture des capteurs.
    pub truncated: bool,
}

/// « 45.00 °C » → (45.0, "°C").
fn split_display(display: &str) -> Option<(f64, String)> {
    let mut parts = display.split_whitespace();
    let value: f64 = parts.next()?.parse().ok()?;
    Some((value, parts.collect::<Vec<_>>().join(" ")))
}

/// Octets de `Get Chassis Status` : bit 0 alimentation, 1 surcharge, 3 défaut.
fn parse_chassis(data: &[u8]) -> (bool, bool, bool) {
    let b = data.first().copied().unwrap_or(0);
    (b & 0x01 != 0, b & 0x08 != 0, b & 0x02 != 0)
}

/// Octets de `Get Sensor Reading` d'un capteur d'alimentation (états discrets) :
/// bit 0 présente ; 1 défaut ; 2 défaillance prévue ; 3 à 6 entrée perdue,
/// hors plage ou erreur de configuration.
fn parse_psu(name: &str, data: &[u8]) -> Option<Psu> {
    // Bit 5 de l'octet 1 : lecture indisponible.
    if data.get(1)? & 0x20 != 0 {
        return None;
    }
    let states = *data.get(2)?;
    Some(Psu { name: name.to_string(), present: states & 0x01 != 0, faulty: states & 0x7E != 0 })
}

/// Seuils critiques ou non récupérables : octet 2 de la lecture d'un capteur à seuils.
fn within_limits(data: &[u8]) -> bool {
    // 0x20 non récupérable haut, 0x10 critique haut, 0x04 non récupérable bas, 0x02 critique bas.
    data.get(2).is_none_or(|b| b & 0x36 == 0)
}

fn io_error(error: &std::io::Error) -> ProbeError {
    ProbeError::Unreachable(format!("The BMC does not answer on UDP 623: {error}"))
}

/// Ouvre la session (RMCP+ si le BMC le sert) et lit tout. Bloquant.
fn read_all(
    address: &str,
    port: u16,
    user: &str,
    password: &str,
    timeout: Duration,
) -> Result<Readings, ProbeError> {
    let started = Instant::now();
    let mut rmcp = Rmcp::new(format!("{address}:{port}"), timeout).map_err(|e| io_error(&e))?;
    rmcp.activate(true, Some(user), Some(password.as_bytes())).map_err(|error| {
        let text = format!("{error:?}");
        if text.contains("Io(")
            || text.contains("PingSend")
            || text.contains("PongReceive")
            || text.contains("Receive(")
        {
            ProbeError::Unreachable(format!(
                "The BMC does not answer on UDP {port}: enable IPMI over LAN and allow the port \
                 ({text})"
            ))
        } else if text.contains("IpmiNotSupported") {
            ProbeError::Config(format!("{address}:{port} does not speak IPMI over LAN."))
        } else {
            ProbeError::Auth(format!(
                "The BMC refused the session: check the user name, the password and that the \
                 user may use IPMI over LAN with the privilege level USER or above ({text})"
            ))
        }
    })?;
    let mut ipmi = Ipmi::new(rmcp);
    let mut out = Readings::default();

    if let Ok(data) = ipmi.send_recv(ChassisStatus) {
        let (on, fault, overload) = parse_chassis(&data);
        out.power_on = Some(on);
        out.power_fault = fault;
        out.power_overload = overload;
    }
    if let Ok(info) = ipmi.send_recv(GetSelInfo) {
        out.sel_entries = Some(info.entries);
        out.sel_overflow = info.overflow;
    }

    let records: Vec<_> = ipmi.sdrs().collect();
    for record in records {
        if started.elapsed() > DEADLINE {
            out.truncated = true;
            break;
        }
        match record.contents {
            RecordContents::FullSensor(full) => {
                let ty = full.common().ty;
                if ty == SensorType::PowerSupply {
                    let name = full.id_string().to_string();
                    read_psu(&mut ipmi, name, *full.key_data(), &mut out);
                    continue;
                }
                if out.analog.len() >= MAX_NAMED {
                    continue;
                }
                let Ok(data) = ipmi.send_recv(RawReading(*full.key_data())) else { continue };
                if data[1] & 0x20 != 0 || data[1] & 0x40 == 0 {
                    continue;
                }
                let Some(display) = full.display_reading(data[0]) else { continue };
                if let Some((value, unit)) = split_display(&display) {
                    out.analog.push(Analog {
                        name: full.id_string().to_string(),
                        value,
                        unit,
                        within_limits: within_limits(&data),
                    });
                }
            }
            RecordContents::CompactSensor(compact)
                if compact.common().ty == SensorType::PowerSupply =>
            {
                let name = compact.id_string().to_string();
                read_psu(&mut ipmi, name, *compact.key_data(), &mut out);
            }
            _ => {}
        }
    }
    Ok(out)
}

fn read_psu(ipmi: &mut Ipmi<Rmcp>, name: String, key: SensorKey, out: &mut Readings) {
    if out.psus.len() >= MAX_NAMED {
        return;
    }
    if let Ok(data) = ipmi.send_recv(RawReading(key)) {
        out.psus.extend(parse_psu(&name, &data));
    }
}

#[async_trait]
impl Collector for IpmiCollector {
    fn kind(&self) -> &'static str {
        "ipmi"
    }

    async fn probe(&self, target: &Target) -> Result<Vec<Sample>, ProbeError> {
        let (user, password) = match &target.credential {
            Credential::UsernamePassword { username, password } if !username.trim().is_empty() => {
                (username.trim().to_string(), password.clone())
            }
            other => {
                return Err(ProbeError::Config(format!(
                    "IPMI expects a BMC user name and password, configured: {other}"
                )));
            }
        };
        let (address, port) = split_address(&target.address)?;
        let timeout = tag(target, "request_timeout_seconds")
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(Duration::from_secs(5), |s| Duration::from_secs(s.clamp(1, 60)));
        let readings = tokio::task::spawn_blocking(move || {
            read_all(&address, port, &user, &password, timeout)
        })
        .await
        .map_err(|e| ProbeError::Protocol(format!("The IPMI read was interrupted: {e}")))??;
        Ok(samples(&readings, now_ms()))
    }

    async fn discover(&self, target: &Target) -> Result<Option<String>, ProbeError> {
        self.probe(target).await?;
        Ok(Some("ipmi".to_string()))
    }
}

/// `hôte` ou `hôte:port` (le port par défaut est 623).
fn split_address(raw: &str) -> Result<(String, u16), ProbeError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ProbeError::Config("The BMC address is required".into()));
    }
    if let Some((host, port)) = raw.rsplit_once(':').filter(|(h, _)| !h.contains(':')) {
        let port =
            port.parse().map_err(|_| ProbeError::Config(format!("Invalid port in \"{raw}\"")))?;
        return Ok((host.to_string(), port));
    }
    Ok((raw.to_string(), DEFAULT_PORT))
}

/// Nom de série et unité normalisée d'une valeur analogique.
fn family(unit: &str) -> Option<&'static str> {
    Some(match unit.trim() {
        "°C" | "C" => "temperature_celsius",
        "RPM" => "fan_rpm",
        "V" => "voltage_volts",
        "A" => "current_amps",
        "W" => "power_watts",
        _ => return None,
    })
}

pub fn samples(r: &Readings, ts_ms: i64) -> Vec<Sample> {
    let g = |name: &str, value: f64| gauge(&format!("{PREFIX}_{name}"), value, ts_ms);
    let mut out = Vec::new();
    if let Some(on) = r.power_on {
        out.push(flag(&format!("{PREFIX}_chassis_power_on"), on, ts_ms));
        out.push(flag(&format!("{PREFIX}_chassis_power_fault"), r.power_fault, ts_ms));
        out.push(flag(&format!("{PREFIX}_chassis_power_overload"), r.power_overload, ts_ms));
        let state = if on { "On" } else { "Off" };
        let healthy = on && !r.power_fault;
        out.push(cloud::item(PREFIX, "Chassis", "Power", healthy, state, ts_ms));
    }
    let mut out_of_limits = 0.0;
    for sensor in &r.analog {
        let series = family(&sensor.unit).unwrap_or("sensor_value");
        let mut sample = g(series, sensor.value).with_label("sensor", &sensor.name);
        if series == "sensor_value" {
            sample = sample.with_label("unit", &sensor.unit);
        }
        out.push(sample);
        let ok = f64::from(sensor.within_limits);
        out.push(g("sensor_ok", ok).with_label("sensor", &sensor.name));
        if !sensor.within_limits {
            out_of_limits += 1.0;
            out.push(cloud::item(PREFIX, "Sensors", &sensor.name, false, "Out of limits", ts_ms));
        }
    }
    out.push(g("sensors", r.analog.len() as f64));
    out.push(g("sensors_out_of_limits", out_of_limits));

    let mut faulty = 0.0;
    for psu in &r.psus {
        out.push(g("psu_present", f64::from(psu.present)).with_label("psu", &psu.name));
        out.push(g("psu_ok", f64::from(!psu.faulty)).with_label("psu", &psu.name));
        if psu.faulty {
            faulty += 1.0;
            out.push(cloud::item(PREFIX, "Power supplies", &psu.name, false, "Fault", ts_ms));
        }
    }
    out.push(g("psus_faulty", faulty));

    if let Some(entries) = r.sel_entries {
        out.push(g("sel_entries", f64::from(entries)));
        out.push(flag(&format!("{PREFIX}_sel_overflow"), r.sel_overflow, ts_ms));
        out.push(cloud::figure(PREFIX, "Event log entries", "", f64::from(entries), ts_ms));
    }
    out.push(flag(&format!("{PREFIX}_read_truncated"), r.truncated, ts_ms));
    out.push(cloud::figure(PREFIX, "Out of limits", "", out_of_limits + faulty, ts_ms));
    out
}

#[cfg(test)]
mod tests;
