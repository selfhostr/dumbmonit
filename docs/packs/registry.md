# Community pack registry

A registry is a static `index.json` that lists integration packs and says where
to download each one. Anyone can host one (a Git repository on GitHub Pages is
enough), and an administrator can then browse it and install packs from
**Settings → Integration packs → Community registry**.

The registry is **off by default**: until an address is set, DumbMonit never
contacts any registry.

## Using a registry

1. Open **Add monitor** and choose **Community packs** in the Import category, or
   open **Settings → Integration packs → Community registry**.
2. While no address is set, press **Use the official registry** to use
   `https://dumbmonit.selfhostr.io/hub/registry/index.json`, or paste the address
   of another registry's `index.json` in Settings and save.
3. Search the list. Each pack shows its author, licence and version. **Install** adds it;
   **Update to x.y.z** appears when the registry has a newer version than the
   one installed. Once installed, the pack's device types appear in the
   **Add monitor** catalogue.

The address is stored in the settings and travels with the
[backup](../install/backup.md). Setting it and installing from it are written to
the audit log (`pack.registry_settings`, `pack.registry_installed`).

## Format of `index.json`

```json
{
  "packs": [
    {
      "id": "shelly-plug",
      "name": "Shelly plug (Gen2+)",
      "version": "1.2.0",
      "description": "Power, voltage and temperature of Shelly plugs.",
      "author": "Jane Doe",
      "license": "MIT",
      "sha256": "9f2c…64 hexadecimal characters…",
      "url": "packs/shelly-plug-1.2.0.yaml",
      "min_version": "0.21.0"
    }
  ]
}
```

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | The pack's `id`. It must match the `id` inside the YAML. |
| `name` | yes | Name shown in the list. |
| `version` | yes | [Semantic version](https://semver.org). Compared with the installed one to offer updates. |
| `description`, `author` | no | Shown in the list. |
| `license` | yes | SPDX identifier, see below. |
| `sha256` | yes | SHA-256 of the pack file, 64 hexadecimal characters. |
| `url` | yes | Address of the pack YAML. Absolute, or relative to the index. |
| `min_version` | no | Oldest DumbMonit that can read the pack. An older server shows the pack as not available. |

A line that is invalid (missing field, bad checksum, duplicate `id`) is
skipped and does not hide the others. The index is limited to 1 MiB and a pack
to 512 KiB.

Compute the checksum with `sha256sum shelly-plug-1.2.0.yaml`. Publish a new
file name for each version and change `sha256` whenever the file changes.

## Accepted licences

Only these SPDX identifiers are accepted: `MIT`, `Apache-2.0`, `BSD-*`
(for example `BSD-3-Clause`), `ISC`, `MPL-2.0`, `CC0-1.0` and `Unlicense`.
Expressions such as `MIT OR GPL-3.0` are refused. A pack under another licence
is listed as not available and cannot be installed from the registry (you can
still [install the file by hand](first-pack.md)).

## Hosting one on GitHub Pages

1. Create a repository, for example `my-packs`, with a `packs/` folder holding
   the YAML files and an `index.json` at the root.
2. In **Settings → Pages**, publish the branch (root folder).
3. The registry address is then
   `https://<user>.github.io/my-packs/index.json`; relative `url` values such as
   `packs/shelly-plug-1.2.0.yaml` resolve next to it.
4. To publish a pack, add its file, compute its checksum, add or update its
   line in `index.json` and push.

Any other static host works the same way, including an internal web server.

## Security notes

- **A pack is code you choose to run.** It cannot run commands, but it sends
  HTTP requests to your devices and creates a device type. Only add registries
  you trust, and read a pack before installing it, as with any pack.
- **Integrity.** DumbMonit installs a pack only if the downloaded file matches
  the `sha256` of the index, so a file swapped on the host without the index
  being changed is refused. The index itself is trusted: use HTTPS and protect
  the repository that holds it.
- **Same checks as a manual install.** The file goes through the usual pack
  validation, and a pack cannot use the id of a built-in monitor type.
- **Outbound requests.** The index and the packs are fetched by the server,
  never by the browser, with the same protection as notifications: DumbMonit
  never contacts itself or its embedded VictoriaMetrics, redirects to another
  host are not followed, and sizes and time are capped. Addresses with a user
  name or password are refused.
- **Private networks.** A registry on your own network is allowed, like a
  notification webhook.

## API

`GET /api/packs/registry` lists the registry (administrator or any API token),
`PUT /api/packs/registry/settings` sets its address (`{"url": "…"}`, empty to
turn it off) and `POST /api/packs/registry/install` installs one pack
(`{"id": "shelly-plug"}`). See the [API reference](../reference/api.md).
