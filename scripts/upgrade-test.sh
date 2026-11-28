#!/usr/bin/env bash
# Test de mise à jour : une image publiée (OLD) crée de VRAIES données sur un
# volume, puis l'image candidate (NEW) reprend le MÊME volume. Vérifie que rien
# n'est perdu ni abîmé, secrets chiffrés compris.
#
#   scripts/upgrade-test.sh [NEW_IMAGE]
#
# Variables :
#   OLD_IMAGE         image de départ : `latest` (défaut, dernière release publiée),
#                     `previous-minor` (dernier correctif de la version mineure
#                     d'avant, pour une montée qui saute une version), ou une
#                     référence d'image complète
#   NEW_IMAGE         image candidate (défaut : ghcr.io/selfhostr/dumbmonit:edge)
#   UPGRADE_LOG_DIR   dossier des journaux (défaut : /tmp/dumbmonit-upgrade-logs-<pid>,
#                     gardé seulement en cas d'échec)
#   UPGRADE_SUMMARY_FILE  fichier Markdown où ajouter le résultat (en CI :
#                     $GITHUB_STEP_SUMMARY)
#   KEEP=1            garde volumes, conteneurs et réseau après coup (débogage)
#
# Déroulement (détails dans scripts/upgrade-test/data.py) :
#   1. OLD démarre sur un volume neuf ; compte admin, comptes, équipes, moniteurs
#      de types variés (http, tcp, ping, snmp, proxmox, Home Assistant à secret
#      chiffré, battement de cœur, agent simulé), règles, silences, canal ntfy à
#      secret, page de statut, incident, réglages. Un faux service HTTP (mock.py)
#      joue Home Assistant et ntfy et note les jetons qu'on lui présente.
#   2. Attente de l'historique VictoriaMetrics, photographie de la configuration,
#      export d'une sauvegarde par OLD, arrêt propre d'OLD.
#   3. NEW démarre sur le même volume : santé, migrations, journal ; chaque objet
#      est relu et comparé ; les secrets sont rejoués contre le faux service ;
#      l'agent, le jeton d'API, la page publique, l'historique, la reprise de la
#      supervision ; sauvegarde exportée et restauration simulée.
#   4. La sauvegarde d'OLD est restaurée sur une instance NEUVE de NEW, qui doit
#      retrouver la configuration et ses secrets.
#
# Prérequis : docker (Linux : le faux service est joint par l'adresse de la
# passerelle du réseau Docker), curl, git, python3. Les images sont lancées comme
# en production (uid 65532, système de fichiers en lecture seule, capacités
# retirées) : le docker-compose.yml publié doit continuer de démarrer sans retouche.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=selfhostr/dumbmonit
# Avant le passage à l'organisation selfhostr, les images étaient publiées ici.
LEGACY_REPO=laupernoe/dumbmonit
NEW_IMAGE=${1:-${NEW_IMAGE:-ghcr.io/$REPO:edge}}
OLD_SPEC=${OLD_IMAGE:-latest}
SETUP_CODE=UPGRD-TEST2
# Doit rester celui de scripts/upgrade-test/data.py (HA_TOKEN).
HA_TOKEN='ha-llat-upgrade-test-0123456789'

RUN_ID=$$
PREFIX=dmup-$RUN_ID
NET=$PREFIX
VOLUME=$PREFIX-data
VOLUME_FRESH=$PREFIX-fresh
C_OLD=$PREFIX-old
C_NEW=$PREFIX-new
C_FRESH=$PREFIX-fresh
WORK=$(mktemp -d)
LOGDIR=${UPGRADE_LOG_DIR:-${TMPDIR:-/tmp}/dumbmonit-upgrade-logs-$RUN_ID}
CHECKS=$LOGDIR/checks.log
MOCK_LOG=$WORK/mock.log
MOCK_PID=
SKIP=0
PORT=
SCHEME=
OLD_IMAGE=
OLD_VERSION=?
NEW_VERSION=?
mkdir -p "$LOGDIR"
: >"$CHECKS"

