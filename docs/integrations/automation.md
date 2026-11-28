# Terraform and Ansible

Everything the interface does goes through the [REST API](../reference/api.md),
so monitors, channels and maintenance windows can be created from Terraform,
Ansible or any script. Three routes, from simplest to most declarative:

1. **Configuration files (GitOps)**: describe the whole configuration in YAML and
   let DumbMonit converge to it, see [Configuration files](../using/config-files.md)
   (`dumbmonit-cli plan` / `apply`, or a watched folder with `DUMBMONIT_CONFIG_DIR`).
   Best when the YAML is the single source of truth.
2. **The Terraform provider** in the repository's `integrations/terraform-provider-dumbmonit`
   folder, which manages monitors, channels, rules, silences and status pages one by one.
   Build it from source; it is not yet published on a registry.
3. **Plain API calls** from Terraform's `restapi` provider or Ansible's `uri`
   module, shown below. Pick one route per object, not several.

## Authentication

Create an API token in *Settings → API & assistants* (an administrator session is
required; the secret is shown once). A token with the **write** scope can create
objects. Send it as `Authorization: Bearer dmt_…`; no CSRF header is needed.

```bash
export DUMBMONIT_URL=https://monit.example.org
export DUMBMONIT_TOKEN=dmt_...
curl -s -H "Authorization: Bearer $DUMBMONIT_TOKEN" "$DUMBMONIT_URL/api/targets"
```

Keep the token in a secret store (Terraform variable marked `sensitive`, Ansible
Vault), never in the repository.

## The three calls

| Object | Call | Minimal body |
|---|---|---|
| Monitor | `POST /api/targets` | `{"name","address","kind","interval_secs","tags":{}}` plus `credential` if the kind needs one |
| Notification channel | `POST /api/notify/channels` | `{"name","kind","enabled","settings":{},"secrets":{}}` |
| Silence (maintenance window) | `POST /api/alerts/silences` | `{"name","schedule":{…},"target_id","enabled"}` |

Channel kinds and their settings are listed by `GET /api/notify/kinds` and in
[Notifications](../notifications.md). A one-off silence uses the schedule
`{"kind":"once","starts_at":"2027-03-01T22:00:00Z","ends_at":"2027-03-01T23:00:00Z"}`;
a recurring one uses `{"kind":"weekly","days":[6],"start_minute":120,"end_minute":240,"timezone":"Europe/Paris"}`.
Secrets are never returned: the API answers `has_secret`, and omitting `secrets`
on `PUT` keeps the stored ones.

## Terraform

### With the `restapi` provider

The [`Mastercard/restapi`](https://registry.terraform.io/providers/Mastercard/restapi/latest)
provider turns an endpoint into a resource. Reads go through the object's `id`.

```hcl
terraform {
  required_providers {
    restapi = { source = "Mastercard/restapi" }
  }
}

variable "dumbmonit_token" {
  type      = string
  sensitive = true
}

variable "ntfy_token" {
  type      = string
  sensitive = true
}

provider "restapi" {
  uri                  = "https://monit.example.org"
  write_returns_object = true
  id_attribute         = "id"
  headers = {
    Authorization = "Bearer ${var.dumbmonit_token}"
    Content-Type  = "application/json"
  }
}

resource "restapi_object" "website" {
  path = "/api/targets"
  data = jsonencode({
    name          = "Website"
    kind          = "http"
    address       = "https://example.org"
    interval_secs = 60
    tags          = { env = "prod" }
  })
}

resource "restapi_object" "ntfy" {
  path = "/api/notify/channels"
  data = jsonencode({
    name     = "Phone"
    kind     = "ntfy"
    enabled  = true
    settings = { topic = "ops-alerts" }
    secrets  = { token = var.ntfy_token }
  })
  # The API never returns secrets: do not diff them.
  ignore_all_server_changes = true
}

resource "restapi_object" "backup_window" {
  path = "/api/alerts/silences"
  data = jsonencode({
    name     = "Sunday backups"
    enabled  = true
    schedule = {
      kind         = "weekly"
      days         = [6]
      start_minute = 120
      end_minute   = 240
      timezone     = "Europe/Paris"
    }
  })
}
```

Check the exact fields of each kind (HTTP options, the channel's secret name)
in `/api/openapi.json` or the interactive reference described on the
[HTTP API](../reference/api.md) page. To attach the silence to one monitor, add
`target_id = jsondecode(restapi_object.website.api_response).id`.

### With only built-in features

Without a third-party provider, `terraform_data` and `local-exec` can call `curl`.
This creates the object but does not track later changes or delete it, so prefer
`restapi` or the provider for anything long-lived.

```hcl
resource "terraform_data" "website" {
  provisioner "local-exec" {
    command = <<-EOT
      curl -fsS -X POST "$DUMBMONIT_URL/api/targets" \
        -H "Authorization: Bearer $DUMBMONIT_TOKEN" \
        -H "Content-Type: application/json" \
        -d '{"name":"Website","kind":"http","address":"https://example.org","interval_secs":60,"tags":{}}'
    EOT
    environment = {
      DUMBMONIT_URL   = "https://monit.example.org"
      DUMBMONIT_TOKEN = var.dumbmonit_token
    }
  }
}
```

## Ansible

`ansible.builtin.uri` is enough. A `201` means created; a second run would create
a duplicate, so check first (`GET /api/targets`) or use `changed_when` as below.

```yaml
- name: DumbMonit objects
  hosts: localhost
  gather_facts: false
  vars:
    dumbmonit_url: https://monit.example.org
    headers:
      Authorization: "Bearer {{ dumbmonit_token }}"   # from Ansible Vault
  tasks:
    - name: List existing monitors
      ansible.builtin.uri:
        url: "{{ dumbmonit_url }}/api/targets"
        headers: "{{ headers }}"
      register: existing

    - name: Create the monitor if it is missing
      ansible.builtin.uri:
        url: "{{ dumbmonit_url }}/api/targets"
        method: POST
        headers: "{{ headers }}"
        body_format: json
        body:
          name: Website
          kind: http
          address: https://example.org
          interval_secs: 60
          tags: {}
        status_code: 201
      when: "'https://example.org' not in (existing.json | map(attribute='address') | list)"

    - name: Create a notification channel
      ansible.builtin.uri:
        url: "{{ dumbmonit_url }}/api/notify/channels"
        method: POST
        headers: "{{ headers }}"
        body_format: json
        body:
          name: Phone
          kind: ntfy
          enabled: true
          settings: { topic: ops-alerts }
        status_code: [201, 400, 409]   # tolerate "already exists"
      register: channel
      changed_when: channel.status == 201

    - name: Silence alerts during the maintenance window
      ansible.builtin.uri:
        url: "{{ dumbmonit_url }}/api/alerts/silences"
        method: POST
        headers: "{{ headers }}"
        body_format: json
        body:
          name: "Patching {{ ansible_date_time.date | default('window') }}"
          enabled: true
          schedule:
            kind: once
            starts_at: "2027-03-01T22:00:00Z"
            ends_at: "2027-03-01T23:00:00Z"
        status_code: 201
```

A typical use is to open a silence at the start of a patching playbook and delete
it at the end (`DELETE /api/alerts/silences/{id}`, with the `id` from the creation
response).

## Which one to use

- Whole instance in Git, reviewed and applied by CI: **configuration files**.
- Infrastructure already in Terraform, one object at a time: **the provider**,
  or `restapi` if you do not want to build it.
- A one-off or a step inside a playbook (a silence during patching): **plain API calls**.
