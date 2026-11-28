# Papra (beta)

Whether Papra, the self-hosted document archive, still answers and can reach its database, how many organisations it holds, and how many documents and how much storage each one uses.

!!! note "Beta"
    Written from Papra's source code and API contracts, not yet tested against a live instance. Tell us what you see.

## Network ports to open

From the DumbMonit server (or its relay agent) to the device:

| Port | Protocol | Purpose |
|---|---|---|
| 1221 | TCP | API (HTTP, default) |
| 443 | TCP | Only with the https scheme behind a reverse proxy |

The failure worth watching: **the database no longer answers** while the front page is still served. Papra's health route says so, and DumbMonit alerts on Monitor unreachable when the server itself stops answering.

## What it watches

All metrics are prefixed `dumbmonit_papra_`.

| Metric | What | Labels |
|---|---|---|
| `database_healthy` | 1 while `/api/health` reports a working database | |
| `organizations` | organisations the API key's owner belongs to | |
| `documents`, `documents_size_bytes` | documents in an organisation, and their size | `organization` |
| `documents_deleted`, `documents_deleted_size_bytes` | documents in the trash, and their size | `organization` |
| `scrape_errors` | calls that failed on the last probe | |

At most 32 organisations are read per probe. DumbMonit reads counts and sizes only, never a document name, content or file.

## Create an API key in Papra

1. In Papra, open Settings → API keys → Create API key and name it as follows.

    ```
    dumbmonit
    ```

2. Tick only the permissions listed below, and copy the key (it starts with ppapi_).

3. In DumbMonit, enter Papra's address and paste the key.

### Required permissions

- **organizations:read**: List of organisations
- **documents:read**: Document counts and sizes, never the documents themselves

!!! warning
    A Papra API key acts as its owner: keep it only in DumbMonit. DumbMonit reads counts and sizes only, never a document name or file.

## Credentials

| Credential | Fields |
|---|---|
| API key | Sent as `Authorization: Bearer …`. |

Address: a host name (`papra.lan`), `host:port`, or a full URL (`https://papra.example.com`). Options: protocol (HTTP by default), port (1221), certificate check and request timeout (15 s).