in_actions() { [ "${GITHUB_ACTIONS:-}" = true ]; }
step() { printf '\n==> %s\n' "$*"; }
group() { if in_actions; then printf '::group::%s\n' "$*"; else printf '\n--- %s ---\n' "$*"; fi; }
endgroup() { if in_actions; then printf '::endgroup::\n'; fi; }
strip_colors() { sed 's/\x1b\[[0-9;]*m//g'; }

# Journaux des conteneurs et du faux service, écrits dans $LOGDIR.
collect_logs() {
  local entry name role
  for entry in "$C_OLD:old" "$C_NEW:new" "$C_FRESH:fresh"; do
    name=${entry%%:*}
    role=${entry##*:}
    if docker inspect "$name" >/dev/null 2>&1; then
      docker logs "$name" 2>&1 | strip_colors >"$LOGDIR/$role.log" || true
    fi
  done
  if [ -s "$MOCK_LOG" ]; then cp "$MOCK_LOG" "$LOGDIR/mock.jsonl" || true; fi
}

show_logs() {
  local role
  for role in old new fresh; do
    [ -s "$LOGDIR/$role.log" ] || continue
    group "journal du conteneur « $role » (80 dernières lignes ; complet : $LOGDIR/$role.log)"
    tail -n 80 "$LOGDIR/$role.log"
    endgroup
  done
  if [ -s "$LOGDIR/mock.jsonl" ]; then
    group "requêtes reçues par le faux service (30 dernières)"
    tail -n 30 "$LOGDIR/mock.jsonl"
    endgroup
  fi
}

fail() {
  printf '\nECHEC : %s\n' "$*" >&2
  collect_logs
  show_logs >&2
  exit 1
}

write_summary() { # write_summary EXIT_CODE
  [ -n "${UPGRADE_SUMMARY_FILE:-}" ] && [ "$SKIP" = 0 ] || return 0
  {
    if [ "$1" = 0 ]; then
      printf '### Upgrade test: %s -> %s, passed\n\n' "${OLD_VERSION}" "${NEW_VERSION}"
    else
      printf '### Upgrade test: %s -> %s, FAILED\n\n' "${OLD_VERSION}" "${NEW_VERSION}"
    fi
    printf 'From `%s` to `%s`.\n\n```\n' "${OLD_IMAGE:-$OLD_SPEC}" "$NEW_IMAGE"
    tail -n 120 "$CHECKS" 2>/dev/null || true
    printf '```\n'
  } >>"$UPGRADE_SUMMARY_FILE" 2>/dev/null || true
}

cleanup() {
  local rc=$?
  trap - EXIT
  write_summary "$rc"
  if [ "${KEEP:-0}" = 1 ]; then
    echo "KEEP=1 : conteneurs $PREFIX-*, volumes $VOLUME et $VOLUME_FRESH, réseau $NET," \
      "dossier $WORK et journaux $LOGDIR conservés."
    exit "$rc"
  fi
  [ -z "$MOCK_PID" ] || kill "$MOCK_PID" >/dev/null 2>&1 || true
  docker rm -f "$C_OLD" "$C_NEW" "$C_FRESH" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" "$VOLUME_FRESH" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  rm -rf "$WORK"
  # Les journaux ne sont gardés qu'en cas d'échec, ou si l'appelant les a demandés.
  if [ "$rc" = 0 ] && [ -z "${UPGRADE_LOG_DIR:-}" ]; then rm -rf "$LOGDIR"; fi
  exit "$rc"
}
trap cleanup EXIT

for tool in docker curl git python3; do
  command -v "$tool" >/dev/null || { echo "$tool est requis." >&2; exit 2; }
done

# --- Choix de l'image de départ ---------------------------------------------

image_exists() { docker manifest inspect "$1" >/dev/null 2>&1; }

# Tag (vX.Y.Z) de la dernière release publiée.
latest_tag() {
  local tag='' auth=()
  if command -v gh >/dev/null; then
    tag=$(gh api "repos/$REPO/releases/latest" --jq .tag_name 2>/dev/null || true)
  fi
  if [ -z "$tag" ]; then
    [ -z "${GITHUB_TOKEN:-}" ] || auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
    # La réponse est une donnée JSON, lue par un code Python fixe : rien de
    # téléchargé n'est exécuté.
    local release
    release=$(curl -fsS ${auth[@]+"${auth[@]}"} \
      "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null || true)
    tag=$(python3 -I -c 'import json,sys; print(json.loads(sys.argv[1])["tag_name"])' \
      "$release" 2>/dev/null || true)
  fi
  printf '%s' "$tag"
}

resolve_latest() {
  # Le tag `latest` n'est posé que par un tag v* sur main : il désigne exactement
  # la dernière release. À défaut (image absente), on prend le dernier tag via gh.
  if image_exists "ghcr.io/$REPO:latest"; then echo "ghcr.io/$REPO:latest"; return; fi
  local tag
  tag=$(latest_tag)
  if [ -n "$tag" ] && image_exists "ghcr.io/$REPO:${tag#v}"; then
    echo "ghcr.io/$REPO:${tag#v}"; return
  fi
  if image_exists "ghcr.io/$LEGACY_REPO:latest"; then echo "ghcr.io/$LEGACY_REPO:latest"; return; fi
  echo "Impossible de trouver la dernière release (fixez OLD_IMAGE)." >&2
  exit 2
}

# Dernier correctif de la version mineure publiée juste avant celle de `latest`.
# Imprime rien (code 0) s'il n'y en a pas.
resolve_previous_minor() {
  local latest tag repo
  latest=$(latest_tag)
  [ -n "$latest" ] || { echo "Dernière release introuvable (gh ou api.github.com)." >&2; exit 2; }
  for tag in $(git ls-remote --tags --refs "https://github.com/$REPO.git" 'v*' 2>/dev/null \
    | python3 -I -c '
import re, sys
def parse(tag):
    m = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", tag)
    return tuple(int(x) for x in m.groups()) if m else None
current = parse(sys.argv[1])
found = []
for line in sys.stdin:
    if "refs/tags/" not in line:
        continue
    tag = line.split("refs/tags/", 1)[1].strip()
    version = parse(tag)
    if version and current and version[:2] < current[:2]:
        found.append((version, tag))
for _, tag in sorted(found, reverse=True):
    print(tag)
' "$latest"); do
    for repo in "$REPO" "$LEGACY_REPO"; do
      if image_exists "ghcr.io/$repo:${tag#v}"; then echo "ghcr.io/$repo:${tag#v}"; return; fi
    done
  done
}

case "$OLD_SPEC" in
  latest) OLD_IMAGE=$(resolve_latest) ;;
  previous-minor)
    OLD_IMAGE=$(resolve_previous_minor)
    if [ -z "$OLD_IMAGE" ]; then
      echo "Aucune version mineure publiée avant la dernière release : rien à tester."
      SKIP=1
      exit 0
    fi
    ;;
  *) OLD_IMAGE=$OLD_SPEC ;;
