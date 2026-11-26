use super::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Vecteurs de MS-NLMP §4.2.4 : utilisateur « User », domaine « Domain »,
/// mot de passe « Password ».
#[test]
fn les_empreintes_suivent_les_vecteurs_de_la_specification() {
    assert_eq!(hex(&nt_hash("Password")), "a4f49c406510bdcab6824ee7c30fd852");
    assert_eq!(hex(&ntlmv2_hash("Password", "User", "Domain")), "0c868a403bfd7a93a3001ef22ef02e3f");
}

#[test]
fn hmac_md5_donne_les_vecteurs_de_la_rfc_2202() {
    assert_eq!(hex(&hmac_md5(&[0x0b; 16], b"Hi There")), "9294727a3638bb1c13f48ef8158bfc9d");
}

#[test]
fn le_message_un_fait_trente_deux_octets() {
    let message = negotiate_message();
    assert_eq!(message.len(), 32);
    assert_eq!(&message[..8], b"NTLMSSP\0");
    assert_eq!(message[8], 1);
}

fn challenge_message() -> Vec<u8> {
    // En-tête de 48 octets, puis deux paires d'attributs et la fin.
    let mut info: Vec<u8> = vec![2, 0, 12, 0];
    info.extend(utf16le("Domain"));
    info.extend([1, 0, 12, 0]);
    info.extend(utf16le("Server"));
    info.extend([0, 0, 0, 0]);
    let mut out = Vec::new();
    out.extend_from_slice(SIGNATURE);
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&[0; 8]); // nom de cible
    out.extend_from_slice(&FLAGS.to_le_bytes());
    out.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&(info.len() as u16).to_le_bytes());
    out.extend_from_slice(&(info.len() as u16).to_le_bytes());
    out.extend_from_slice(&48u32.to_le_bytes());
    out.extend_from_slice(&info);
    out
}

#[test]
fn le_message_deux_est_lu() {
    let challenge = parse_challenge(&challenge_message()).expect("message valide");
    assert_eq!(challenge.server_challenge, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(challenge.target_info.len(), 36);
    assert!(parse_challenge(b"NTLMSSP\0").is_none());
    assert!(parse_challenge(&[0u8; 60]).is_none());
}

#[test]
fn le_message_trois_range_ses_champs_a_leurs_decalages() {
    let challenge = parse_challenge(&challenge_message()).unwrap();
    let message = authenticate_message(&challenge, "User", "Domain", "Password", [0xaa; 8], 0);
    assert_eq!(&message[..8], b"NTLMSSP\0");
    assert_eq!(message[8], 3);
    // Domaine : 6 caractères UTF-16, juste après l'en-tête de 64 octets, après LM (24) et NT.
    let nt_len = u16::from_le_bytes([message[20], message[21]]) as usize;
    // 16 (preuve) + 8 + 8 + 8 + 4 + 36 + 4 octets de blob.
    assert_eq!(nt_len, 16 + 68);
    let domain_offset = u32::from_le_bytes(message[32..36].try_into().unwrap()) as usize;
    assert_eq!(domain_offset, 64 + 24 + nt_len);
    assert_eq!(&message[domain_offset..domain_offset + 12], &utf16le("Domain")[..]);
    let user_offset = u32::from_le_bytes(message[40..44].try_into().unwrap()) as usize;
    assert_eq!(user_offset, domain_offset + 12);
    assert_eq!(&message[user_offset..user_offset + 8], &utf16le("User")[..]);
}

#[test]
fn l_horodatage_du_serveur_est_repris() {
    let mut info = vec![7, 0, 8, 0];
    info.extend_from_slice(&[9; 8]);
    info.extend_from_slice(&[0, 0, 0, 0]);
    assert_eq!(server_timestamp(&info), Some([9; 8]));
    assert_eq!(server_timestamp(&[0, 0, 0, 0]), None);
}

#[test]
fn filetime_part_de_1601() {
    assert_eq!(filetime(0), 116_444_736_000_000_000);
}
