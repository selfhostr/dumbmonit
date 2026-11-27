//! Copies hors site des sauvegardes locales, contre de faux serveurs S3 et
//! WebDAV montés en local : envoi, élagage, essai, échec sans dommage pour
//! la copie locale, et voyage des destinations dans le lot.
//!
//! Le faux S3 recalcule la signature SigV4 de chaque requête comme le ferait
//! AWS, et l'empreinte du corps : une requête mal signée y est refusée.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use serde_json::{Value, json};
use sqlx::SqlitePool;

use dumbmonit_server::backup::local;
use dumbmonit_server::backup::offsite::sigv4::{self, Signer};
use dumbmonit_server::backup::offsite::store::{self, Input};
use dumbmonit_server::backup::offsite::transfer::percent_decode;
use dumbmonit_server::backup::offsite::{self, Kind, Retry, Secrets, Settings};
use dumbmonit_server::backup::{ExportOptions, collect, restore};
use dumbmonit_server::crypto::Cipher;
use dumbmonit_server::db;

const ACCESS_KEY: &str = "AKTEST";
const SECRET_KEY: &str = "s3cr3t-key";
const DAV_USER: &str = "backup";
const DAV_PASSWORD: &str = "app-password";

type Objects = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    base
}

// --------------------------------------------------------------------------
// Faux S3 (requêtes en chemin : /seau/clé)
// --------------------------------------------------------------------------

fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers.get(name).and_then(|value| value.to_str().ok()).unwrap_or("")
}

fn s3_error(status: StatusCode, code: &str) -> Response {
    let body = format!("<Error><Code>{code}</Code><Message>refused</Message></Error>");
    (status, body).into_response()
}

async fn fake_s3(
    State(objects): State<Objects>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let authorization = header(&headers, "authorization");
    let signed_names = authorization
        .split("SignedHeaders=")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .unwrap_or("");
    let signed: Vec<(String, String)> = signed_names
        .split(';')
        .map(|name| (name.to_string(), header(&headers, name).to_string()))
        .collect();
    let query: Vec<(String, String)> = uri
        .query()
        .unwrap_or("")
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect();
    let payload_hash = header(&headers, "x-amz-content-sha256");
    let signer = Signer {
        access_key: ACCESS_KEY,
        secret_key: SECRET_KEY,
        region: "us-east-1",
        service: "s3",
    };
    let request = sigv4::Request {
        method: method.as_str(),
        canonical_uri: uri.path(),
        query: &query,
        headers: &signed,
        payload_hash,
    };
    if signer.authorization(&request, header(&headers, "x-amz-date")) != authorization {
        return s3_error(StatusCode::FORBIDDEN, "SignatureDoesNotMatch");
    }
    if !signed_names.contains("host") || sigv4::sha256_hex(&body) != payload_hash {
        return s3_error(StatusCode::BAD_REQUEST, "BadDigest");
    }

    let Some(key) = uri.path().strip_prefix("/backups/").map(percent_decode) else {
        return s3_error(StatusCode::NOT_FOUND, "NoSuchBucket");
    };
    let mut objects = objects.lock().unwrap();
    match method {
        Method::PUT => {
            objects.insert(key, body.to_vec());
            StatusCode::OK.into_response()
        }
        Method::DELETE => {
            objects.remove(&key);
            StatusCode::NO_CONTENT.into_response()
        }
        Method::GET if key.is_empty() => {
            let prefix = query
                .iter()
                .find(|(name, _)| name == "prefix")
                .map(|(_, value)| value.clone())
                .unwrap_or_default();
            let contents: String = objects
                .keys()
                .filter(|key| key.strip_prefix(&prefix).is_some_and(|rest| !rest.contains('/')))
                .map(|key| format!("<Contents><Key>{key}</Key></Contents>"))
                .collect();
            let body = format!(
                "<?xml version=\"1.0\"?><ListBucketResult \
                 xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">{contents}\
                 <IsTruncated>false</IsTruncated></ListBucketResult>"
            );
            (StatusCode::OK, body).into_response()
        }
        _ => s3_error(StatusCode::METHOD_NOT_ALLOWED, "MethodNotAllowed"),
    }
}