esac

# --- Aides -------------------------------------------------------------------

# Extrait un champ JSON : `jfield '<expr python sur d>'`, JSON sur stdin.
jfield() { python3 -I -c 'import sys,json; d=json.load(sys.stdin); print(eval(sys.argv[1]))' "$1"; }

# Exécute une phase de data.py, affiche et garde sa sortie ; rend son code.
run_py() {
  local rc
  set +e
  python3 -I -u "$HERE/upgrade-test/data.py" "$@" 2>&1 | tee -a "$CHECKS"
  rc=${PIPESTATUS[0]}
  set -e
  return "$rc"
}

start() { # start NOM VOLUME IMAGE : lance le conteneur, attend /api/health (PORT, SCHEME)
  local name=$1 volume=$2 image=$3 i
  docker run -d --name "$name" --network "$NET" -p 127.0.0.1::8080 -v "$volume:/data" \
    -e DUMBMONIT_SETUP_CODE="$SETUP_CODE" \
    --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges:true \
    --sysctl net.ipv4.ping_group_range="0 2147483647" "$image" >/dev/null \
    || fail "docker run $image"
  PORT=$(docker port "$name" 8080/tcp | head -1 | sed 's/.*://')
  for i in $(seq 1 90); do
    if [ "$(docker inspect -f '{{.State.Running}}' "$name")" != true ]; then
      fail "$image s'est arrêtée au démarrage"
    fi
    # Depuis la 0.16 le port web ne sert que du HTTPS (certificat auto-signé) : une requête en
    # clair reçoit une redirection sans corps. On garde le premier schéma qui rend du JSON.
    for SCHEME in https http; do
      if curl -fsk "$SCHEME://127.0.0.1:$PORT/api/health" >"$WORK/health" 2>/dev/null \
        && python3 -I -c 'import json,sys; json.load(open(sys.argv[1]))' "$WORK/health" 2>/dev/null; then
        return
      fi
    done
    sleep 1
  done
  fail "$image ne répond pas sur /api/health après 90 s"
}

