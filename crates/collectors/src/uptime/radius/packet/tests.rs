use super::*;

const SECRET: &[u8] = b"testing123";
const AUTH: [u8; 16] = *b"0123456789abcdef";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn md5_donne_les_vecteurs_de_la_rfc_1321() {
    assert_eq!(hex(&md5(&[b""])), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(hex(&md5(&[b"abc"])), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(hex(&md5(&[b"a", b"bc"])), "900150983cd24fb0d6963f7d28e17f72");
}

#[test]
fn hmac_md5_donne_les_vecteurs_de_la_rfc_2202() {
    assert_eq!(hex(&hmac_md5(&[0x0b; 16], b"Hi There")), "9294727a3638bb1c13f48ef8158bfc9d");
    assert_eq!(
        hex(&hmac_md5(b"Jefe", b"what do ya want for nothing?")),
        "750c783e6ab0b503eaa86e310a5db738"
    );
    // Clé plus longue que le bloc : elle est d'abord condensée.
    assert_eq!(
        hex(&hmac_md5(&[0xaa; 80], b"Test Using Larger Than Block-Size Key - Hash Key First")),
        "6b1ab7fe4bd7bf8f0b62e6ce61b9d0cd"
    );
}

#[test]
fn le_mot_de_passe_masque_se_demasque_a_lidentique() {
    let long = "x".repeat(128);
    for password in ["", "a", "motdepasse", "exactement16octet", long.as_str()] {
        let hidden = hide_password(password.as_bytes(), SECRET, &AUTH);
        assert_eq!(hidden.len() % 16, 0);
        assert!(hidden.len() >= 16);
        assert_eq!(reveal_password(&hidden, SECRET, &AUTH), password.as_bytes());
    }
}

#[test]
fn le_premier_bloc_du_mot_de_passe_suit_la_rfc_2865() {
    // b1 = MD5(secret + authentificateur), c1 = p1 XOR b1.
    let key = md5(&[SECRET, &AUTH]);
    let hidden = hide_password(b"arctangent", SECRET, &AUTH);
    let mut padded = b"arctangent".to_vec();
    padded.resize(16, 0);
    let expected: Vec<u8> = padded.iter().zip(key).map(|(p, k)| p ^ k).collect();
    assert_eq!(hidden, expected);
}

#[test]
fn la_requete_est_bien_formee_et_signee() {
    let packet = encode_access_request(7, &AUTH, SECRET, "alice", "hunter2", "dumbmonit")
        .expect("requête valide");
    assert_eq!(packet[0], ACCESS_REQUEST);
    assert_eq!(packet[1], 7);
    assert_eq!(usize::from(u16::from_be_bytes([packet[2], packet[3]])), packet.len());
    assert_eq!(&packet[4..20], &AUTH);

    let parsed = parse_request(&packet, SECRET).unwrap();
    assert_eq!(parsed.username, "alice");
    assert_eq!(parsed.password, "hunter2");
    assert_eq!(parsed.nas_identifier, "dumbmonit");
    assert!(parsed.signature_valid);
    assert!(parsed.signature_first, "le Message-Authenticator vient en premier");

    // Un autre secret ne valide pas la signature.
    assert!(!parse_request(&packet, b"autre").unwrap().signature_valid);
}

#[test]
fn une_requete_invalide_est_refusee_avant_lenvoi() {
    assert!(encode_access_request(1, &AUTH, b"", "alice", "pw", "n").is_err());
    assert!(encode_access_request(1, &AUTH, SECRET, "", "pw", "n").is_err());
    assert!(encode_access_request(1, &AUTH, SECRET, &"u".repeat(254), "pw", "n").is_err());
    assert!(encode_access_request(1, &AUTH, SECRET, "alice", &"p".repeat(129), "n").is_err());
    // Sans identifiant de NAS, l'attribut est simplement omis.
    let packet = encode_access_request(1, &AUTH, SECRET, "alice", "pw", "").unwrap();
    assert_eq!(parse_request(&packet, SECRET).unwrap().nas_identifier, "");
}

#[test]
fn une_reponse_signee_par_le_bon_secret_est_acceptee() {
    let datagram =
        build_response(ACCESS_ACCEPT, 9, &AUTH, SECRET, &[(18, b"Welcome".to_vec())]);
    let response = decode_response(&datagram, &AUTH, 9, SECRET).expect("réponse valide");
    assert_eq!(response.code, ACCESS_ACCEPT);
    assert_eq!(response.reply_message().as_deref(), Some("Welcome"));
}

#[test]
fn une_reponse_signee_par_un_autre_secret_est_reperee() {
    let datagram = build_response(ACCESS_ACCEPT, 9, &AUTH, b"autre-secret", &[]);
    let error = decode_response(&datagram, &AUTH, 9, SECRET).unwrap_err();
    assert!(matches!(error, DecodeError::BadAuthenticator(_)), "{error:?}");
}

#[test]
fn une_reponse_a_une_autre_requete_est_ecartee() {
    let datagram = build_response(ACCESS_ACCEPT, 9, &AUTH, SECRET, &[]);
    assert_eq!(
        decode_response(&datagram, &AUTH, 10, SECRET).unwrap_err(),
        DecodeError::IdMismatch
    );
    // Mauvais authentificateur de requête : la signature ne tient plus.
    let error = decode_response(&datagram, &[0u8; 16], 9, SECRET).unwrap_err();
    assert!(matches!(error, DecodeError::BadAuthenticator(_)));
}

#[test]
fn un_message_authenticator_falsifie_est_reperee() {
    // La signature de réponse (MD5) est valide, pas le HMAC (des zéros).
    let datagram = build_response(ACCESS_ACCEPT, 9, &AUTH, SECRET, &[(80, vec![0u8; 16])]);
    let error = decode_response(&datagram, &AUTH, 9, SECRET).unwrap_err();
    match error {
        DecodeError::BadAuthenticator(detail) => assert!(detail.contains("Message-Auth")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn un_message_authenticator_correct_est_accepte() {
    let mut datagram = build_response(ACCESS_ACCEPT, 9, &AUTH, SECRET, &[(80, vec![0u8; 16])]);
    // Le serveur signe d'abord avec l'authentificateur de la requête, puis
    // calcule l'authentificateur de réponse sur le paquet final.
    let mut to_sign = datagram.clone();
    to_sign[4..20].copy_from_slice(&AUTH);
    let hmac = hmac_md5(SECRET, &to_sign);
    datagram[22..38].copy_from_slice(&hmac);
    let signature = md5(&[&datagram[..4], &AUTH, &datagram[20..], SECRET]);
    datagram[4..20].copy_from_slice(&signature);
    assert!(decode_response(&datagram, &AUTH, 9, SECRET).is_ok());
}

#[test]
fn un_paquet_tronque_ou_incoherent_est_mal_forme() {
    let valid = build_response(ACCESS_ACCEPT, 9, &AUTH, SECRET, &[(18, b"hello".to_vec())]);
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        valid[..10].to_vec(),
        valid[..valid.len() - 3].to_vec(),
        {
            let mut bad = valid.clone();
            bad[3] = 2; // longueur annoncée inférieure à l'en-tête
            bad
        },
        {
            let mut bad = valid.clone();
            bad[21] = 1; // longueur d'attribut impossible
            bad
        },
    ];
    for datagram in cases {
        let error = decode_response(&datagram, &AUTH, 9, SECRET).unwrap_err();
        assert!(matches!(error, DecodeError::Malformed(_)), "{error:?}");
    }
}

#[test]
fn le_reply_message_est_nettoye() {
    let response = Response {
        code: ACCESS_REJECT,
        attributes: vec![(18, b"  Account\r\nlocked\x07  ".to_vec())],
    };
    assert_eq!(response.reply_message().as_deref(), Some("Account  locked"));
    let none = Response { code: ACCESS_REJECT, attributes: vec![] };
    assert!(none.reply_message().is_none());
}
