//! Lancement d'un contrôle : sans shell, environnement vidé, entrée fermée,
//! sortie bornée, durée bornée.
//!
//! Sous Unix, le contrôle ouvre son propre groupe de processus : au délai
//! dépassé, c'est le groupe entier qui est tué (un `check_http` lancé par un
//! script, un `sleep` oublié), et non le seul premier processus. Le groupe est
//! aussi tué quand le contrôle a fini, pour qu'aucun petit-enfant ne survive en
//! gardant la sortie ouverte. Sous Windows, `taskkill /T` tue l'arbre au délai.

use std::process::Stdio;
use std::time::Duration;

use dumbmonit_proto::script::CHECK_OUTPUT_MAX_BYTES;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::allow::Program;

/// Octets de l'erreur standard gardés : de quoi expliquer un échec.
const STDERR_MAX_BYTES: usize = 4 * 1024;

/// Délai laissé aux flux pour se vider une fois le processus terminé.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Arguments de PowerShell devant un `.ps1` : ni profil, ni invite, ni
/// stratégie d'exécution — le fichier est dans un dossier que seul
/// l'administrateur peut écrire, c'est ce dossier qui fait foi.
#[cfg(windows)]
const POWERSHELL_ARGS: &[&str] =
    &["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"];

/// Fin d'un contrôle.
#[derive(Debug)]
pub enum Ended {
    Finished { code: Option<i32>, stdout: String, stderr: String },
    TimedOut,
}

/// Lance `program` avec `args` et `env` (en plus d'un environnement minimal).
/// `user` : uid et gid sous lesquels tourner (Unix), sinon ceux de l'agent.
pub async fn run(
    program: &Program,
    args: &[String],
    env: &[(String, String)],
    timeout: Duration,
    user: Option<(u32, u32)>,
) -> Result<Ended, String> {
    let mut command = match &program.interpreter {
        Some(interpreter) => {
            let mut command = tokio::process::Command::new(interpreter);
            #[cfg(windows)]
            command.args(POWERSHELL_ARGS);
            command.arg(&program.path);
            command
        }
        None => tokio::process::Command::new(&program.path),
    };
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = program.path.parent() {
        command.current_dir(dir);
    }
    base_env(&mut command);
    for (key, value) in env {
        command.env(key, value);
    }
    #[cfg(unix)]
    {
        command.process_group(0);
        if let Some((uid, gid)) = user {
            // `std` retire aussi les groupes supplémentaires quand l'agent
            // tourne en root : le contrôle n'hérite d'aucun de ses droits.
            command.uid(uid);
            command.gid(gid);
        }
    }
    #[cfg(not(unix))]
    let _ = user;
    #[cfg(windows)]
    {
        // Pas de fenêtre de console, et un groupe à soi.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", program.path.display()))?;
    let pid = child.id();
    let stdout = tokio::spawn(drain(child.stdout.take(), CHECK_OUTPUT_MAX_BYTES));
    let stderr = tokio::spawn(drain(child.stderr.take(), STDERR_MAX_BYTES));
    let waited = tokio::time::timeout(timeout, child.wait()).await;
    let status = match waited {
        Ok(Ok(status)) => {
            kill_group(pid).await;
            Some(status)
        }
        Ok(Err(error)) => {
            kill_tree(pid).await;
            return Err(format!("waiting for {}: {error}", program.path.display()));
        }
        Err(_) => {
            kill_tree(pid).await;
            let _ = child.kill().await;
            None
        }
    };
    let collect = |handle: tokio::task::JoinHandle<Vec<u8>>| async move {
        let bytes = tokio::time::timeout(DRAIN_GRACE, handle).await.ok().and_then(Result::ok);
        String::from_utf8_lossy(&bytes.unwrap_or_default()).into_owned()
    };
    let stdout = collect(stdout).await;
    let stderr = collect(stderr).await;
    Ok(match status {
        Some(status) => Ended::Finished { code: status.code(), stdout, stderr },
        None => Ended::TimedOut,
    })
}

/// Environnement minimal commun à tous les contrôles.
fn base_env(command: &mut tokio::process::Command) {
    #[cfg(unix)]
    command.env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
    #[cfg(windows)]
    for key in ["SystemRoot", "SystemDrive", "windir", "PATH", "PATHEXT", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.env("LANG", "C");
    command.env("LC_ALL", "C");
}

/// Lit un flux jusqu'au bout, en ne gardant que ses `cap` premiers octets : le
/// reste est lu et jeté, pour que le processus ne bloque pas sur un tuyau plein.
async fn drain<R: AsyncRead + Unpin>(reader: Option<R>, cap: usize) -> Vec<u8> {
    let mut kept = Vec::new();
    let Some(mut reader) = reader else { return kept };
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = cap.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    }
    kept
}

/// Tue le groupe de processus du contrôle (Unix) : ses descendants restés
/// en vie. Rien ailleurs.
async fn kill_group(pid: Option<u32>) {
    #[cfg(unix)]
    kill_unix_group(pid).await;
    #[cfg(not(unix))]
    let _ = pid;
}

/// Tue le contrôle et tous ses descendants, au délai dépassé.
async fn kill_tree(pid: Option<u32>) {
    #[cfg(unix)]
    kill_unix_group(pid).await;
    #[cfg(windows)]
    if let Some(pid) = pid.filter(|pid| *pid > 4) {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let taskkill = std::path::Path::new(&root).join(r"System32\taskkill.exe");
        let _ = tokio::process::Command::new(taskkill)
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    #[cfg(not(any(unix, windows)))]
    let _ = pid;
}

#[cfg(target_os = "linux")]
async fn kill_unix_group(pid: Option<u32>) {
    // Jamais 0 ni 1 : `kill(0)` viserait le groupe de l'agent lui-même.
    let Some(pid) = pid.filter(|pid| *pid > 1).and_then(|pid| i32::try_from(pid).ok()) else {
        return;
    };
    // SAFETY: kill(2) ne lit ni n'écrit aucune mémoire du processus ; un pid
    // négatif désigne le groupe dont le contrôle est le chef (`process_group(0)`).
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
async fn kill_unix_group(pid: Option<u32>) {
    let Some(pid) = pid.filter(|pid| *pid > 1) else { return };
    let _ = tokio::process::Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}
