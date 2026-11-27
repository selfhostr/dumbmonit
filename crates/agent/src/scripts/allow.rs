//! Ce que l'agent accepte d'exécuter : la liste blanche de la machine.
//!
//! Le serveur nomme un programme ; c'est ici qu'il devient — ou non — un
//! fichier à lancer. Les règles, dans l'ordre :
//!
//! 1. le nom passe [`validate_command`] (pas de `..`, pas de chemin réseau,
//!    pas de chemin relatif à plusieurs éléments) ;
//! 2. le fichier réel, liens symboliques résolus (`canonicalize`), est un
//!    enfant **direct** d'un dossier de `scripts_dirs`, lui aussi résolu : un
//!    lien qui sort du dossier, ou un sous-dossier, est refusé ;
//! 3. sous Unix, le fichier est exécutable et ni lui ni son dossier ne sont
//!    modifiables par le groupe ou les autres, et ils appartiennent à `root`
//!    ou au compte de l'agent ; sous Windows, l'extension est `.exe`, `.com`,
//!    `.bat`, `.cmd` ou `.ps1` (les droits passent par les ACL du dossier,
//!    réservé aux administrateurs) ;
//! 4. le programme n'est ni un interpréteur, ni un outil qui exécute ou écrit
//!    ce que ses arguments lui disent ([`DENIED_PROGRAMS`]) : déposé par erreur
//!    dans le dossier, il donnerait au serveur une ligne de commande complète.

use std::path::{Path, PathBuf};

use dumbmonit_proto::script::validate_command;

/// Programmes jamais lancés, même présents dans un dossier autorisé : shells,
/// interpréteurs, lanceurs, outils qui écrivent un fichier choisi par leurs
/// arguments, et les greffons Nagios qui exécutent une autre commande
/// (`negate`, `urlize`), en ouvrent une à distance (`check_by_ssh`) ou
/// recopient un fichier où on leur dit (`check_log -O`).
pub const DENIED_PROGRAMS: &[&str] = &[
    "sh",
    "ash",
    "bash",
    "dash",
    "zsh",
    "ksh",
    "mksh",
    "csh",
    "tcsh",
    "fish",
    "busybox",
    "env",
    "sudo",
    "su",
    "doas",
    "pkexec",
    "runuser",
    "nohup",
    "setsid",
    "xargs",
    "find",
    "timeout",
    "nice",
    "ionice",
    "chroot",
    "script",
    "expect",
    "awk",
    "gawk",
    "mawk",
    "sed",
    "ed",
    "vi",
    "vim",
    "nvim",
    "tclsh",
    "wish",
    "curl",
    "wget",
    "nc",
    "ncat",
    "socat",
    "ssh",
    "scp",
    "tar",
    "cp",
    "mv",
    "rm",
    "dd",
    "tee",
    "install",
    "cmd",
    "wscript",
    "cscript",
    "mshta",
    "rundll32",
    "regsvr32",
    "msiexec",
    "negate",
    "urlize",
    "check_by_ssh",
    "check_log",
];

/// Interpréteurs refusés sous toutes leurs versions (`python3.12`, `php8.2`).
const DENIED_INTERPRETERS: &[&str] =
    &["python", "perl", "php", "ruby", "lua", "node", "nodejs", "pwsh", "powershell"];

/// Extensions lancées sous Windows.
#[cfg(windows)]
const WINDOWS_EXTENSIONS: &[&str] = &["exe", "com", "bat", "cmd", "ps1"];

/// Un programme retenu : son chemin réel, et l'interpréteur qui le lit quand
/// ce n'est pas un exécutable (un `.ps1` sous Windows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub path: PathBuf,
    pub interpreter: Option<PathBuf>,
}

/// Retrouve le programme d'un contrôle dans les dossiers autorisés.
///
/// Un nom seul est cherché dans chaque dossier, dans l'ordre ; un chemin
/// absolu doit désigner un fichier de l'un d'eux. L'erreur dit quoi corriger,
/// sur la machine : c'est l'administrateur qui la lira.
pub fn resolve(dirs: &[PathBuf], command: &str) -> Result<Program, String> {
    validate_command(command)?;
    if dirs.is_empty() {
        return Err("no script directory is configured (scripts_dirs in agent.yaml)".to_string());
    }
    let allowed: Vec<PathBuf> = dirs.iter().filter_map(|dir| allowed_dir(dir).ok()).collect();
    let candidates: Vec<PathBuf> = if Path::new(command).is_absolute() || looks_absolute(command) {
        vec![PathBuf::from(command)]
    } else {
        dirs.iter().map(|dir| dir.join(command)).collect()
    };
    let Some(found) = candidates.iter().find(|path| std::fs::symlink_metadata(path).is_ok()) else {
        return Err(format!(
            "'{command}' was not found in the script directories ({})",
            shown_dirs(dirs)
        ));
    };
    let real =
        std::fs::canonicalize(found).map_err(|error| format!("{}: {error}", found.display()))?;
    let parent = real.parent().map(Path::to_path_buf).unwrap_or_default();
    if !allowed.contains(&parent) {
        // Le dossier est peut-être listé mais refusé (droits trop larges) :
        // c'est ce qu'il faut dire, plutôt que « hors des dossiers ».
        if let Some(problem) = dirs.iter().find_map(|dir| {
            let canonical = std::fs::canonicalize(dir).ok()?;
            (canonical == parent).then(|| allowed_dir(dir).err()).flatten()
        }) {
            return Err(problem);
        }
        return Err(format!(
            "'{command}' resolves to {}, which is not directly inside an allowed script \
             directory ({}); symbolic links leading elsewhere and subdirectories are refused",
            real.display(),
            shown_dirs(dirs)
        ));
    }
    check_file(&real)?;
    check_not_denied(&real)?;
    Ok(Program { interpreter: interpreter_for(&real), path: real })
}

