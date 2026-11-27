# Custom checks (scripts and Nagios plugins)

Some things only the machine itself can tell: the age of the last backup file,
a queue length read from a local database, a licence about to expire, a RAID
controller with its own command-line tool. A **custom check** runs your own
script, or any [Nagios / Monitoring Plugins](https://www.monitoring-plugins.org/)
plugin (`check_disk`, `check_http`, `check_mysql`…), on a machine that already
runs the DumbMonit agent, and turns its result into a monitor with a status,
alerts and graphs.

The monitor is set up in DumbMonit, but **the machine decides what may run**:
the agent refuses every custom check until its own `agent.yaml` says
`scripts: true`, and it only runs files of the directories that same file
allows. See [Security](#security) below.

## Network ports to open

From the agent to the DumbMonit server:

| Port | Protocol | Purpose |
|---|---|---|
| 8443 | TCP | Readings and commands: the encrypted agent channel (the one the agent already uses) |

Nothing is opened on the monitored machine: the check travels in the
acknowledgement of the agent's next report, and the result comes back with the
report after it.

## Setup

1. Enable custom checks in the `agent.yaml` of the machine (`/etc/dumbmonit` on Linux, `C:\ProgramData\DumbMonit` on Windows), then restart the agent.

    ```yaml
    scripts: true
    ```

2. Put the script in the script directory (`scripts.d` next to `agent.yaml`), executable and writable by its owner only.

    ```bash
    sudo install -m 0755 check_backup.sh /etc/dumbmonit/scripts.d/
    ```

3. For the Nagios plugins of the distribution, add their directory to `scripts_dirs` instead of copying them.

    ```yaml
    scripts_dirs: [/etc/dumbmonit/scripts.d, /usr/lib/nagios/plugins]
    ```

4. Here, pick that agent, enter the command and one argument per row: no shell is involved, so pipes and variables stay plain text.
5. Choose the output format: `nagios` for plugins (exit code and performance data), `json` or `prometheus` for your own scripts.

The **address** of the monitor is just a unique name for the check, for
example `nas01/disk`. The **interval** of the monitor is how often the agent
runs the check (60 s by default, 10 s at least).

!!! warning
    The agent runs nothing until `scripts: true` is set on the machine, and only files of its own script directories: the server cannot make it run anything else.

### Options

| Option | Default | What it does |
|---|---|---|
| Agent that runs the check | — | The DumbMonit agent of the machine where the script lives. Required. |
| Command | — | A file name of the agent's script directories (`check_disk`, `backup-age.sh`), or its absolute path. Required. |
| Arguments | none | One argument per row, passed as is (`argv`): `;`, pipes, `$()` and `*` are not interpreted. |
| Output format | `nagios` | `nagios`, `json` or `prometheus`, see below. |
| Timeout (seconds) | `10` | From 1 to 120. Past it, the check and every process it started are killed. |
| Environment variables | none | Up to 16, added to a minimal environment (`PATH`, `LANG=C`). |

**Credentials.** The monitor can hold a token, or a user name and a password,
stored encrypted like any other credential. The agent writes them where an
argument says `{{secret.token}}`, `{{secret.username}}` or
`{{secret.password}}`, and also passes them as the environment variables
`DUMBMONIT_SECRET_TOKEN`, `DUMBMONIT_SECRET_USERNAME` and
`DUMBMONIT_SECRET_PASSWORD`. Prefer the variables when your script can read
them: arguments are visible to every user of the machine in `ps`.

Every check also receives `DUMBMONIT_CHECK_ID` (the monitor id).

## Agent configuration

These keys go in `agent.yaml` on the machine (or the matching environment
variables). Restart the agent after a change.

| Key | Variable | Default | What it does |
|---|---|---|---|
| `scripts` | `DUMBMONIT_AGENT_SCRIPTS` | `false` | Allow custom checks at all. |
| `scripts_dirs` | `DUMBMONIT_AGENT_SCRIPTS_DIRS` (comma-separated) | `scripts.d` next to `agent.yaml` (`/etc/dumbmonit/scripts.d`; `C:\ProgramData\DumbMonit\scripts` on Windows) | Directories whose files may be run. A bare command name is looked up in them, in this order. |
| `scripts_user` | `DUMBMONIT_AGENT_SCRIPTS_USER` | the agent's account | Linux and other Unix systems: run the checks under this account (a name of `/etc/passwd`, or `uid:gid`). Recommended when the agent runs with full privileges. |
| `scripts_max_concurrent` | — | `4` | Checks run at the same time, from 1 to 16. |

A complete example, with the plugins of Debian or Ubuntu
(`apt install monitoring-plugins`):

```yaml
scripts: true
scripts_dirs:
  - /etc/dumbmonit/scripts.d
  - /usr/lib/nagios/plugins
scripts_user: nagios
```

On Windows, the default directory is `C:\ProgramData\DumbMonit\scripts`; it
inherits the permissions of `C:\ProgramData\DumbMonit`, writable by
administrators only. `.exe`, `.com`, `.bat`, `.cmd` and `.ps1` files are run;
a `.ps1` runs with `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy
Bypass -File`.

## Output formats

### `nagios`

The [Nagios plugin output](https://nagios-plugins.org/doc/guidelines.html#AEN200)
conventions, which every Nagios, Icinga, Naemon and Monitoring Plugins check
follows:

| Exit code | Status | In DumbMonit |
|---|---|---|
| 0 | OK | Up. |
| 1 | WARNING | Up, and the **Custom check warning** alert fires after two minutes. |
| 2 | CRITICAL | Down, with the check's message: the **Monitor unreachable** alert, as for any device that is down. |
| 3 (or anything else, or killed by a signal) | UNKNOWN | Shown as a configuration problem with the message, never notified. |

The first line is the message. What follows `|` is **performance data**,
`'label'=value[unit];[warn];[crit];[min];[max]`, separated by spaces, on the
first line and on any line after the first `|` of the long output:

```text
DISK OK - free space: / 3326 MB (56%); | /=2643MB;5948;5958;0;5968
/ 15272 MB (77%);
/boot 68 MB (69%); | /boot=68MB;88;93;0;98
/home=69357MB;253404;253409;0;253414
```

Units are converted to base units: `ms`, `us` and `s` to seconds, `B`, `KB`,
`MB`, `GB` and `TB` to bytes (powers of 1024, as the plugins mean them), `%`
kept, `c` marks a counter. A range threshold (`10:20`, `~:10`, `@5:15`) keeps
its upper bound, or its lower one when it has none. A value `U` (undetermined)
or an unknown unit is skipped, and at most 64 values are kept per run.

### `json`

For your own scripts, one JSON object on standard output:

```json
{
  "status": "warning",
  "message": "2 backups older than 26 h",
  "metrics": {
    "late_backups": 2,
    "oldest_backup_age": {"value": 97200, "unit": "s", "warn": 93600, "crit": 172800}
  }
}
```

`status` is `ok`, `warning`, `critical` or `unknown` (also `up`, `degraded`,
`down`, or the codes 0 to 3); without it, the exit code decides as for a
Nagios plugin. A metric is a number, or an object with `value` and optionally
`unit`, `warn`, `crit`, `min` and `max`.

### `prometheus`

The [text exposition format](https://prometheus.io/docs/instrumenting/exposition_formats/):
each sample becomes a value named as it is written
(`backup_size_bytes{repo="photos"}`), and `# TYPE … counter` marks counters.
The status comes from the exit code.

Whatever the format, standard output is read up to 64 KiB (the rest is read
and dropped), and when it gives no message the first line of standard error
is used.

## Examples

### Disk space with `check_disk`

Command `check_disk`, arguments one per row:

| Argument |
|---|
| `-w` |
| `20%` |
| `-c` |
| `10%` |
| `-p` |
| `/` |

WARNING under 20 % free, CRITICAL under 10 %, and the used space of `/` as a
graph (`dumbmonit_script_value{name="/"}`, in bytes).

### A local web application with `check_http`

Command `check_http`, arguments `-H`, `localhost`, `-p`, `8080`, `-u`,
`/health`, `-s`, `"status":"ok"`, `-w`, `2`, `-c`, `5`. Checks a service that
listens on the loopback interface only, which the server could not reach. The
response time comes back as `dumbmonit_script_value{name="time"}`, in seconds.

### MySQL or MariaDB with `check_mysql`

Give the monitor a **Username / password** credential, then command
`check_mysql` with the arguments `-H`, `127.0.0.1`, `-u`,
`{{secret.username}}`, `-p`, `{{secret.password}}`, `-S` (replication status
too). The password is stored encrypted in DumbMonit and only ever written into
the command line on the machine.

A dedicated account with no rights is enough:

```sql
CREATE USER 'dumbmonit'@'127.0.0.1' IDENTIFIED BY 'a long random password';
GRANT REPLICATION CLIENT ON *.* TO 'dumbmonit'@'127.0.0.1';
```

### Your own script (JSON)

`/etc/dumbmonit/scripts.d/backup-age.sh`, format `json`, no argument:

```bash
#!/bin/sh
# Age of the newest backup file, in seconds.
last=$(find /srv/backups -name '*.tar.zst' -printf '%T@\n' | sort -n | tail -1)
if [ -z "$last" ]; then
  echo '{"status": "critical", "message": "no backup file found"}'
  exit 0
fi
age=$(( $(date +%s) - ${last%.*} ))
status=ok
[ "$age" -gt 93600 ] && status=warning
[ "$age" -gt 172800 ] && status=critical
printf '{"status": "%s", "message": "last backup %d h ago", "metrics": {"backup_age": {"value": %d, "unit": "s"}}}\n' \
  "$status" $((age / 3600)) "$age"
```

```bash
sudo install -m 0755 backup-age.sh /etc/dumbmonit/scripts.d/
```

### Windows (PowerShell)

`C:\ProgramData\DumbMonit\scripts\check-service-queue.ps1`, format `nagios`:

```powershell
$depth = (Get-ChildItem 'D:\Queue\pending' -File).Count
if ($depth -gt 500) { "CRITICAL - $depth messages waiting | depth=$depth;100;500;0"; exit 2 }
if ($depth -gt 100) { "WARNING - $depth messages waiting | depth=$depth;100;500;0"; exit 1 }
"OK - $depth messages waiting | depth=$depth;100;500;0"; exit 0
```

## Metrics

Written when the result reaches the server, at the time the check ran, with
the usual `target` label of the monitor:

| Metric | Meaning |
|---|---|
| `dumbmonit_script_status` | 0 OK, 1 WARNING, 2 CRITICAL, 3 UNKNOWN (also when the check could not run). |
| `dumbmonit_script_warning` | 1 while the check returns WARNING, 0 otherwise. |
| `dumbmonit_script_duration_seconds` | How long the run took. |
| `dumbmonit_script_value{name="…"}` | One series per performance value or JSON metric; `unit` label when it has one. |
| `dumbmonit_script_value_warning`, `_critical`, `_min`, `_max` | The thresholds and bounds the check gave, same `name` label. |
| `dumbmonit_script_last_result_age_seconds` | Age of the last result, at each scheduler pass. |

Thresholds on `dumbmonit_script_value` can be added as rules like any other
metric, for example on one `name`.

## Status at a glance

| Situation | Shown as |
|---|---|
| The agent has not collected the check yet (new monitor) | Waiting, for up to three intervals. |
| The agent received it but never answered | Down: the agent is probably too old for custom checks; update it. |
| `scripts: true` missing, file not found or refused | Configuration problem, with the agent's reason. Not notified. |
| Killed after its timeout | Down (timed out). |
| No result for three intervals | Down: the agent stopped reporting. |

## Security

Custom checks are designed so that a compromised DumbMonit server, or someone
who steals an administrator session, still cannot run arbitrary commands on
your machines:

- **Off by default, switched on by the machine.** The agent answers every
  check with a refusal until `scripts: true` is in its own `agent.yaml`, a
  file only the machine's administrator can write.
- **An allow-list of directories.** Only files directly inside a directory of
  `scripts_dirs` run. The path is resolved with symbolic links followed, and
  must land directly in one of those directories: a link pointing elsewhere,
  a subdirectory, `..` or a network path are refused.
- **Permissions checked on every run.** On Unix the file must be executable,
  and neither the file nor its directory may be writable by group or others,
  or belong to an account other than the agent's or the system's. On Windows,
  keep the directory writable by administrators only (the default under
  `C:\ProgramData\DumbMonit`).
- **No shell, no interpreter on demand.** The program and its arguments go to
  the system as they are. Shells, interpreters, privilege tools and programs
  that run or write what their arguments say (`sh`, `bash`, `python*`,
  `perl`, `sudo`, `env`, `curl`, `tar`, `cmd`, `powershell`…) are never run,
  even when present in an allowed directory; so are the Nagios plugins that
  run another command or copy a file where they are told (`negate`, `urlize`,
  `check_by_ssh`, `check_log`).
- **A clean environment.** The environment is emptied, then `PATH` and
  `LANG=C` are set. Variables that load code or change what runs (`PATH`,
  `LD_*`, `DYLD_*`, `BASH_ENV`, `PYTHON*`, `PERL5*`, `NODE_OPTIONS`,
  `COMSPEC`…) are refused, by the server when you save the monitor and again
  by the agent.
- **Bounded.** Standard input is closed, output is read up to 64 KiB, and past
  the timeout (120 s at most) the whole process group is killed — on Windows,
  the process tree.
- **Least privilege.** With `scripts_user`, checks run under an account
  without rights, with no supplementary groups, even if the agent itself runs
  with full privileges.

What remains possible to someone in control of the server is to run the
programs you allowed, **with arguments of their choosing**. Keep that in mind
before allowing a whole plugin directory: `/usr/lib/nagios/plugins` holds
dozens of programs, some of which connect to other hosts or read files. A
directory of your own scripts that each take few or no arguments is the
tightest setup. Results the agent sends back are only accepted for the
checks assigned to that very agent.

## Troubleshooting

The reason of a refusal is shown on the monitor, and the agent logs it too
(`custom check refused`):

| Message | Fix |
|---|---|
| custom checks are disabled on this machine | Add `scripts: true` to `agent.yaml` and restart the agent. |
| was not found in the script directories | Check the file name, and `scripts_dirs`. |
| is not directly inside an allowed script directory | The path, or a symbolic link, leads out of the directories: copy the file in. |
| is writable by group or others (chmod go-w) | `sudo chmod go-w` the file and its directory. |
| is not executable (chmod +x) | `sudo chmod 0755` the file. |
| is a shell, an interpreter or a tool that runs or writes what its arguments say | Write a small script that calls it with fixed arguments, and run that script instead. |
