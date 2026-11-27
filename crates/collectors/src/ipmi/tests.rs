use super::*;
use crate::rest::test_support::{find, value};

#[test]
fn valeur_et_unite() {
    assert_eq!(split_display("45.00 °C"), Some((45.0, "°C".to_string())));
    assert_eq!(split_display("12600.00 RPM"), Some((12_600.0, "RPM".to_string())));
    assert_eq!(split_display("n/a"), None);
}

#[test]
fn etat_du_chassis() {
    assert_eq!(parse_chassis(&[0x01, 0, 0]), (true, false, false));
    assert_eq!(parse_chassis(&[0x08]), (false, true, false));
    assert_eq!(parse_chassis(&[0x03]), (true, false, true));
}

#[test]
fn alimentations() {
    let ok = parse_psu("PS1", &[0x00, 0xC0, 0x01]).unwrap();
    assert!(ok.present && !ok.faulty);
    let broken = parse_psu("PS2", &[0x00, 0xC0, 0x03]).unwrap();
    assert!(broken.faulty);
    let lost = parse_psu("PS2", &[0x00, 0xC0, 0x09]).unwrap();
    assert!(lost.faulty, "input lost");
    assert_eq!(parse_psu("PS3", &[0x00, 0xE0, 0x00]), None, "reading unavailable");
}

#[test]
fn seuils() {
    assert!(within_limits(&[0x30, 0xC0, 0x08]), "upper non-critical is a warning only");
    assert!(!within_limits(&[0x30, 0xC0, 0x10]));
    assert!(!within_limits(&[0x30, 0xC0, 0x02]));
}

#[test]
fn adresse_et_port() {
    assert_eq!(split_address("10.0.0.5").unwrap(), ("10.0.0.5".to_string(), 623));
    assert_eq!(split_address("bmc.lan:6230").unwrap(), ("bmc.lan".to_string(), 6230));
    assert!(split_address(" ").is_err());
    assert!(split_address("host:abc").is_err());
}

fn analog(name: &str, value: f64, unit: &str, within_limits: bool) -> Analog {
    Analog { name: name.into(), value, unit: unit.into(), within_limits }
}

#[test]
fn mesures() {
    let readings = Readings {
        power_on: Some(true),
        analog: vec![
            analog("CPU1 Temp", 51.0, "°C", true),
            analog("FAN1", 8400.0, "RPM", false),
            analog("Odd", 3.0, "%", true),
        ],
        psus: vec![
            Psu { name: "PS1".into(), present: true, faulty: false },
            Psu { name: "PS2".into(), present: true, faulty: true },
        ],
        sel_entries: Some(12),
        ..Readings::default()
    };
    let s = samples(&readings, 1);
    assert_eq!(value(&s, "ipmi_chassis_power_on", &[]), 1.0);
    assert_eq!(value(&s, "ipmi_temperature_celsius", &[("sensor", "CPU1 Temp")]), 51.0);
    assert_eq!(value(&s, "ipmi_fan_rpm", &[("sensor", "FAN1")]), 8400.0);
    assert_eq!(value(&s, "ipmi_sensor_value", &[("sensor", "Odd"), ("unit", "%")]), 3.0);
    assert_eq!(value(&s, "ipmi_sensors_out_of_limits", &[]), 1.0);
    assert_eq!(value(&s, "ipmi_psus_faulty", &[]), 1.0);
    assert_eq!(value(&s, "ipmi_psu_ok", &[("psu", "PS2")]), 0.0);
    assert_eq!(value(&s, "ipmi_sel_entries", &[]), 12.0);
    assert!(find(&s, "ipmi_sel_overflow", &[]).is_some());
}

#[tokio::test]
async fn identifiants_absents_sont_une_erreur_de_configuration() {
    let target = crate::uptime::tags::test_support::cible("ipmi", "10.0.0.5", &[]);
    let error = IpmiCollector::new().probe(&target).await.unwrap_err();
    assert!(matches!(error, ProbeError::Config(_)));
}
