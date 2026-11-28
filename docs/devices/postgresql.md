# PostgreSQL

Beyond "the port answers": a database that has reached `max_connections`, a transaction left open for hours, a replica that stopped streaming, a standby that replays minutes late, deadlocks, an old transaction ID heading for wraparound. DumbMonit logs in with a role that can only read the statistics views, runs one query, then reads the server's health on the same connection.

Setup, ports and options are described under [PostgreSQL in Services](services.md#postgresql). This page lists what is measured and the exact grants.

## Network ports to open

| Port | Protocol | Purpose |
|---|---|---|
| 5432 | TCP | PostgreSQL (default port) |

Choose the encryption with the **Encryption** option: `prefer` (default), `require`, `verify-full` (certificate and name checked) or `disable`.

## What it watches

All metrics are prefixed `dumbmonit_postgres_`. Every read is independent and read-only (statistics views only). The full list is in [Server health](services.md#server-health); the ones asked for most often:

| Need | Metrics |
|---|---|
| Availability, version, uptime | `probe_success`, `version_info`, `uptime_seconds`, `in_recovery` |
| Connections used out of the limit | `connections`, `max_connections`, `connections_used_percent` |
| Queries per second | rate of `database_commits_total` + `database_rollbacks_total` |
| Long-running queries | `longest_query_seconds`, `long_running_queries` (active for more than 60 seconds) |
| Replication | `replica_streaming`, `replica_lag_seconds`, `replica_lag_bytes` on a primary; `standby_replay_lag_seconds`, `standby_receiving` on a standby |
| Database sizes | `database_size_bytes` (up to 20 databases) |
| Cache hit ratio | `cache_hit_percent` |
| Deadlocks | `database_deadlocks_total` |

The [built-in rules](../alerting/rules.md) that apply: PostgreSQL connections near the limit, long transaction, session idle in transaction, lock waits, replica not streaming, replica lagging, deadlocks, cache hit ratio, WAL archiving, transaction ID wraparound, plus Monitor unreachable.

## Create a least-privilege role

```
CREATE ROLE dumbmonit LOGIN PASSWORD 'choose-a-long-password';
GRANT pg_monitor TO dumbmonit;
```

`pg_monitor` is a predefined read-only role (PostgreSQL 10 and later): it lets the role see every session and the replication views, and grants no access to any table. Without it, the connection, lock and replication metrics only cover the role's own sessions. Allow the connection in `pg_hba.conf`:

```
host all dumbmonit 10.0.0.5/32 scram-sha-256
```

!!! warning
    The check opens a real connection on every poll: on an instance close to its connection limit, give it a longer interval.
