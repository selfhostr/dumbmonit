#!/usr/bin/env python3
"""Données et vérifications du test de mise à jour (scripts/upgrade-test.sh).

Le script shell pilote les conteneurs ; ce programme parle à l'API d'une
instance (HTTPS auto-signé accepté) et ne dépend que de la bibliothèque
standard. Une phase par sous-commande :

  seed      sur l'image de départ : compte admin, comptes, moniteurs de types
            variés (dont un agent simulé, un battement de cœur et une
            intégration à secret chiffré jointe sur le faux service), règles,
            silences, canaux, page de statut, incident, équipes, réglages.
  before    attend l'historique, le photographie (configuration, séries
            VictoriaMetrics, sondes) et exporte une sauvegarde.
  after     sur l'image candidate, même volume : relit tout, compare, rejoue
            les secrets (sonde, envoi de test, sauvegarde), l'agent, le jeton
            d'API, la page publique ; exporte et simule une restauration.
  restore   sur une instance neuve de l'image candidate : restaure la
            sauvegarde faite par l'ancienne version et recompare.

Chaque contrôle imprime `[ok]`, `[KO]` ou `[..]` (non couvert, non bloquant) ;
le code de sortie est 1 dès qu'un `[KO]` est imprimé.
"""
import argparse
import json
import os
import ssl
import sys
import time
import urllib.error
import urllib.request

ADMIN = "upgrader"
PASSWORD = "correct horse battery 42"
PASSPHRASE = "upgrade test backup passphrase 2026"
HA_TOKEN = "ha-llat-upgrade-test-0123456789"
NTFY_TOKEN = "tk_upgradeSECRET0123456789"
SNMP_COMMUNITY = "s3cret-community"
PVE_TOKEN = "root@pam!dm=00000000-0000-0000-0000-000000000000"
USERS = {
    "alice": ("operator", "alice upgrade pass 42"),
    "bob": ("viewer", "bob upgrade pass 4242"),
}
PAGE_SLUG = "upgrade-test"
PAGE_TITLE = "Upgrade test"
AGENT_HOST = "upgrade-agent"
AGENT_MACHINE = "upgrade-test-machine-0001"
AGENT_METRIC = "cpu_usage_percent"
AGENT_POINTS = 30

TARGET_FIELDS = [
    "address", "kind", "tags", "interval_secs", "enabled", "credential_kind",
    "has_secret", "credential_public", "group_name",
]
RULE_FIELDS = [
    "name", "query", "operator", "threshold", "for_secs", "severity", "enabled",
    "clear_threshold", "repeat_secs",
]

# Ce qu'une restauration doit au moins rendre à l'identique (les identifiants
# changent, les positions et les valeurs par défaut ajoutées par une version
# plus récente n'ont pas à être comparés).
RESTORE_FIELDS = {
    "users": ["role", "disabled"],
    "targets": ["address", "kind", "group_name", "credential_kind", "has_secret", "parent"],
    "rules": ["name", "query", "threshold", "severity", "enabled"],
    "channels": ["kind", "has_secret"],
    "silences": ["comment", "enabled"],
    "pages": ["title", "published", "items"],
    "teams": ["members", "grants"],
}


class Failure(Exception):
    """Une étape dont dépend la suite a échoué."""


class Report:
    def __init__(self):
        self.failed = 0

    def ok(self, message):
        print(f"  [ok] {message}", flush=True)

    def ko(self, message):
        self.failed += 1
        print(f"  [KO] {message}", flush=True)

    def warn(self, message):
        print(f"  [..] {message}", flush=True)

    def check(self, condition, ok_message, ko_message):
        (self.ok if condition else self.ko)(ok_message if condition else ko_message)
        return bool(condition)


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        return None