/// Un chemin Windows absolu (`C:\…`), reconnu sur toutes les plateformes pour
/// que le message reste juste.
fn looks_absolute(command: &str) -> bool {
    let bytes = command.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn shown_dirs(dirs: &[PathBuf]) -> String {
    dirs.iter().map(|dir| dir.display().to_string()).collect::<Vec<_>>().join(", ")
}

/// Un dossier autorisé, résolu, s'il est sûr.
pub fn allowed_dir(dir: &Path) -> Result<PathBuf, String> {
    let canonical =
        std::fs::canonicalize(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    check_not_writable_by_others(&canonical)?;
    Ok(canonical)
}

/// Le nom du fichier, en minuscules, sans extension Windows.
fn bare_name(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    for extension in [".exe", ".com", ".bat", ".cmd", ".ps1"] {
        if let Some(stem) = name.strip_suffix(extension) {
            return stem.to_string();
        }
    }
    name
}

/// Refuse les programmes de [`DENIED_PROGRAMS`] et les interpréteurs.
pub fn check_not_denied(path: &Path) -> Result<(), String> {
    let name = bare_name(path);
    let interpreter = DENIED_INTERPRETERS.iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit() || c == '.'))
    });
    if DENIED_PROGRAMS.contains(&name.as_str()) || interpreter {
        return Err(format!(
            "{} is a shell, an interpreter or a tool that runs or writes what its arguments say; \
             it is never run as a check",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn check_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if meta.permissions().mode() & 0o111 == 0 {
        return Err(format!("{} is not executable (chmod +x)", path.display()));
    }
    check_not_writable_by_others(path)
}

#[cfg(windows)]
fn check_file(path: &Path) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let extension =
        path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if !WINDOWS_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!(
            "{}: only .exe, .com, .bat, .cmd and .ps1 files are run",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn check_file(path: &Path) -> Result<(), String> {
    Err(format!("{}: custom checks are not supported on this system", path.display()))
}

/// Refuse un fichier ou un dossier que le groupe ou les autres peuvent
/// modifier, ou qui n'appartient ni à `root` ni au compte de l'agent.
#[cfg(unix)]
pub fn check_not_writable_by_others(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if meta.mode() & 0o022 != 0 {
        return Err(format!("{} is writable by group or others (chmod go-w)", path.display()));
    }
    if let Some(uid) = current_uid()
        && meta.uid() != 0
        && meta.uid() != uid
    {
        return Err(format!(
            "{} is owned by another account (uid {}): it must belong to root or to the agent",
            path.display(),
            meta.uid()
        ));
    }
    Ok(())
}

/// Sous Windows, les droits passent par des ACL que la bibliothèque standard
/// ne lit pas : l'installateur réserve `C:\ProgramData\DumbMonit` aux
/// administrateurs, et la documentation demande de garder ce dossier ainsi.
#[cfg(not(unix))]
pub fn check_not_writable_by_others(path: &Path) -> Result<(), String> {
    std::fs::metadata(path).map(|_| ()).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(target_os = "linux")]
fn current_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|meta| meta.uid())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn current_uid() -> Option<u32> {
    None
}

/// PowerShell pour un `.ps1`, rien pour un exécutable.
#[cfg(windows)]
fn interpreter_for(path: &Path) -> Option<PathBuf> {
    let is_ps1 =
        path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ps1"));
    is_ps1.then(|| {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
    })
}

#[cfg(not(windows))]
fn interpreter_for(_path: &Path) -> Option<PathBuf> {
    None
}

/// Compte sous lequel lancer les contrôles (`scripts_user`) : un nom de
/// `/etc/passwd`, ou `uid:gid` en chiffres pour un compte d'annuaire.
#[cfg(unix)]
pub fn lookup_user(user: &str) -> Result<(u32, u32), String> {
    if let Some((uid, gid)) = user.split_once(':')
        && let (Ok(uid), Ok(gid)) = (uid.parse::<u32>(), gid.parse::<u32>())
    {
        return Ok((uid, gid));
    }
    let passwd = std::fs::read_to_string("/etc/passwd")
        .map_err(|error| format!("reading /etc/passwd: {error}"))?;
    parse_passwd(&passwd, user).ok_or_else(|| {
        format!("scripts_user: no account named '{user}' in /etc/passwd (or write uid:gid)")
    })
}

/// L'uid et le gid d'un compte dans le texte de `/etc/passwd`.
#[cfg(unix)]
pub fn parse_passwd(passwd: &str, user: &str) -> Option<(u32, u32)> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        if name != user || name.is_empty() {
            return None;
        }
        let _password = fields.next()?;
        let uid = fields.next()?.parse().ok()?;
        let gid = fields.next()?.parse().ok()?;
        Some((uid, gid))
    })
}
