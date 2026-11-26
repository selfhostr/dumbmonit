//! Sonde IMAP (`kind = "imap"`).
//!
//! « Puis-je encore relever mon courrier ? » : un serveur IMAP peut ouvrir son
//! port, présenter un beau certificat et refuser tout le monde depuis que la
//! base des comptes est indisponible. La sonde mène le début d'une vraie session :
//! bannière `* OK`, `STARTTLS` si demandé, `LOGIN` si la cible porte des
//! identifiants, puis `LOGOUT`. Elle ne sélectionne aucune boîte et ne lit aucun
//! message.
//!
//! # Adresse et étiquettes
//!
//! Adresse : `imap.exemple.fr`, ou `imap.exemple.fr:1993`. Identifiants :
//! facultatifs (nom d'utilisateur et mot de passe).
//!
//! | Étiquette | Défaut | Rôle |
//! |---|---|---|
//! | `security` | `tls` | `tls` (993), `starttls` (143) ou `none` (143, identifiants en clair). |
//! | `port` | selon `security` | Port, si l'adresse n'en précise pas. |
//! | `server_name` | l'hôte | Nom envoyé en SNI. |
//! | `insecure_tls` | `false` | Une chaîne non vérifiable ne fait plus échouer. |
//! | `allow_private_targets` | `false` | Autorise la boucle locale (voir `guard`). |
//! | `timeout_seconds` | `5` | Délai propre à la sonde (1 à 60). |

pub(crate) mod options;

use std::time::Instant;

use async_trait::async_trait;
use dumbmonit_proto::{Collector, ProbeError, Sample, Target};
use tracing::debug;

use super::mailbox::{self, Aborted, MAX_LINE_BYTES, Security, truncated};
use super::outcome::{Failure, Report};
use super::session::{Deadline, LineReader, Stream};
use options::Options;

/// Étiquettes des commandes. Une seule est en vol à la fois : des tags fixes suffisent.
const TAG_STARTTLS: &str = "a001";
const TAG_LOGIN: &str = "a002";
const TAG_LOGOUT: &str = "a003";

/// Réponses non étiquetées tolérées avant la réponse étiquetée.
const MAX_UNTAGGED_LINES: usize = 128;

/// Collecteur de disponibilité d'un serveur IMAP.
#[derive(Default)]
pub struct ImapCollector;

impl ImapCollector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Collector for ImapCollector {
    fn kind(&self) -> &'static str {
        "imap"
    }

    async fn probe(&self, target: &Target) -> Result<Vec<Sample>, ProbeError> {
        let options = options::from_target(target)?;
        let mut report = Report::new(self.kind())
            .label("port", options.port.to_string())
            .label("security", options.security.as_str());

        converse(&mut report, &options).await?;

        if let Some(detail) = report.detail() {
            debug!(target_id = target.id, host = %options.host, port = options.port, detail,
                "sonde IMAP en échec");
        }
        Ok(report.finish())
    }
}

/// Où en est la session après la bannière.
enum Opening {
    /// Rien d'autre à négocier. `true` si le serveur a répondu `PREAUTH`.
    Ready { preauthenticated: bool },
    /// Le serveur a accepté `STARTTLS` : la suite se passe chiffrée.
    Upgrade,
}

/// `Err` seulement quand le garde-fou refuse l'adresse.
async fn converse(report: &mut Report, options: &Options) -> Result<(), ProbeError> {
    let deadline = Deadline::starting_now(options.timeout);
    let Some(mut stream) = mailbox::open(report, options, deadline).await? else {
        return Ok(());
    };
    let mut reader = LineReader::new(MAX_LINE_BYTES);

    let preauthenticated = match opening(report, &mut stream, &mut reader, options, deadline).await
    {
        Err(Aborted) => {
            stream.close().await;
            return Ok(());
        }
        Ok(Opening::Ready { preauthenticated }) => preauthenticated,
        Ok(Opening::Upgrade) => {
            let plain = stream.into_plain().expect("la session est encore en clair");
            let Some(secured) = mailbox::upgrade(report, plain, options, deadline).await else {
                return Ok(());
            };
            stream = secured;
            reader = LineReader::new(MAX_LINE_BYTES);
            false
        }
    };

    if login(report, &mut stream, &mut reader, options, preauthenticated, deadline).await.is_ok() {
        // Sans attendre la réponse : le service a déjà tout dit.
        let _ = stream.write_line(&format!("{TAG_LOGOUT} LOGOUT"), deadline).await;
    }
    stream.close().await;
    Ok(())
}