class Api:
    """Client de l'API : cookie de session tenu à la main, CSRF `X-Requested-With`."""

    def __init__(self, base):
        self.base = base.rstrip("/")
        self.cookies = {}
        context = ssl.create_default_context()
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPSHandler(context=context), _NoRedirect()
        )

    def request(self, method, path, body=None, bearer=None, headers=None, timeout=90):
        send = {"X-Requested-With": "dumbmonit", "Accept": "application/json"}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            send["Content-Type"] = "application/json"
        if bearer:
            send["Authorization"] = f"Bearer {bearer}"
        elif self.cookies:
            send["Cookie"] = "; ".join(f"{k}={v}" for k, v in self.cookies.items())
        send.update(headers or {})
        request = urllib.request.Request(self.base + path, data=data, method=method, headers=send)
        try:
            response = self.opener.open(request, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        except (urllib.error.URLError, OSError) as error:
            raise Failure(f"{method} {path} : {error}") from error
        status = response.status if hasattr(response, "status") else response.code
        text = response.read().decode("utf-8", "replace")
        if not bearer:
            for line in response.headers.get_all("Set-Cookie") or []:
                name, _, value = line.split(";", 1)[0].partition("=")
                if value:
                    self.cookies[name.strip()] = value.strip()
                else:
                    self.cookies.pop(name.strip(), None)
        try:
            parsed = json.loads(text) if text else None
        except ValueError:
            parsed = text
        return status, parsed

    def call(self, method, path, body=None, **kwargs):
        status, parsed = self.request(method, path, body, **kwargs)
        if not 200 <= status < 300:
            raise Failure(f"{method} {path} -> HTTP {status} : {str(parsed)[:300]}")
        return parsed

    def get(self, path):
        """Lecture tolérante : `None` si la route n'existe pas (ou plus) ou est refusée."""
        status, parsed = self.request("GET", path)
        return parsed if status == 200 else None

    def get_list(self, path):
        """Comme `get`, mais `None` aussi quand la réponse n'est pas une liste."""
        parsed = self.get(path)
        return parsed if isinstance(parsed, list) else None

    def login(self, username=ADMIN, password=PASSWORD):
        self.cookies = {}
        self.call("POST", "/api/auth/login", {"username": username, "password": password})


# --- Aides ------------------------------------------------------------------


def now_ms():
    return int(time.time() * 1000)


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def save(path, value):
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(value, handle, indent=1, sort_keys=True)


def mock_entries(path):
    entries = []
    try:
        with open(path, encoding="utf-8") as handle:
            for line in handle:
                try:
                    entries.append(json.loads(line))
                except ValueError:
                    pass
    except OSError:
        pass
    return entries


def history(api, query, start, end, step):
    """Nombre de points et horodatage du plus ancien, sur une fenêtre FIXE."""
    path = f"/api/metrics/query_range?query={urllib.request.quote(query, safe='')}"
    path += f"&start={start}&end={end}&step={step}"
    series = api.call("GET", path)
    points = [value for one in series for value in one["values"]]
    return len(points), min((float(v[0]) for v in points), default=0.0)


def last_sample_time(api, query):
    """Horodatage (secondes) du dernier échantillon de la série, 0 s'il n'y en a pas."""
    path = "/api/metrics/query?query=" + urllib.request.quote(f"timestamp({query})", safe="")
    series = api.call("GET", path)
    values = [float(v[1]) for one in series for v in one["values"]]
    return max(values, default=0.0)


# --- Photographie de la configuration ----------------------------------------


def snapshot(api):
    """Configuration relue par l'API, indexée par clé naturelle (nom, identifiant
    stable) pour pouvoir aussi servir à comparer une instance restaurée. Une
    section dont la route n'existe pas dans cette version est simplement omise."""
    snap = {}
    targets = api.get_list("/api/targets")
    names = {t["id"]: t["name"] for t in targets or []}
    if targets is not None:
        snap["targets"] = {
            t["name"]: {
                "_id": t["id"],
                **{key: t.get(key) for key in TARGET_FIELDS},
                "parent": names.get(t.get("parent_id")),
            }
            for t in targets
        }

    channels = api.get_list("/api/notify/channels")
    channel_names = {c["id"]: c["name"] for c in channels or []}
    if channels is not None:
        snap["channels"] = {
            c["name"]: {
                "_id": c["id"],
                **{k: c.get(k) for k in ("kind", "enabled", "settings", "has_secret", "policy")},
            }
            for c in channels
        }

    rules = api.get_list("/api/alerts/rules")
    if rules is not None:
        out = {}
        for rule in rules:
            key = rule.get("uid") or rule["name"]
            if rule.get("builtin"):
                # Les règles livrées évoluent d'une version à l'autre : seul le
                # choix de l'utilisateur (active ou non) doit survivre.
                out[key] = {"builtin": True, "enabled": rule.get("enabled")}
            else:
                out[key] = {
                    "_id": rule["id"],
                    "builtin": False,
                    **{k: rule.get(k) for k in RULE_FIELDS},
                    "channels": sorted(
                        channel_names.get(c, f"#{c}") for c in rule.get("channels") or []
                    ),
                }
        snap["rules"] = out

    silences = api.get_list("/api/alerts/silences")
    if silences is not None:
        snap["silences"] = {
            s["name"]: {
                "_id": s["id"],
                "comment": s.get("comment"),
                "matchers": s.get("matchers"),
                "schedule": s.get("schedule"),
                "enabled": s.get("enabled"),
                "target": names.get(s.get("target_id")),
            }
            for s in silences
        }

    pages = api.get_list("/api/status-pages")
    page_slugs = {p["id"]: p["slug"] for p in pages or []}
    if pages is not None:
        snap["pages"] = {
            p["slug"]: {
                "_id": p["id"],
                **{k: p.get(k) for k in ("title", "description", "published", "show_uptime_days")},
                "items": [[names.get(i.get("target_id")), i.get("label")] for i in p.get("items", [])],
            }
            for p in pages
        }

    incidents = api.get_list("/api/incidents")
    if incidents is not None:
        snap["incidents"] = {
            i["title"]: {
                **{k: i.get(k) for k in ("kind", "status", "severity")},
                "page": page_slugs.get(i.get("page_id")),
                "updates": len(i.get("updates") or []),
            }
            for i in incidents
        }

    users = api.get_list("/api/users")
    if users is not None:
        snap["users"] = {
            u["username"]: {
                "_id": u["id"],
                **{k: u.get(k) for k in ("display_name", "role", "disabled")},
            }
            for u in users
        }

    teams = api.get("/api/teams")
    if isinstance(teams, dict):
        snap["teams"] = {
            t["name"]: {
                "description": t.get("description"),
                "idp_groups": sorted(t.get("idp_groups") or []),
                "members": sorted(m["username"] for m in t.get("members") or []),
                "grants": sorted(f"{g['folder']}:{g['level']}" for g in t.get("grants") or []),
            }
            for t in teams.get("teams", [])
        }
        snap["team_settings"] = {"_": teams.get("settings") or {}}

    instance = api.get("/api/instance/settings")
    if isinstance(instance, dict):
        snap["instance"] = {
            "_": {k: instance.get(k) for k in ("name", "language", "timezone", "setup_completed")}
        }

    policy = api.get("/api/notify/policy")
    if isinstance(policy, dict):
        # Seulement ce que le test a réglé : un défaut d'une version plus récente
        # n'est pas une donnée de l'utilisateur.
        snap["notify_policy"] = {
            "_": {k: policy.get(k) for k in ("batch_window_secs", "max_per_hour")}
        }

    tokens = api.get("/api/tokens")
    if isinstance(tokens, list):
        snap["api_tokens"] = {
            t["name"]: {"prefix": t.get("prefix"), "scope": t.get("scope")} for t in tokens
        }

    agent_tokens = api.get("/api/agent/tokens")
    if isinstance(agent_tokens, list):
        snap["agent_tokens"] = {
            t["prefix"]: {k: t.get(k) for k in ("max_uses", "uses", "revoked_at", "relay")}
            for t in agent_tokens
        }
    return snap


def subset_diff(before, after, path=""):
    """Différences où `after` ne reprend pas `before` : un champ ajouté par une
    version plus récente n'en est pas une, un champ changé ou perdu en est une."""
    if isinstance(before, dict):
        if not isinstance(after, dict):
            return [f"{path or '.'} : {before!r} -> {after!r}"]
        diffs = []
        for key, value in before.items():
            if key not in after:
                diffs.append(f"{path}.{key} : {value!r} -> (absent)")
            else:
                diffs += subset_diff(value, after[key], f"{path}.{key}")
        return diffs
    return [] if before == after else [f"{path or '.'} : {before!r} -> {after!r}"]


def compare(before, after, report):
    for section, objects in sorted(before.items()):
        if section not in after:
            report.ko(f"{section} : lu avant la mise à jour, introuvable après")
            continue
        bad = 0
        for key, fields in objects.items():
            now = after[section].get(key)
            if now is None:
                if section == "rules" and fields.get("builtin"):
                    report.warn(f"règle livrée {key} retirée par la nouvelle version")
                else:
                    report.ko(f"{section} « {key} » a disparu")
                    bad += 1
                continue
            diffs = subset_diff(fields, now)
            if diffs:
                bad += 1
                report.ko(f"{section} « {key} » modifié (avant -> après) : {'; '.join(diffs)}")
        if not bad:
            report.ok(f"{section} : {len(objects)} objet(s) inchangé(s)")


def project(snap):
    """Réduit une photographie aux champs qu'une restauration doit rendre."""
    out = {}
    for section, fields in RESTORE_FIELDS.items():
        if section not in snap:
            continue
        objects = {}
        for key, obj in snap[section].items():
            if section == "rules" and obj.get("builtin"):
                continue
            objects[key] = {f: obj.get(f) for f in fields}
        out[section] = objects
    return out


# --- seed ---------------------------------------------------------------------


def optional(report, label, action):
    """Crée un objet que la version de départ n'a peut-être pas encore : un
    échec est signalé mais ne bloque pas (la section ne sera pas comparée)."""
    try:
        return action()
    except Failure as error:
        report.warn(f"non créé sur la version de départ ({label}) : {error}")
        return None


def agent_batch(api, token, secret, points, now):
    identity = {
        "hostname": AGENT_HOST,
        "os": "linux",
        "os_version": "Debian GNU/Linux 12",
        "arch": "x86_64",
        "agent_version": "0.0.0-upgrade-test",
        "machine_id": AGENT_MACHINE,
        "binding_supported": True,
    }
    samples = [
        {"metric": AGENT_METRIC, "labels": {}, "value": value, "kind": "gauge", "ts_ms": ts}
        for ts, value in points
    ]
    batch = {"protocol": 1, "identity": identity, "sent_at_ms": now, "samples": samples}
    headers = {"X-Dumbmonit-Agent-Secret": secret} if secret else {}
    return api.request("POST", "/api/ingest", batch, bearer=token, headers=headers)


def phase_seed(a, report):
    api = Api(a.url)
    state = {"mock": f"{a.mock_ip}:{a.mock_port}", "old_version": a.old_version}
    mock = state["mock"]

    api.call(
        "POST", "/api/auth/setup",
        {"setup_code": a.setup_code, "username": ADMIN, "password": PASSWORD},
    )
    api.login()
    print("  compte administrateur créé", flush=True)
    # Depuis 0.21, les modules facultatifs sont coupés sur une installation neuve :
    # le test les allume pour couvrir leurs données. Absent avant 0.21 : rien à faire.
    status, _ = api.request("PUT", "/api/settings/features", {
        "wall": True, "sla_reports": True, "status_pages": True, "slo": True,
    })
    if 200 <= status < 300:
        print("  modules facultatifs allumés", flush=True)

    def create_target(label, body):
        state.setdefault("targets", {})[label] = api.call("POST", "/api/targets", body)["id"]
        return state["targets"][label]

    web = create_target("web", {
        "name": "web mock", "address": mock, "kind": "http", "group_name": "Lab",
        "tags": {"scheme": "http"}, "interval_secs": 15,
    })
    create_target("tcp", {
        "name": "tcp mock", "address": mock, "kind": "tcp", "parent_id": web,
        "tags": {}, "interval_secs": 15,
    })
    create_target("ping", {
        "name": "ping loopback", "address": "127.0.0.1", "kind": "ping",
        "tags": {"allow_private_targets": "true"}, "interval_secs": 15,
    })
    create_target("snmp", {
        "name": "switch", "address": "192.0.2.10", "kind": "snmp", "interval_secs": 60,
        "credential": {"type": "snmp_community", "community": SNMP_COMMUNITY},
    })
    create_target("proxmox", {
        "name": "pve", "address": "192.0.2.20", "kind": "proxmox", "interval_secs": 60,
        "credential": {"type": "api_token", "token": PVE_TOKEN},
    })
    # Intégration à secret chiffré, jointe sur le faux Home Assistant : une sonde
    # qui réussit prouve que le jeton a été relu et déchiffré.
    optional(report, "Home Assistant", lambda: create_target("homeassistant", {
        "name": "home assistant", "address": a.mock_ip, "kind": "homeassistant",
        "interval_secs": 30, "group_name": "Lab",
        "tags": {"scheme": "http", "port": str(a.mock_port), "repairs": "false"},
        "credential": {"type": "api_token", "token": HA_TOKEN},
    }))
    optional(report, "battement de cœur", lambda: create_target("push", {
        "name": "nightly backup", "address": "nightly-backup", "kind": "push",
        "tags": {"expected_interval": "1h", "grace": "5m"},
    }))
    print(f"  moniteurs : {state['targets']}", flush=True)

    # Comptes, équipes, réglages.
    for name, (role, password) in USERS.items():
        user = optional(report, f"compte {name}", lambda n=name, r=role, p=password: api.call(
            "POST", "/api/users",
            {"username": n, "display_name": n.title(), "role": r, "password": p},
        ))
        if user:
            state.setdefault("users", {})[name] = user["id"]
    users = state.get("users", {})
    if users:
        optional(report, "équipes", lambda: [
            api.call("POST", "/api/teams", {
                "name": "Ops", "description": "On call",
                "members": [users[n] for n in ("alice",) if n in users],
                "grants": [{"folder": "Lab", "level": "operate"}],
            }),
            api.call("POST", "/api/teams", {
                "name": "Readers", "description": "Read only",
                "members": [users[n] for n in ("bob",) if n in users],
                "grants": [{"folder": "Lab", "level": "read"}],
            }),
            api.call("PUT", "/api/teams/settings", {"unassigned_access": "none"}),
        ])
    optional(report, "réglages d'instance", lambda: api.call(
        "PUT", "/api/instance/settings",
        {"name": "Upgrade lab", "language": "fr", "timezone": "Europe/Paris",
         "setup_completed": True},
    ))
    optional(report, "politique de notification", lambda: api.call(
        "PUT", "/api/notify/policy", {"batch_window_secs": 20, "max_per_hour": 120},
    ))

    # Canal (avec secret), règles, silences.
    channel = api.call("POST", "/api/notify/channels", {
        "name": "Upgrade test ntfy", "kind": "ntfy",
        "settings": {"topic": "upgrade-test-topic", "server_url": f"http://{mock}"},
        "secrets": {"token": NTFY_TOKEN},
    })
    state["channel"] = channel["id"]
    api.call("POST", "/api/alerts/rules", {
        "name": "Upgrade test: target down", "query": "dumbmonit_up == 0", "operator": ">",
        "threshold": 0, "for_secs": 60, "severity": "warning", "channels": [channel["id"]],
    })
    optional(report, "seconde règle", lambda: api.call("POST", "/api/alerts/rules", {
        "name": "Upgrade test: recovery mode", "query": "dumbmonit_homeassistant_recovery_mode",
        "operator": ">", "threshold": 0, "for_secs": 300, "severity": "critical",
    }))
    rules = api.get_list("/api/alerts/rules") or []
    builtin = next((r for r in rules if r.get("builtin")), None)
    if builtin:
        # Choix de l'utilisateur sur une règle livrée : doit survivre.
        optional(report, "règle livrée désactivée", lambda: api.call(
            "POST", f"/api/alerts/rules/{builtin['id']}/enable", {"enabled": False},
        ))
        state["disabled_builtin"] = builtin.get("uid") or builtin["name"]
    api.call("POST", "/api/alerts/silences", {
        "name": "Upgrade test silence", "comment": "kept", "matchers": {"host": "switch"},
        "schedule": {"kind": "weekly", "days": [6], "start_minute": 120, "end_minute": 240,
                     "utc_offset_minutes": 60},
    })
    optional(report, "silence ponctuel", lambda: api.call("POST", "/api/alerts/silences", {
        "name": "Upgrade test once", "comment": "window", "target_id": state["targets"]["tcp"],
        "schedule": {"kind": "once", "starts_at": "2031-01-01T00:00:00Z",
                     "ends_at": "2031-01-01T02:00:00Z"},
    }))

    # Page de statut, incident.
    page = api.call("POST", "/api/status-pages", {
        "title": PAGE_TITLE, "slug": PAGE_SLUG, "published": True,
        "description": "Published by the upgrade test",
    })
    state["page"] = page["id"]
    items = [{"target_id": web, "label": "Web"}, {"target_id": state["targets"]["tcp"]}]
    api.call("PUT", f"/api/status-pages/{page['id']}/items", items)
    optional(report, "incident", lambda: api.call("POST", "/api/incidents", {
        "title": "Upgrade test incident", "kind": "incident", "status": "investigating",
        "severity": "minor", "page_id": page["id"], "body": "Looking into it.",
    }))

    # Jetons : API (lecture) et agent simulé.
    token = optional(report, "jeton d'API", lambda: api.call(
        "POST", "/api/tokens", {"name": "upgrade-test read", "scope": "read"},
    ))
    if token:
        state["api_token"] = token["secret"]
    enrol = optional(report, "jeton d'agent", lambda: api.call(
        "POST", "/api/agent/tokens", {"reusable": False},
    ))
    if enrol:
        state["agent_token"] = enrol["secret"]
        now = now_ms()
        points = [(now - (AGENT_POINTS - i) * 60_000, 10.0 + i) for i in range(AGENT_POINTS)]
        status, ack = agent_batch(api, enrol["secret"], None, points, now)
        if status == 200 and isinstance(ack, dict):
            state["agent_target"] = ack["target_id"]
            state["agent_secret"] = ack.get("agent_secret")
            print(f"  agent simulé enrôlé : cible {ack['target_id']}, "
                  f"lié={ack.get('bound')}", flush=True)
        else:
            report.warn(f"agent simulé non enrôlé (HTTP {status}) : {str(ack)[:200]}")
            state.pop("agent_token", None)

    save(os.path.join(a.work, "state.json"), state)


# --- before -------------------------------------------------------------------


def probe(api, target_id):
    status, body = api.request("POST", f"/api/targets/{target_id}/probe")
    return status, body


def phase_before(a, report):
    api = Api(a.url)
    api.login()
    state = load(os.path.join(a.work, "state.json"))
    targets = state["targets"]

    # Ce que la version de départ sait déjà faire avec les secrets : si elle ne le
    # fait pas, la version candidate n'a pas à le faire non plus.
    if "homeassistant" in targets:
        status, body = probe(api, targets["homeassistant"])
        state["ha_probe_before"] = status == 200
        print(f"  sonde Home Assistant (départ) : HTTP {status}", flush=True)
        if status != 200:
            report.warn(f"la sonde Home Assistant échoue déjà au départ : {str(body)[:200]}")
    status, body = api.request("POST", f"/api/notify/channels/{state['channel']}/test")
    state["ntfy_before"] = status == 200
    print(f"  envoi de test du canal ntfy (départ) : HTTP {status}", flush=True)
    if status != 200:
        report.warn(f"l'envoi de test échoue déjà au départ : {str(body)[:200]}")

    if "push" in targets:
        monitor = api.get(f"/api/targets/{targets['push']}/push") or {}
        state["push_token"] = monitor.get("token")
        if state["push_token"]:
            status, _ = api.request("GET", f"/api/push/{state['push_token']}")
            state["push_before"] = status == 200
            print(f"  battement de cœur (départ) : HTTP {status}", flush=True)

    # Historique : assez de points pour que la comparaison veuille dire quelque chose.
    agent_query = (
        f'dumbmonit_{AGENT_METRIC}{{target="{state["agent_target"]}"}}'
        if "agent_target" in state else None
    )
    deadline = time.time() + a.wait
    windows = {}
    while True:
        end = now_ms()
        start = end - 3_600_000
        up, up_oldest = history(api, "dumbmonit_up", start, end, 15)
        agent = (0, 0.0)
        if agent_query:
            agent = history(api, agent_query, start, end, 60)
        enough = up >= 4 and (not agent_query or agent[0] >= AGENT_POINTS // 2)
        if enough:
            windows["up"] = {"query": "dumbmonit_up", "start": start, "end": end, "step": 15,
                             "count": up, "oldest": up_oldest}
            if agent_query:
                windows["agent"] = {"query": agent_query, "start": start, "end": end, "step": 60,
                                    "count": agent[0], "oldest": agent[1]}
            break
        if time.time() > deadline:
            raise Failure(
                f"historique insuffisant après {a.wait} s : dumbmonit_up={up} point(s), "
                f"agent={agent[0]} point(s)"
            )
        time.sleep(2)
    state["windows"] = windows
    for name, window in windows.items():
        print(f"  historique {name} : {window['count']} point(s), plus ancien {window['oldest']}",
              flush=True)

    snap = snapshot(api)
    save(os.path.join(a.work, "before.json"), snap)
    print("  sections photographiées : " + ", ".join(sorted(snap)), flush=True)

    status, bundle = api.request(
        "POST", "/api/backup", {"passphrase": PASSPHRASE, "include_account_secrets": True},
    )
    if status == 200 and isinstance(bundle, dict):
        save(os.path.join(a.work, "old-backup.json"), bundle)
        state["old_backup"] = True
        print("  sauvegarde exportée par la version de départ", flush=True)
    else:
        state["old_backup"] = False
        report.warn(f"export de sauvegarde impossible au départ (HTTP {status}) : {str(bundle)[:200]}")
    save(os.path.join(a.work, "state.json"), state)


# --- after --------------------------------------------------------------------


def phase_after(a, report):
    state = load(os.path.join(a.work, "state.json"))
    before = load(os.path.join(a.work, "before.json"))
    targets = state["targets"]
    api = Api(a.url)

    try:
        api.login()
        report.ok("connexion avec le même mot de passe")
    except Failure as error:
        report.ko(f"connexion refusée avec le mot de passe d'avant la mise à jour : {error}")
        return
    for name, (_role, password) in USERS.items():
        if name in state.get("users", {}):
            try:
                Api(a.url).login(name, password)
                report.ok(f"connexion de {name} (empreinte de mot de passe relue)")
            except Failure as error:
                report.ko(f"connexion de {name} impossible : {error}")

    after = snapshot(api)
    compare(before, after, report)
    builtin = state.get("disabled_builtin")
    if builtin and "rules" in after:
        rule = after["rules"].get(builtin)
        if rule is not None:
            report.check(rule.get("enabled") is False, f"règle livrée {builtin} toujours désactivée",
                         f"règle livrée {builtin} de nouveau active")

    notes = api.get("/api/upgrade-notes")
    if isinstance(notes, dict):
        came_from = notes.get("upgraded_from")
        if came_from is None:
            report.warn("la nouvelle version ne note pas la version dont elle vient")
        else:
            report.check(came_from == a.old_version,
                         f"montée de version notée ({came_from} -> {notes.get('current')})",
                         f"version d'origine notée {came_from}, attendu {a.old_version}")

    # Secrets : sondes réelles contre le faux service.
    entries_from = a.new_start
    if "homeassistant" in targets:
        status, body = probe(api, targets["homeassistant"])
        if status == 200:
            report.ok("secret Home Assistant déchiffré : la sonde présente le bon jeton")
        elif state.get("ha_probe_before"):
            report.ko(f"sonde Home Assistant en échec après la mise à jour (HTTP {status}) : "
                      f"{str(body)[:300]}")
        else:
            report.warn("sonde Home Assistant non vérifiée (échouait déjà au départ)")
    status, body = api.request("POST", f"/api/notify/channels/{state['channel']}/test")
    seen = [e for e in mock_entries(a.mock_log)
            if e["method"] == "POST" and e["t"] >= entries_from
            and e.get("auth") == f"Bearer {NTFY_TOKEN}"]
    if status == 200 and seen:
        report.ok("secret du canal ntfy déchiffré : l'envoi de test présente le bon jeton")
    elif state.get("ntfy_before"):
        report.ko(f"envoi de test du canal ntfy en échec après la mise à jour (HTTP {status}, "
                  f"{len(seen)} requête(s) avec le bon jeton) : {str(body)[:300]}")
    else:
        report.warn("envoi de test du canal ntfy non vérifié (échouait déjà au départ)")

    # Battement de cœur : l'URL appelée par les cron ne doit pas changer.
    if state.get("push_token"):
        status, _ = api.request("GET", f"/api/push/{state['push_token']}")
        if state.get("push_before"):
            report.check(status == 200, "URL du battement de cœur toujours acceptée",
                         f"URL du battement de cœur refusée (HTTP {status})")

    # Agent simulé : jeton, liaison et cible survivent ; l'agent n'a rien à refaire.
    if state.get("agent_token"):
        now = now_ms()
        status, ack = agent_batch(api, state["agent_token"], state.get("agent_secret"),
                                  [(now, 42.0)], now)
        if status == 200 and isinstance(ack, dict):
            report.check(ack.get("target_id") == state["agent_target"],
                         "agent : lot accepté, même machine (jeton et liaison conservés)",
                         f"agent : cible {ack.get('target_id')} au lieu de {state['agent_target']}")
        else:
            report.ko(f"agent : lot refusé après la mise à jour (HTTP {status}) : {str(ack)[:300]}")

    if state.get("api_token"):
        status, _ = api.request("GET", "/api/targets", bearer=state["api_token"])
        report.check(status == 200, "jeton d'API toujours valide",
                     f"jeton d'API refusé (HTTP {status})")

    page = api.request("GET", f"/api/public/status/{PAGE_SLUG}")
    if page[0] == 200 and isinstance(page[1], dict) and (page[1].get("page") or {}).get("title") == PAGE_TITLE:
        report.ok("page de statut publique servie, même titre")
    else:
        report.ko(f"page de statut publique /api/public/status/{PAGE_SLUG} : HTTP {page[0]} "
                  f"{str(page[1])[:200]}")

    # Historique : sur la même fenêtre, tous les points d'avant sont là (un point
    # de plus n'est pas une perte), et le plus ancien n'a pas bougé.
    for name, window in sorted(state["windows"].items()):
        count, oldest = history(api, window["query"], window["start"], window["end"],
                                window["step"])
        report.check(
            count >= window["count"] and oldest == window["oldest"],
            f"historique {name} conservé ({window['count']} -> {count} point(s), "
            f"plus ancien {window['oldest']} -> {oldest})",
            f"historique {name} perdu : {window['count']} -> {count} point(s), "
            f"plus ancien {window['oldest']} -> {oldest}",
        )

    # La supervision reprend : un échantillon écrit APRÈS le démarrage.
    query = f'dumbmonit_up{{target="{targets["web"]}"}}'
    deadline = time.time() + 90
    newest = 0.0
    while time.time() < deadline:
        newest = last_sample_time(api, query)
        if newest >= a.new_start:
            break
        time.sleep(3)
    report.check(newest >= a.new_start, "la supervision a repris (échantillon écrit après la mise à jour)",
                 "aucun échantillon dumbmonit_up écrit après le démarrage de la nouvelle version "
                 "(90 s)")

    # Sauvegarde : tous les secrets sont déchiffrés pour la produire, puis une
    # restauration simulée relit le lot sans rien écrire.
    status, bundle = api.request(
        "POST", "/api/backup", {"passphrase": PASSPHRASE, "include_account_secrets": True},
    )
    if status == 200 and isinstance(bundle, dict) and bundle:
        report.ok("sauvegarde exportée (tous les secrets relus et re-scellés)")
        save(os.path.join(a.work, "new-backup.json"), bundle)
        status, result = api.request(
            "POST", "/api/backup/restore",
            {"bundle": bundle, "passphrase": PASSPHRASE, "apply": False},
        )
        report.check(status == 200 and isinstance(result, dict) and result.get("applied") is False,
                     "restauration simulée de cette sauvegarde acceptée",
                     f"restauration simulée refusée (HTTP {status}) : {str(result)[:300]}")
    else:
        report.ko(f"export de la sauvegarde impossible (HTTP {status}) : {str(bundle)[:300]}")


# --- restore ------------------------------------------------------------------


def phase_restore(a, report):
    state = load(os.path.join(a.work, "state.json"))
    before = load(os.path.join(a.work, "before.json"))
    bundle = load(os.path.join(a.work, "old-backup.json"))

    api = Api(a.url)
    api.call("POST", "/api/auth/setup",
             {"setup_code": a.setup_code, "username": ADMIN, "password": PASSWORD})
    api.login()
    status, result = api.request(
        "POST", "/api/backup/restore", {"bundle": bundle, "passphrase": PASSPHRASE, "apply": True},
    )
    if status != 200 or not isinstance(result, dict):
        report.ko(f"restauration de la sauvegarde de l'ancienne version refusée (HTTP {status}) : "
                  f"{str(result)[:400]}")
        return
    report.ok(f"sauvegarde de {result.get('source_version', '?')} restaurée sur une instance neuve "
              f"({result.get('created')} créé(s), {result.get('updated')} mis à jour, "
              f"{result.get('skipped')} ignoré(s))")
    for warning in result.get("warnings") or []:
        report.warn(f"avertissement de la restauration : {warning}")

    api = Api(a.url)
    try:
        api.login()
    except Failure as error:
        report.ko(f"connexion impossible après la restauration (comptes) : {error}")
        return
    compare(project(before), project(snapshot(api)), report)

    # Les secrets sont repartis en clair dans la sauvegarde puis chiffrés avec la
    # clé de CETTE instance : les rejouer contre le faux service le prouve.
    restored = {t["name"]: t["id"] for t in api.get_list("/api/targets") or []}
    if state.get("ha_probe_before") and "home assistant" in restored:
        status, body = probe(api, restored["home assistant"])
        report.check(status == 200,
                     "secret Home Assistant restauré : la sonde présente le bon jeton",
                     f"sonde Home Assistant en échec après la restauration (HTTP {status}) : "
                     f"{str(body)[:300]}")
    channels = {c["name"]: c["id"] for c in api.get_list("/api/notify/channels") or []}
    if state.get("ntfy_before") and "Upgrade test ntfy" in channels:
        started = time.time() - 1
        status, body = api.request("POST", f"/api/notify/channels/{channels['Upgrade test ntfy']}/test")
        seen = [e for e in mock_entries(a.mock_log)
                if e["method"] == "POST" and e["t"] >= started
                and e.get("auth") == f"Bearer {NTFY_TOKEN}"]
        report.check(status == 200 and seen,
                     "secret du canal ntfy restauré : l'envoi de test présente le bon jeton",
                     f"envoi de test du canal restauré en échec (HTTP {status}) : {str(body)[:300]}")


# --- Programme ----------------------------------------------------------------


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("phase", choices=["seed", "before", "after", "restore"])
    parser.add_argument("--url", required=True)
    parser.add_argument("--work", required=True)
    parser.add_argument("--setup-code", default="")
    parser.add_argument("--mock-ip", default="")
    parser.add_argument("--mock-port", type=int, default=0)
    parser.add_argument("--mock-log", default="")
    parser.add_argument("--old-version", default="")
    parser.add_argument("--new-start", type=float, default=0.0,
                        help="secondes epoch du démarrage de la version candidate")
    parser.add_argument("--wait", type=int, default=150,
                        help="secondes d'attente de l'historique avant la mise à jour")
    a = parser.parse_args()

    report = Report()
    try:
        {"seed": phase_seed, "before": phase_before, "after": phase_after,
         "restore": phase_restore}[a.phase](a, report)
    except Failure as error:
        report.ko(str(error))
    return 1 if report.failed else 0


if __name__ == "__main__":
    sys.exit(main())
