use super::*;
use serde_json::json;

fn nested(levels: usize) -> String {
    format!("{}{}", "[".repeat(levels), "]".repeat(levels))
}

#[test]
fn un_document_ordinaire_passe() {
    let value: Value = from_slice(br#"{"a": [1, {"b": "x{[["}]}"#).unwrap();
    assert_eq!(value["a"][1]["b"], "x{[[");
    let back: Value = from_value(value).unwrap();
    assert_eq!(back["a"][0], 1);
}

#[test]
fn la_profondeur_est_bornee() {
    assert!(from_slice::<Value>(nested(MAX_DEPTH).as_bytes()).is_ok());
    assert!(matches!(from_slice::<Value>(nested(MAX_DEPTH + 1).as_bytes()), Err(Rejected::TooDeep)));
    let deep: Value = serde_json::from_str(&nested(MAX_DEPTH + 1)).unwrap();
    assert!(matches!(from_value::<Value>(deep), Err(Rejected::TooDeep)));
    let ok: Value = serde_json::from_str(&nested(MAX_DEPTH)).unwrap();
    assert!(from_value::<Value>(ok).is_ok());
}

#[test]
fn les_accolades_dans_une_chaine_ne_comptent_pas() {
    let text = format!(r#"{{"s": "{}\"{}"}}"#, "[".repeat(100), "{".repeat(100));
    assert!(check_bytes(text.as_bytes()).is_ok());
}

#[test]
fn la_taille_est_bornee() {
    let big = vec![b' '; MAX_BYTES + 1];
    assert!(matches!(from_slice::<Value>(&big), Err(Rejected::TooLarge)));
    let wide = Value::Array(vec![json!(0); 10_000]);
    assert!(check_value(&wide).is_ok());
}

#[test]
fn le_json_invalide_garde_le_detail_de_serde() {
    let error = from_slice::<Value>(b"{oops").unwrap_err();
    assert!(matches!(error, Rejected::Invalid(_)));
    assert!(!error.to_string().is_empty());
}