stop() { docker stop -t 30 "$1" >/dev/null || fail "arrêt de $1"; }

# --- Mise en place -------------------------------------------------------------

step "Image de départ : $OLD_IMAGE"
step "Image candidate : $NEW_IMAGE"
docker pull -q "$OLD_IMAGE" >/dev/null || fail "pull $OLD_IMAGE"
if ! docker image inspect "$NEW_IMAGE" >/dev/null 2>&1; then
  docker pull -q "$NEW_IMAGE" >/dev/null || fail "pull $NEW_IMAGE"
fi
docker volume create "$VOLUME" >/dev/null
docker network create "$NET" >/dev/null
# Le faux service écoute sur l'hôte ; les conteneurs le joignent par la passerelle
# de leur réseau (adresse privée : jamais refusée par les gardes d'adresses).
MOCK_IP=$(docker network inspect -f '{{(index .IPAM.Config 0).Gateway}}' "$NET")
python3 -I "$HERE/upgrade-test/mock.py" --portfile "$WORK/mock.port" --log "$MOCK_LOG" \
  --ha-token "$HA_TOKEN" >"$LOGDIR/mock-stderr.log" 2>&1 &
MOCK_PID=$!
for _ in $(seq 1 50); do [ -s "$WORK/mock.port" ] && break; sleep 0.1; done
[ -s "$WORK/mock.port" ] || fail "le faux service n'a pas démarré (voir $LOGDIR/mock-stderr.log)"
MOCK_PORT=$(cat "$WORK/mock.port")
echo "faux service : $MOCK_IP:$MOCK_PORT"

# --- Départ : image publiée --------------------------------------------------

step "Démarrage de l'ancienne version"
start "$C_OLD" "$VOLUME" "$OLD_IMAGE"
OLD_VERSION=$(jfield 'd["version"]' <"$WORK/health")
echo "version de départ : $OLD_VERSION"
URL="$SCHEME://127.0.0.1:$PORT"

step "Création des données (version $OLD_VERSION)"
run_py seed --url "$URL" --work "$WORK" --setup-code "$SETUP_CODE" \
  --mock-ip "$MOCK_IP" --mock-port "$MOCK_PORT" --old-version "$OLD_VERSION" \
  || fail "création des données impossible sur $OLD_IMAGE"

step "Attente de l'historique, photographie, sauvegarde"
run_py before --url "$URL" --work "$WORK" --mock-log "$MOCK_LOG" \
  || fail "préparation de la comparaison impossible sur $OLD_IMAGE"