/// Bannière, puis `STARTTLS` le cas échéant.
async fn opening(
    report: &mut Report,
    stream: &mut Stream,
    reader: &mut LineReader,
    options: &Options,
    deadline: Deadline,
) -> Result<Opening, Aborted> {
    let started = Instant::now();
    let greeting = mailbox::read_line(report, stream, reader, deadline).await?;
    report.gauge("imap_greeting_seconds", started.elapsed().as_secs_f64());

    let upper = greeting.to_ascii_uppercase();
    let preauthenticated = upper.starts_with("* PREAUTH");
    if !preauthenticated && !upper.starts_with("* OK") {
        report
            .fail(Failure::Protocol, format!("not an IMAP greeting: \"{}\"", truncated(&greeting)));
        return Err(Aborted);
    }
    if options.security != Security::StartTls {
        return Ok(Opening::Ready { preauthenticated });
    }

    mailbox::send(report, stream, &format!("{TAG_STARTTLS} STARTTLS"), deadline).await?;
    let (status, text) = tagged_status(report, stream, reader, TAG_STARTTLS, deadline).await?;
    if status != "OK" {
        report.fail(Failure::Tls, format!("STARTTLS refused: {status} {}", truncated(&text)));
        return Err(Aborted);
    }
    // Des octets déjà lus seraient du texte glissé avant la négociation, ce
    // qu'aucun serveur honnête ne fait.
    if !reader.take_buffered().is_empty() {
        report.fail(
            Failure::Protocol,
            "the server sent data before the TLS negotiation, which STARTTLS forbids",
        );
        return Err(Aborted);
    }
    Ok(Opening::Upgrade)
}

/// `LOGIN`, si la cible porte des identifiants.
async fn login(
    report: &mut Report,
    stream: &mut Stream,
    reader: &mut LineReader,
    options: &Options,
    preauthenticated: bool,
    deadline: Deadline,
) -> Result<(), Aborted> {
    let Some((username, password)) = &options.login else { return Ok(()) };
    if preauthenticated {
        report.gauge("imap_authenticated", 1.0);
        return Ok(());
    }

    let started = Instant::now();
    let command = format!("{TAG_LOGIN} LOGIN {} {}", quote(username), quote(password));
    mailbox::send(report, stream, &command, deadline).await?;
    let (status, text) = tagged_status(report, stream, reader, TAG_LOGIN, deadline).await?;
    report.gauge("imap_login_seconds", started.elapsed().as_secs_f64());
    if status == "OK" {
        report.gauge("imap_authenticated", 1.0);
        return Ok(());
    }

    // `NO` : compte ou mot de passe refusé. `[UNAVAILABLE]` : le serveur ne
    // sait pas répondre pour l'instant, ce qui n'est pas la faute du compte.
    let service_side = text.to_ascii_uppercase().contains("[UNAVAILABLE]");
    let reason = if status == "NO" && !service_side { Failure::Auth } else { Failure::Protocol };
    // Détail journalisé, jamais mis en étiquette : il peut citer l'identifiant.
    report.fail(reason, format!("login refused: {status} {}", truncated(&text)));
    Err(Aborted)
}

/// Lit jusqu'à la réponse étiquetée `tag`, et rend son statut (`OK`, `NO`,
/// `BAD`) et le texte qui suit.
async fn tagged_status(
    report: &mut Report,
    stream: &mut Stream,
    reader: &mut LineReader,
    tag: &str,
    deadline: Deadline,
) -> Result<(String, String), Aborted> {
    let prefix = format!("{tag} ");
    for _ in 0..MAX_UNTAGGED_LINES {
        let line = mailbox::read_line(report, stream, reader, deadline).await?;
        if line.starts_with("* BYE") {
            report.fail(Failure::Protocol, format!("the server hung up: {}", truncated(&line)));
            return Err(Aborted);
        }
        let Some(rest) = line.strip_prefix(&prefix) else { continue };
        let (status, text) = rest.split_once(' ').unwrap_or((rest, ""));
        return Ok((status.to_ascii_uppercase(), text.to_string()));
    }
    report.fail(Failure::Protocol, "the server's reply never ends");
    Err(Aborted)
}

/// Chaîne entre guillemets au sens d'IMAP (RFC 3501 §4.3) : seuls `"` et `\`
/// s'échappent. Les sauts de ligne sont exclus en amont, à la lecture des options.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests;