async fn s3_server(objects: Objects) -> String {
    let router =
        Router::new().fallback(fake_s3).layer(DefaultBodyLimit::disable()).with_state(objects);
    serve(router).await
}

// --------------------------------------------------------------------------
// Faux WebDAV (dossier /dav/backups/)
// --------------------------------------------------------------------------

async fn fake_dav(
    State(files): State<Objects>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let expected =
        base64::engine::general_purpose::STANDARD.encode(format!("{DAV_USER}:{DAV_PASSWORD}"));
    if header(&headers, "authorization") != format!("Basic {expected}") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let path = uri.path().to_string();
    let mut files = files.lock().unwrap();
    match method.as_str() {
        "PROPFIND" if path == "/dav/backups/" && header(&headers, "depth") == "1" => {
            let mut body = String::from(
                "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">\
                 <d:response><d:href>/dav/backups/</d:href></d:response>",
            );
            for name in files.keys() {
                body.push_str(&format!(
                    "<d:response><d:href>/dav/backups/{name}</d:href></d:response>"
                ));
            }
            body.push_str("</d:multistatus>");
            (StatusCode::MULTI_STATUS, body).into_response()
        }
        "PUT" => match path.strip_prefix("/dav/backups/") {
            Some(name) if !name.contains('/') => {
                files.insert(percent_decode(name), body.to_vec());
                StatusCode::CREATED.into_response()
            }
            _ => StatusCode::CONFLICT.into_response(),
        },
        "DELETE" => match path.strip_prefix("/dav/backups/") {
            Some(name) if files.remove(&percent_decode(name)).is_some() => {
                StatusCode::NO_CONTENT.into_response()
            }
            _ => StatusCode::NOT_FOUND.into_response(),
        },
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn dav_server(files: Objects) -> String {
    let router =
        Router::new().fallback(fake_dav).layer(DefaultBodyLimit::disable()).with_state(files);
    serve(router).await
}

// --------------------------------------------------------------------------
// Instance et sauvegarde locale
// --------------------------------------------------------------------------

struct Instance {
    pool: SqlitePool,
    cipher: Cipher,
    settings: local::Settings,
    _dir: tempfile::TempDir,
}

async fn instance() -> Instance {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.unwrap();
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.unwrap();
    let secret = dir.path().join("secret.key");
    tokio::fs::write(&secret, "secret-de-test-suffisamment-long").await.unwrap();
    let settings = local::Settings {
        enabled: true,
        directory: dir.path().join("backups"),
        interval: Duration::from_secs(3600),
        keep: 7,
        database: dir.path().join("dumbmonit.db"),
        secret_file: Some(secret),
    };
    Instance { pool, cipher, settings, _dir: dir }
}

fn secrets(pairs: &[(&str, &str)]) -> Secrets {
    pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

async fn add(
    instance: &Instance,
    name: &str,
    kind: Kind,
    settings: Value,
    keep: u32,
    include_key: bool,
    secrets: &Secrets,
) -> i64 {
    let settings = Settings::parse(kind, &settings).unwrap();
    settings.validate().unwrap();
    settings.check_secrets(secrets).unwrap();
    let input = Input { name, enabled: true, keep, include_key, settings: &settings };
    store::insert(&instance.pool, &instance.cipher, &input, secrets).await.unwrap()
}

fn s3_settings(base: &str) -> Value {
    json!({
        "endpoint": base,
        "bucket": "backups",
        "prefix": "home",
        "access_key_id": ACCESS_KEY,
        "path_style": true
    })
}

async fn local_file(directory: &Path, name: &str) -> Vec<u8> {
    tokio::fs::read(directory.join(name)).await.unwrap()
}

// --------------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------------

#[tokio::test]
async fn a_backup_goes_to_s3_with_its_key_and_old_copies_are_pruned() {
    let objects: Objects = Arc::default();
    {
        let mut objects = objects.lock().unwrap();
        for key in [
            "home/dumbmonit-20200101-000000.db",
            "home/dumbmonit-20200101-000000.key",
            "home/dumbmonit-20200102-000000.db",
            "home/notes.txt",
            "other/dumbmonit-20200101-000000.db",
        ] {
            objects.insert(key.to_string(), b"old".to_vec());
        }
    }
    let base = s3_server(objects.clone()).await;
    let instance = instance().await;
    let id = add(
        &instance,
        "Bucket",
        Kind::S3,
        s3_settings(&base),
        2,
        true,
        &secrets(&[("secret_access_key", SECRET_KEY)]),
    )
    .await;

    let record = local::run_once(&instance.pool, &instance.settings).await;
    assert!(record.ok && record.with_secret, "{record:?}");
    let results = offsite::push_all(
        &instance.pool,
        &instance.cipher,
        &instance.settings.directory,
        &record,
        Retry::once(),
    )
    .await;
    assert_eq!(results.len(), 1);
    let (name, result) = &results[0];
    assert_eq!(name, "Bucket");
    assert!(result.ok, "{result:?}");
    assert_eq!(result.error, None);

    let key = offsite::key_name(&record.file).unwrap();
    let stored = objects.lock().unwrap().clone();
    assert_eq!(
        stored.get(&format!("home/{}", record.file)),
        Some(&local_file(&instance.settings.directory, &record.file).await)
    );
    assert!(stored.contains_key(&format!("home/{key}")), "the key was asked for");
    assert!(stored.contains_key("home/dumbmonit-20200102-000000.db"), "second newest kept");
    assert!(!stored.contains_key("home/dumbmonit-20200101-000000.db"), "beyond keep");
    assert!(!stored.contains_key("home/dumbmonit-20200101-000000.key"), "its key with it");
    assert!(stored.contains_key("home/notes.txt"), "foreign files are never touched");
    assert!(stored.contains_key("other/dumbmonit-20200101-000000.db"), "other prefixes neither");

    let destination = store::get(&instance.pool, id).await.unwrap().unwrap();
    let last = destination.last.expect("result recorded");
    assert!(last.ok && last.bytes > 0);
    assert_eq!(destination.last_success_at.as_deref(), Some(last.at.as_str()));
    let metrics = store::last_successes(&instance.pool).await.unwrap();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].0, "Bucket");
}

#[tokio::test]
async fn a_failed_upload_leaves_the_local_backup_alone() {
    let objects: Objects = Arc::default();
    let base = s3_server(objects.clone()).await;
    let instance = instance().await;
    let wrong = secrets(&[("secret_access_key", "not-the-key")]);
    let id = add(&instance, "Wrong key", Kind::S3, s3_settings(&base), 7, false, &wrong).await;
    let closed = json!({ "url": "http://127.0.0.1:1/dav/backups/", "username": "u" });
    add(&instance, "Down", Kind::Webdav, closed, 7, false, &secrets(&[("password", "p")])).await;

    let record = local::run_once(&instance.pool, &instance.settings).await;
    assert!(record.ok);
    let results = offsite::push_all(
        &instance.pool,
        &instance.cipher,
        &instance.settings.directory,
        &record,
        Retry::once(),
    )
    .await;
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|(_, result)| !result.ok), "{results:?}");

    // La copie locale est là, entière, et le journal local dit « réussi ».
    let files = local::list_files(&instance.settings.directory).await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, record.file);
    assert!(local::last_run(&instance.pool).await.unwrap().unwrap().ok);

    let destination = store::get(&instance.pool, id).await.unwrap().unwrap();
    let error = destination.last.unwrap().error.unwrap();
    assert!(error.contains("SignatureDoesNotMatch"), "{error}");
    assert!(error.contains("Check the secret access key"), "{error}");
    assert!(destination.last_success_at.is_none());
    assert!(objects.lock().unwrap().is_empty());
    assert!(store::last_successes(&instance.pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn webdav_receives_the_database_only_and_the_test_cleans_up() {
    let files: Objects = Arc::default();
    let base = dav_server(files.clone()).await;
    let settings = json!({ "url": format!("{base}/dav/backups"), "username": DAV_USER });

    let parsed = Settings::parse(Kind::Webdav, &settings).unwrap();
    let outcome = offsite::test_destination(&parsed, &secrets(&[("password", DAV_PASSWORD)])).await;
    assert!(outcome.ok, "{outcome:?}");
    assert!(files.lock().unwrap().is_empty(), "the test file is deleted");

    let refused = offsite::test_destination(&parsed, &secrets(&[("password", "wrong")])).await;
    assert!(!refused.ok);
    assert!(refused.message.contains("refused the user name or password"), "{}", refused.message);

    let instance = instance().await;
    let password = secrets(&[("password", DAV_PASSWORD)]);
    add(&instance, "Nextcloud", Kind::Webdav, settings, 1, false, &password).await;
    files.lock().unwrap().insert("dumbmonit-20200101-000000.db".into(), b"old".to_vec());
    files.lock().unwrap().insert("photo.jpg".into(), b"keep me".to_vec());

    let record = local::run_once(&instance.pool, &instance.settings).await;
    let results = offsite::push_all(
        &instance.pool,
        &instance.cipher,
        &instance.settings.directory,
        &record,
        Retry::once(),
    )
    .await;
    assert!(results[0].1.ok, "{results:?}");
    let written = local_file(&instance.settings.directory, &record.file).await;
    let remote = files.lock().unwrap().clone();
    let names: Vec<&String> = remote.keys().collect();
    assert_eq!(names, vec![&record.file, &"photo.jpg".to_string()]);
    assert_eq!(remote.get(&record.file), Some(&written));
}

#[tokio::test]
async fn destinations_travel_with_the_bundle_and_their_secrets_are_resealed() {
    let source = instance().await;
    add(
        &source,
        "Offsite",
        Kind::S3,
        s3_settings("https://s3.example.com"),
        14,
        true,
        &secrets(&[("secret_access_key", SECRET_KEY)]),
    )
    .await;
    let lot = collect(&source.pool, &source.cipher, ExportOptions::default()).await.unwrap();
    assert_eq!(lot.backup_destinations.len(), 1);
    assert_eq!(lot.summary().get("backup_destinations"), Some(&1));
    assert_eq!(lot.backup_destinations[0].secrets.get("secret_access_key").unwrap(), SECRET_KEY);

    let target = instance().await;
    let dry = restore(&target.pool, &target.cipher, &lot, false).await.unwrap();
    let section = |report: &dumbmonit_server::backup::RestoreReport| {
        let section =
            report.sections.iter().find(|s| s.section == "backup_destinations").expect("section");
        (section.created, section.updated, section.skipped)
    };
    assert_eq!(section(&dry), (1, 0, 0));
    assert!(store::list(&target.pool).await.unwrap().is_empty(), "a dry run writes nothing");

    let applied = restore(&target.pool, &target.cipher, &lot, true).await.unwrap();
    assert_eq!(section(&applied), (1, 0, 0));
    let restored = store::list(&target.pool).await.unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].name, "Offsite");
    assert_eq!(restored[0].keep, 14);
    assert!(restored[0].include_key && restored[0].has_secret);
    assert!(restored[0].last.is_none(), "results do not travel");
    let resealed = store::secrets(&target.pool, &target.cipher, restored[0].id).await.unwrap();
    assert_eq!(resealed, secrets(&[("secret_access_key", SECRET_KEY)]));
    // Chiffré par la clé de l'instance qui restaure, pas par celle d'origine.
    assert!(store::secrets(&target.pool, &source.cipher, restored[0].id).await.is_err());

    let again = restore(&target.pool, &target.cipher, &lot, true).await.unwrap();
    assert_eq!(section(&again), (0, 0, 1), "nothing changes the second time");

    // Une ligne invalide est signalée, pas restaurée.
    let mut broken = lot.clone();
    broken.backup_destinations[0].name = "Broken".into();
    broken.backup_destinations[0].secrets.clear();
    let report = restore(&target.pool, &target.cipher, &broken, true).await.unwrap();
    assert_eq!(section(&report), (0, 0, 1));
    let notes = &report.sections.iter().find(|s| s.section == "backup_destinations").unwrap().notes;
    assert!(notes[0].contains("Broken"), "{notes:?}");
}