step "Arrêt propre de l'ancienne version"
stop "$C_OLD"
collect_logs

# --- Arrivée : image candidate sur le même volume -----------------------------

step "Démarrage de la version candidate sur le même volume"
NEW_START=$(date +%s)
start "$C_NEW" "$VOLUME" "$NEW_IMAGE"
NEW_VERSION=$(jfield 'd["version"]' <"$WORK/health")
echo "version candidate : $NEW_VERSION ($(jfield 'd.get("build","?")' <"$WORK/health"))"
URL="$SCHEME://127.0.0.1:$PORT"

OK=1
check() { printf '  [%s] %s\n' "$1" "$2" | tee -a "$CHECKS"; [ "$1" = ok ] || OK=0; }

step "Vérifications de démarrage"
[ "$(jfield 'd["status"]' <"$WORK/health")" = ok ] && check ok "/api/health : status ok" \
  || check KO "/api/health : $(cat "$WORK/health")"
[ "$(jfield 'd["database"]["ok"]' <"$WORK/health")" = True ] && check ok "base de données saine" \
  || check KO "base de données : $(cat "$WORK/health")"
[ "$(jfield 'd["victoria"]["ok"]' <"$WORK/health")" = True ] && check ok "VictoriaMetrics saine" \
  || check KO "VictoriaMetrics : $(cat "$WORK/health")"
# Les migrations s'appliquent avant « database ready » ; un échec arrête le serveur.
# grep sans -q : avec pipefail, -q ferme le tube tôt et docker logs meurt de SIGPIPE.
docker logs "$C_NEW" 2>&1 | strip_colors | grep 'database ready' >/dev/null \
  && check ok "migrations appliquées (database ready)" \
  || check KO "« database ready » absent du journal"

step "Relecture des données et des secrets"
run_py after --url "$URL" --work "$WORK" --mock-log "$MOCK_LOG" \
  --old-version "$OLD_VERSION" --new-start "$NEW_START" || OK=0

LOGS=$(docker logs "$C_NEW" 2>&1 | strip_colors)
if grep -Eq ' (ERROR|PANIC) |panicked at' <<<"$LOGS"; then
  check KO "erreurs dans le journal de la version candidate :"
  grep -E ' (ERROR|PANIC) |panicked at' <<<"$LOGS" | head -8
else
  check ok "aucune ERROR ni panic dans le journal"
fi
if grep -Eiq 'decrypt|cipher|déchiffr' <<<"$LOGS"; then
  check KO "le journal parle d'un secret illisible :"
  grep -Ei 'decrypt|cipher|déchiffr' <<<"$LOGS" | head -5
else
  check ok "aucun secret illisible signalé dans le journal"
fi

# --- Sauvegarde de l'ancienne version restaurée sur une instance neuve --------

if [ -s "$WORK/old-backup.json" ]; then
  step "Restauration de la sauvegarde de $OLD_VERSION sur une instance neuve de $NEW_VERSION"
  docker volume create "$VOLUME_FRESH" >/dev/null
  start "$C_FRESH" "$VOLUME_FRESH" "$NEW_IMAGE"
  run_py restore --url "$SCHEME://127.0.0.1:$PORT" --work "$WORK" --mock-log "$MOCK_LOG" \
    --setup-code "$SETUP_CODE" || OK=0
else
  check ok "restauration de la sauvegarde ancienne ignorée (aucune exportée par $OLD_VERSION)"
fi

collect_logs
printf '\n'
if [ "$OK" = 1 ]; then
  echo "SUCCES : $OLD_VERSION ($OLD_IMAGE) -> $NEW_VERSION ($NEW_IMAGE), données intactes ($SECONDS s)."
else
  fail "la mise à jour $OLD_VERSION -> $NEW_VERSION a abîmé des données (voir [KO] ci-dessus)."
fi
