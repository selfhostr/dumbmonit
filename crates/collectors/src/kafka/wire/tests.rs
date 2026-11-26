use super::*;

#[test]
fn la_metadonnee_est_lue() {
    let cluster = parse_metadata(&sample_metadata()).unwrap();
    assert_eq!(cluster.cluster_id.as_deref(), Some("cluster-abc"));
    assert_eq!(cluster.controller, 1);
    assert_eq!(cluster.brokers.len(), 2);
    assert_eq!(
        cluster.brokers[1],
        BrokerInfo { id: 2, host: "kafka-2.lan".into(), port: 9092 }
    );
    let orders = &cluster.topics[0];
    assert_eq!((orders.name.as_str(), orders.internal), ("orders", false));
    assert_eq!(orders.partitions[1].leader, -1);
    assert_eq!(orders.partitions[1].in_sync, 1);
    let internal = &cluster.topics[1];
    assert!(internal.internal);
    assert_eq!((internal.partitions[0].replicas, internal.partitions[0].in_sync), (2, 1));
}

#[test]
fn une_reponse_tronquee_est_une_erreur_de_protocole() {
    let full = sample_metadata();
    for cut in [0, 3, 10, full.len() - 1] {
        let error = parse_metadata(&full[..cut]).unwrap_err();
        assert!(matches!(error, ProbeError::Protocol(_)), "{cut}");
    }
    // Une longueur de tableau délirante ne réserve rien.
    let mut hostile = Writer::new();
    hostile.i32(0).i32(i32::MAX);
    assert!(parse_metadata(&hostile.finish()).is_err());
}

#[test]
fn l_en_tete_et_la_longueur_de_la_requete() {
    let body = metadata_request();
    let bytes = frame(API_METADATA, VERSION_METADATA, 7, &body);
    let length = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    assert_eq!(length, bytes.len() - 4);
    assert_eq!(&bytes[4..6], &API_METADATA.to_be_bytes());
    assert_eq!(&bytes[6..8], &VERSION_METADATA.to_be_bytes());
    assert_eq!(&bytes[8..12], &7i32.to_be_bytes());
    // « dumbmonit » : longueur sur deux octets, puis le texte.
    assert_eq!(&bytes[12..14], &9i16.to_be_bytes());
    assert_eq!(&bytes[14..23], b"dumbmonit");
    assert_eq!(&bytes[23..], &body[..]);
    // Toutes les rubriques (-1), sans création automatique.
    assert_eq!(body, [0xff, 0xff, 0xff, 0xff, 0]);
}

#[test]
fn les_groupes_et_les_offsets() {
    let mut w = Writer::new();
    w.i16(0).i32(2);
    w.string("billing").string("consumer");
    w.string("connect-sink").string("sink");
    let (error, groups) = parse_list_groups(&w.finish()).unwrap();
    assert_eq!(error, 0);
    assert_eq!(groups[0], ("billing".to_string(), "consumer".to_string()));

    // OffsetFetch : une rubrique, deux partitions (une sans offset), erreur globale 0.
    let mut w = Writer::new();
    w.i32(1).string("orders").i32(2);
    w.i32(0).i64(120).null_string().i16(0);
    w.i32(1).i64(-1).null_string().i16(0);
    w.i16(0);
    let (error, offsets) = parse_offset_fetch(&w.finish()).unwrap();
    assert_eq!(error, 0);
    assert_eq!(offsets, vec![("orders".to_string(), 0, 120)]);
}

#[test]
fn list_offsets_regroupe_par_rubrique() {
    let request = list_offsets_request(&[
        ("orders".into(), 0),
        ("orders".into(), 1),
        ("payments".into(), 0),
    ]);
    let mut r = Reader::new(&request);
    assert_eq!(r.i32().unwrap(), -1);
    assert_eq!(r.i32().unwrap(), 2);
    assert_eq!(r.string().unwrap(), "orders");
    assert_eq!(r.i32().unwrap(), 2);

    let mut w = Writer::new();
    w.i32(1).string("orders").i32(2);
    w.i32(0).i16(0).i64(-1).i64(500);
    w.i32(1).i16(6).i64(-1).i64(-1);
    assert_eq!(parse_list_offsets(&w.finish()).unwrap(), vec![("orders".to_string(), 0, 500)]);
}

#[test]
fn sasl_plain_et_son_resultat() {
    let request = sasl_plain_request("dumbmonit", "secret");
    assert_eq!(&request[..4], &(17i32).to_be_bytes());
    assert_eq!(&request[4..], b"\0dumbmonit\0secret");
    let mut w = Writer::new();
    w.i16(ERROR_SASL_AUTHENTICATION_FAILED).string("Authentication failed").bytes(&[]);
    let (error, message) = parse_sasl_authenticate(&w.finish()).unwrap();
    assert_eq!(error, ERROR_SASL_AUTHENTICATION_FAILED);
    assert_eq!(message.as_deref(), Some("Authentication failed"));
}
