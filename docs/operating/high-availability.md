# Running DumbMonit resiliently

How to keep monitoring available, and get it back quickly, when the machine
running DumbMonit fails. This page is about the choices; the settings are
documented on the pages it links to.

!!! note
    DumbMonit is a **single-active** system. It does not support running two
    active servers at once (active/active, load balancing). Resilience comes
    from keeping the data safe and the replacement ready.

## What there is to protect

| Data | Where | Protected by |
|---|---|---|
| Configuration, users, alert state | SQLite in `/data` | [Encrypted backups](../install/backup.md) |
| Credentials | Inside the database, encrypted | The instance secret (`/data/secret.key` or `DUMBMONIT_SECRET`) |
| Metrics history | VictoriaMetrics | An external VictoriaMetrics, or its own backups |

Keep the instance secret **separately** from the backups: a backup is useless
without it for the credentials it holds.

## 1. Move the metrics off the server

By default VictoriaMetrics is bundled in the container (`/data/vm`). Point
DumbMonit at one you run elsewhere with `DUMBMONIT_VM_URL`, as described in
[External VictoriaMetrics](../reference/external-victoriametrics.md). The server
then becomes almost stateless: replacing it does not lose the history, and a
replacement keeps drawing the same graphs at once.

## 2. Schedule encrypted backups, off the machine

- Turn on [scheduled local backups](../install/backup.md#scheduled-local-backups):
  an encrypted bundle on a schedule, with a retention.
- Add an [off-site destination](../install/backup.md#off-site-copies) (SFTP, S3
  and others) so a copy survives the loss of the machine, and the disk.
- Check now and then that backups still run (see
  [Knowing that it ran](../install/backup.md#knowing-that-it-ran)). Restore one on a spare instance a few times a year;
  an untested backup is a hope.
- Store the bundle passphrase and the instance secret in your password manager.

## 3. Cold standby: restoring on a new machine

A cold standby is a second host with Docker and the same compose file, stopped
or empty, that you start when the first one is lost.

1. Install the **same version** of DumbMonit on the new host.
2. Start it and open the setup page; create a temporary administrator.
3. Provide the instance secret if you use `DUMBMONIT_SECRET`.
4. Restore the latest bundle ([Restoring onto a fresh instance](../install/backup.md#restoring-onto-a-fresh-instance)),
   from the off-site copy if the local one is gone.
5. Point the name or the reverse proxy of the old server at the new one. Agents
   that list several addresses in `server_urls` find it on their own; the
   others reconnect once the name resolves to the new host.
6. Open the Overview and check that monitors report again.

**Estimated recovery time** (RTO) with the metrics already external and a
prepared host: **15 to 30 minutes**, mostly the restore and DNS. From scratch
(installing Docker, fetching the bundle, rebuilding the compose file) plan for
an hour. **Data loss** (RPO) is the age of the last backup: with a daily
schedule, up to a day of configuration changes. Metrics are not lost when
VictoriaMetrics is external; agents also keep their measurements while no
server answers, then send them.

## 4. Warm standby: built-in active/standby

If an hour without monitoring is too long, DumbMonit can keep a second server
ready that takes over by itself or on your command, from a replica of the
database in a shared directory. It needs an external VictoriaMetrics and
storage both hosts can write. See [High availability](../install/high-availability.md).
Even then, keep the off-site backups: the shared directory protects against the
loss of a server, not of the storage.

## 5. Health checks

- `GET /api/health` answers `200` while the server is up (liveness), and the
  container's own `dumbmonit healthcheck` uses it. A `restart: unless-stopped`
  policy brings the container back after a crash or a reboot.
- Watch DumbMonit from outside: have a second tool, or a small instance on
  another host, check the web address and an
  [agent heartbeat](../devices/push.md). A monitoring system cannot report its
  own total failure.
- Use [self-monitoring](../install/self-monitoring.md) to be told about a
  full disk, a stalled scheduler or a failing backup before they matter.

## What is not supported

- **Active/active**, or several servers behind a load balancer: two servers
  writing the same database would corrupt it, and the scheduler would probe and
  alert twice.
- **Sharing `/data` between two running containers**, on NFS or otherwise.
  Only one server may open a database at a time (the built-in standby respects
  this).
- **Automatic failover without the built-in standby**: a script that starts a
  second copy when the first stops answering risks two active servers.
