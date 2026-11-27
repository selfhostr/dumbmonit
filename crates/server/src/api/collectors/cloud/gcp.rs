//! Notice de gcp (Google Cloud).

use super::super::*;

pub(super) fn view() -> CollectorView {
    CollectorView {
        kind: "gcp",
        label: "Google Cloud",
        summary: "Compute Engine and Cloud SQL instance states and Cloud Monitoring metrics of a Google Cloud project, read through a read-only service account.",
        examples: &["Google Cloud project"],
        credential_types: &["api_token"],
        credentials: &[CredentialView {
            kind: "api_token",
            label: "Service account key (JSON)",
            help: "The JSON key file of a service account that has read-only roles on the project.",
            fields: &[CredentialField {
                key: "token",
                label: "Service account key",
                help: "Paste the whole content of the JSON key file.",
                placeholder: "",
                input: "password",
                choices: &[],
                required: true,
            }],
        }],
        address_hint: "googleapis.com",
        default_port: 443,
        setup: Setup {
            title: "Create a read-only service account for DumbMonit",
            steps: &[
                "In the Google Cloud console, open IAM & Admin > Service accounts > Create service account, and name it dumbmonit.",
                "Grant it the roles Compute Viewer, Cloud SQL Viewer and Monitoring Viewer on the project (IAM & Admin > IAM > Grant access).",
                "Open the service account > Keys > Add key > Create new key > JSON: a key file is downloaded, shown once.",
                "Make sure the Compute Engine, Cloud SQL Admin and Cloud Monitoring APIs are enabled on the project (APIs & Services > Enabled APIs).",
                "In DumbMonit, enter googleapis.com as address, paste the content of the JSON key file, and fill the Project ID option if the key belongs to another project.",
            ],
            ports: &[
                to_device!(443, "tcp", "Google Cloud REST APIs (HTTPS, *.googleapis.com)"),
                to_device!(443, "tcp", "Google sign-in, access token (HTTPS, oauth2.googleapis.com)"),
            ],
            permissions: &[
                "Compute Viewer (role on the project)\nLists the Compute Engine instances and their state.",
                "Cloud SQL Viewer (role on the project)\nLists the Cloud SQL instances and their state. Optional.",
                "Monitoring Viewer (role on the project)\nReads the Cloud Monitoring time series. Optional.",
            ],
            warning: "A stopped Compute Engine instance or Cloud SQL instance (TERMINATED, SUSPENDED) is counted apart and raises no alert. Billing is not read.",
        },
        options: &[
            OptionView {
                key: "project_id",
                label: "Project ID",
                help: "The project to watch. Empty: the project the service account key belongs to.",
                placeholder: "my-project-123",
                default: "",
                required: false,
                input: "text",
                choices: &[],
                group: "",
                items: no_items,
                columns: no_columns,
            },
            OptionView {
                key: "metrics",
                label: "Monitoring metrics",
                help: "Cloud Monitoring metric types to read, separated by commas (5 at most). CPU utilization is shown as a percentage.",
                placeholder: "compute.googleapis.com/instance/cpu/utilization",
                default: "compute.googleapis.com/instance/cpu/utilization",
                required: false,
                input: "text",
                choices: &[],
                group: "",
                items: no_items,
                columns: no_columns,
            },
            OptionView {
                key: "request_timeout_seconds",
                label: "Timeout per request (seconds)",
                help: "Time allowed for each call, from 1 to 120.",
                placeholder: "15",
                default: "15",
                required: false,
                input: "number",
                choices: &[],
                group: "",
                items: no_items,
                columns: no_columns,
            },
        ],
    }
}
