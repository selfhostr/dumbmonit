//! Réglages et identifiants des contrôles personnalisés exécutés par un agent
//! (`collectors/agent/scripts.rs`).

use super::*;

/// Options lues par `collectors/agent/scripts.rs`, dans l'ordre du formulaire.
pub(super) const AGENT_SCRIPT_OPTIONS: &[OptionView] = &[
    agent_link(
        "agent_id",
        "Agent that runs the check",
        "The DumbMonit agent of the machine where the script or plugin lives. It only runs files of the directories its agent.yaml allows (scripts: true, scripts_dirs).",
    ),
    OptionView {
        required: true,
        ..text(
            "command",
            "Command",
            "A file name of the agent's script directories (check_disk, backup-age.sh) or its absolute path. No shell: pipes, ; and $() are not interpreted.",
            "check_disk",
            "",
        )
    },
    OptionView {
        key: "args",
        label: "Arguments",
        help: "One argument per row, passed as is. Write {{secret.username}}, {{secret.password}} or {{secret.token}} where the credential of this monitor goes.",
        placeholder: "",
        default: "",
        required: false,
        input: "rows",
        choices: &[],
        group: "",
        items: no_items,
        columns: script_arg_columns,
    },
    select(
        "format",
        "Output format",
        "nagios: exit code 0 OK, 1 WARNING, 2 CRITICAL, 3 UNKNOWN, first line as message, performance data after |. json: {\"status\", \"message\", \"metrics\"}. prometheus: text exposition format, status from the exit code.",
        "nagios",
        &["nagios", "json", "prometheus"],
    ),
    number(
        "timeout_seconds",
        "Timeout (seconds)",
        "The check is killed with all its child processes after this time, from 1 to 120.",
        "10",
        "10",
    ),
    OptionView {
        key: "env",
        label: "Environment variables",
        help: "Added to a minimal environment. Variables that load code or change what runs (PATH, LD_*, PYTHON*, BASH_ENV…) are refused.",
        placeholder: "",
        default: "",
        required: false,
        input: "rows",
        choices: &[],
        group: "",
        items: no_items,
        columns: script_env_columns,
    },
];

pub(super) fn script_arg_columns() -> Vec<OptionColumn> {
    vec![OptionColumn {
        key: "value".into(),
        label: "Argument".into(),
        placeholder: "-w".into(),
        input: "text".into(),
        ..OptionColumn::default()
    }]
}

pub(super) fn script_env_columns() -> Vec<OptionColumn> {
    vec![
        OptionColumn {
            key: "name".into(),
            label: "Name".into(),
            placeholder: "MYSQL_HOST".into(),
            input: "text".into(),
            pattern: "^[A-Za-z_][A-Za-z0-9_]{0,63}$".into(),
            invalid: "Letters, digits and _ only, not starting with a digit.".into(),
            unique: true,
            ..OptionColumn::default()
        },
        OptionColumn {
            key: "value".into(),
            label: "Value".into(),
            input: "text".into(),
            ..OptionColumn::default()
        },
    ]
}

/// Un jeton, remplacé dans les arguments et passé en variable.
pub(super) const SCRIPT_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "Token",
    help: "Written where an argument says {{secret.token}}, and passed as DUMBMONIT_SECRET_TOKEN.",
    fields: &[cred_secret("token", "Token", "Stored encrypted, never shown again.", "", true)],
};

/// Un compte, remplacé dans les arguments et passé en variables.
pub(super) const SCRIPT_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "Username / password",
    help: "Written where an argument says {{secret.username}} and {{secret.password}}, and passed as DUMBMONIT_SECRET_USERNAME and DUMBMONIT_SECRET_PASSWORD.",
    fields: &[
        cred_text("username", "User name", "", ""),
        cred_secret("password", "Password", "Stored encrypted, never shown again.", "", true),
    ],
};
