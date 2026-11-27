# MySQL and MariaDB

Beyond "the port answers": a database that accepts connections but has run out of them, a replica that silently stopped following its source, slow queries piling up, a buffer pool too small for the data. DumbMonit logs in with a read-only account, runs one query, then reads the server status on the same connection.

Setup, ports and options are described under [MySQL and MariaDB in Services](services.md#mysql-and-mariadb). This page lists what is measured and the exact grants.

## Network ports to open

| Port | Protocol | Purpose |
|---|---|---|
| 3306 | TCP | MySQL or MariaDB (default port) |

Choose the encryption with the **Encryption** option: `prefer` (default), `require`, `verify-full` (certificate and name checked) or `disable`.

## What it watches

All metrics are prefixed `dumbmonit_mysql_`. Every read is independent and read-only (`SHOW` and `information_schema`): a missing privilege or an older server hides that metric and never makes the check fail. Turn them off with the "Server health metrics" option.

| Metric | What | Labels |
|---|---|---|
| `connections`, `max_connections`, `connections_used_percent` | open connections, `max_connections`, and the share used | |
| `threads_running`, `max_used_connections` | statements executing now, peak of connections since start | |
| `queries_total` | statements executed (counter; its rate is queries per second) | |
| `slow_queries_total` | statements slower than `long_query_time` (counter) | |
| `aborted_connects_total`, `aborted_clients_total` | failed logins and dropped clients (counters) | |
| `buffer_pool_hit_percent` | share of InnoDB reads served from memory since start | |
| `buffer_pool_read_requests_total`, `buffer_pool_disk_reads_total` | counters behind the hit ratio | |
| `long_running_queries`, `longest_query_seconds` | statements running for more than 60 seconds, and the longest (replication and event threads excluded) | |
| `replicas_configured` | replication channels of this server (0 on a source that replicates nothing) | |
| `replica_running`, `replica_io_running`, `replica_sql_running` | per channel: both replication threads running | `channel` |
| `replica_lag_seconds` | `Seconds_Behind_Source`; absent while replication is broken | `channel` |
| `database_size_bytes` | data and index size of each schema (top 10) | `database` |
| `read_only`, `uptime_seconds`, `version_info` | read-only flag, uptime, version | `version`, `flavor` |

The [built-in rules](../alerting/rules.md) that apply: MySQL connections near the limit, MySQL replication stopped, MySQL replica lagging, MySQL restarted, plus Monitor unreachable.

## Create a least-privilege account

```
CREATE USER 'dumbmonit'@'%' IDENTIFIED BY 'choose-a-long-password';
GRANT PROCESS, REPLICATION CLIENT ON *.* TO 'dumbmonit'@'%';
```

- **PROCESS**: see the statements of every user (long-running queries). Without it only the account's own sessions are visible.
- **REPLICATION CLIENT**: `SHOW REPLICA STATUS`. On MySQL 8.0.22 and later this privilege still works; on MariaDB 10.5.9 and later the same read is allowed by `REPLICA MONITOR`, and `REPLICATION CLIENT` is its alias, so the statement above is enough.
- Schema sizes only list the schemas on which the account holds some privilege. To see an application schema, grant it something harmless such as `GRANT SHOW VIEW ON appdb.* TO 'dumbmonit'@'%';`.

The account can read no data and change nothing. Connection time and query time are recorded separately.

!!! warning
    The check opens a real connection on every poll: on an instance close to its connection limit, give it a longer interval.
