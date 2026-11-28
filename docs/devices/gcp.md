# Google Cloud

The state of a Google Cloud project: Compute Engine instances, Cloud SQL
instances and the Cloud Monitoring metrics you choose (CPU utilization by
default).

DumbMonit reads three REST APIs once a minute: **Compute Engine**
(`aggregated/instances`, the state of every instance in every zone),
**Cloud SQL Admin** (`instances`) and **Cloud Monitoring**
(`projects.timeSeries.list`, the latest 5-minute mean of each metric type).
The credential is the JSON key of a service account with read-only roles.
DumbMonit signs a JWT with the key (RS256) and exchanges it for a one-hour
access token (OAuth 2.0 JWT bearer flow, RFC 7523), kept until it expires.
Billing is not read.

A stopped instance (`TERMINATED`, `SUSPENDED`) is the owner's choice: it is
counted apart and does not alert. Only `REPAIRING` Compute Engine instances and
`FAILED` Cloud SQL instances are failures.

This is an Internet API: the address is the API's, not a machine's. DumbMonit
needs outbound HTTPS to `*.googleapis.com`; nothing inside your instances is
contacted.

!!! warning "Validated against the vendor documentation only — not yet tested on a real system"

    This integration was built from Google's REST API documentation, not
    against a live project. It only reads. Tell us what breaks.

## Network ports to open

From the DumbMonit server (or its relay agent) to the device:

| Port | Protocol | Purpose |
|---|---|---|
| 443 | TCP | Google Cloud REST APIs (HTTPS, *.googleapis.com) |
| 443 | TCP | Google sign-in, access token (HTTPS, oauth2.googleapis.com) |

## Create a read-only service account for DumbMonit

1. In the Google Cloud console, open IAM & Admin > Service accounts > Create service account, and name it dumbmonit.

2. Grant it the roles Compute Viewer, Cloud SQL Viewer and Monitoring Viewer on the project (IAM & Admin > IAM > Grant access).

3. Open the service account > Keys > Add key > Create new key > JSON: a key file is downloaded, shown once.

4. Make sure the Compute Engine, Cloud SQL Admin and Cloud Monitoring APIs are enabled on the project (APIs & Services > Enabled APIs).

5. In DumbMonit, enter googleapis.com as address, paste the content of the JSON key file, and fill the Project ID option if the key belongs to another project.

### Required permissions

- **Compute Viewer (role on the project)** — Lists the Compute Engine instances and their state.
- **Cloud SQL Viewer (role on the project)** — Lists the Cloud SQL instances and their state. Optional.
- **Monitoring Viewer (role on the project)** — Reads the Cloud Monitoring time series. Optional.

!!! warning
    A stopped Compute Engine instance or Cloud SQL instance (TERMINATED, SUSPENDED) is counted apart and raises no alert. Billing is not read.

Vendor documentation: https://cloud.google.com/compute/docs/reference/rest/v1/instances/aggregatedList, https://cloud.google.com/sql/docs/mysql/admin-api/rest/v1beta4/instances/list and https://cloud.google.com/monitoring/api/ref_v3/rest/v3/projects.timeSeries/list

## Credentials

| Credential | Fields |
|---|---|
| Service account key (JSON) | The whole content of the key file (`client_email` and `private_key` are used). |

The key is stored encrypted and never shown again. Address: `googleapis.com`.
Options:

- **Project ID**: the project to watch; empty means the `project_id` of the key.
- **Monitoring metrics**: metric types separated by commas (5 at most), for
  example `compute.googleapis.com/instance/cpu/utilization`. Metrics ending
  in `/utilization` are shown as percentages.
- **Timeout per request** (15 s).

## What it watches

All metrics are prefixed `dumbmonit_gcp_`.

| Metric | Labels | Meaning |
|---|---|---|
| `instances`, `instances_running`, `instances_stopped`, `instances_failed` | | Compute Engine instances by state. |
| `instance_unhealthy` | `instance`, `zone`, `state` | Value 1 for each failed (repairing) instance. |
| `sql_instances`, `sql_instances_running`, `sql_instances_stopped`, `sql_instances_failed` | | Cloud SQL instances by state. |
| `sql_unhealthy` | `instance`, `state` | Value 1 for each failed Cloud SQL instance. |
| `sql_readable`, `monitoring_readable` | | 0 when that API cannot be read; Compute Engine is still reported. |
| `metric` | `metric`, `resource`, `unit` | Latest value of each chosen Monitoring series. |

At most 200 named instances are listed; the counts stay exact.

The [built-in rules](../alerting/rules.md#google-cloud) that apply:

- **Google Cloud instance failed**: an instance repairing for 10 minutes
  (Critical).
- **Google Cloud SQL instance failed**: a Cloud SQL instance failed for 10
  minutes (Critical).

## Troubleshooting

"Google refused the service account": the key was deleted or disabled, or the
server clock is wrong (the signed token is valid for one hour from its issue time).

"Google refused to list the Compute Engine instances (403)": the service
account lacks the Compute Viewer role, or the Compute Engine API is not
enabled on the project.

"Google knows no project …": use the project ID, not its display name.

`sql_readable` or `monitoring_readable` is 0: the matching role or API is
missing; the rest keeps working.
