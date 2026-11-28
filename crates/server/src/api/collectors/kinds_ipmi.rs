//! Notice de ipmi (contrôleurs de gestion, IPMI v2.0 sur le réseau).

use super::*;

pub(super) fn view(kind: &str) -> Option<CollectorView> {
    (kind == "ipmi").then_some(CollectorView {
        kind: "ipmi",
        label: "Server hardware (IPMI)",
        summary: "Temperatures, fans, voltages, power supplies, power state and event log of a BMC, read over IPMI v2.0 (RMCP+).",
        examples: &["Supermicro BMC", "Dell iDRAC", "HPE iLO", "Any IPMI-capable BMC"],
        credential_types: &["username_password"],
        credentials: &[CredentialView {
            kind: "username_password",
            label: "BMC user",
            help: "A BMC user allowed to use IPMI over LAN, with the USER privilege level (read-only) or above.",
            fields: &[
                CredentialField {
                    key: "username",
                    label: "User name",
                    help: "The BMC user name.",
                    placeholder: "dumbmonit",
                    input: "text",
                    choices: &[],
                    required: true,
                },
                CredentialField {
                    key: "password",
                    label: "Password",
                    help: "The BMC user password. IPMI limits it to 20 characters.",
                    placeholder: "",
                    input: "password",
                    choices: &[],
                    required: true,
                },
            ],
        }],
        address_hint: "192.168.1.50",
        default_port: 623,
        setup: Setup {
            title: "Prepare the BMC for DumbMonit",
            steps: &[
                "In the BMC web interface, create a user for DumbMonit with the privilege level User (read-only) and IPMI over LAN enabled.",
                "Make sure IPMI over LAN is enabled on the BMC network interface (often under Configuration > Network > IPMI, or Services).",
                "Allow UDP 623 from the DumbMonit server (or its relay agent) to the BMC.",
                "In DumbMonit, enter the BMC address and the user name and password.",
            ],
            ports: &[to_device!(623, "udp", "IPMI over LAN (RMCP+)")],
            permissions: &[
                "IPMI user, privilege level User\nReads sensors, the chassis status and the event log; changes nothing.",
            ],
            warning: "IPMI v2.0 passwords are exchanged with a challenge but the protocol has known weaknesses: keep the BMC on a management network. If the BMC serves Redfish, prefer the Server hardware (Redfish) integration.",
        },
        options: &[
            OptionView {
                key: "request_timeout_seconds",
                label: "Timeout per exchange (seconds)",
                help: "Time allowed for each BMC answer, from 1 to 60.",
                placeholder: "5",
                default: "5",
                required: false,
                input: "number",
                choices: &[],
                group: "",
                items: no_items,
                columns: no_columns,
            },
        ],
    })
}
