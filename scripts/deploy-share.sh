#!/usr/bin/env bash
# Met à jour le jeu publié en permanence sur le homelab.
#
#   ./scripts/deploy-share.sh                 # math + build + books, puis redémarre
#   ./scripts/deploy-share.sh --code          # + reconstruit l'image (code SlotLab modifié)
#   ./scripts/deploy-share.sh --game tachyon  # un autre jeu (stack déjà créé)
#   ./scripts/deploy-share.sh --dry-run       # montre ce qui serait envoyé
#
# Le transfert est incrémental : ne repartent que les fichiers qui ont changé.
set -euo pipefail

GAME=skullperch
REBUILD_IMAGE=0
DRY=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --game) GAME="$2"; shift 2 ;;
    --code) REBUILD_IMAGE=1; shift ;;
    --dry-run) DRY="--dry-run"; shift ;;
    -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "option inconnue : $1" >&2; exit 2 ;;
  esac
done

HOST=${SLOTLAB_DEPLOY_HOST:-root@192.168.1.181}
REMOTE=/media/usb-disk/slotlab/games     # = /mnt/storage/slotlab/games vu du CT
SRC_REMOTE=$(dirname "$REMOTE")/src     # = /mnt/storage/slotlab/src vu du CT (docker build)
CONTAINER=${SLOTLAB_CONTAINER:-slotlab-share}
STACK_DIR=${SLOTLAB_STACK_DIR:-/opt/stacks/slotlab-share}
# Le hub relit son disque à chaud : plus besoin de recréer le conteneur, donc
# les visiteurs en cours ne perdent plus leur session.
HUB=${SLOTLAB_HUB_URL:-http://192.168.1.200:3610}
DEPLOY_JSON="$HOME/Library/Application Support/slotlab/deploy.json"
PUBLIC=${SLOTLAB_PUBLIC_URL:-https://skullperch.mcberry.fr}
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
STAKE=$(cd "$REPO/../.." && pwd)   # SideProjects/slotlab → racine StakeEngine
# Les jeux sont rangés par provider : StakeEngine/<provider>/<jeu>.
GAME_DIR=$(ls -d "$STAKE"/*/"$GAME" 2>/dev/null | head -1 || true)
GAME_DIR=${GAME_DIR:-$STAKE/$GAME}

PUBLISH="$GAME_DIR/math/games/$GAME/library/publish_files"
BUILD="$GAME_DIR/frontend/apps/$GAME/build"
CACHE=$(ls -dt "$HOME/Library/Application Support/slotlab/cache/$GAME-"* 2>/dev/null | head -1 || true)

step() { printf '\n\033[1m▸ %s\033[0m\n' "$1"; }
die()  { printf '\033[31m✗ %s\033[0m\n' "$1" >&2; exit 1; }

[[ -f "$PUBLISH/index.json" ]] || die "publish_files introuvable : $PUBLISH"
[[ -f "$BUILD/index.html"   ]] || die "build frontend introuvable : $BUILD — lance d'abord: pnpm --filter $GAME build"
[[ -n "$CACHE" ]]              || die "aucun cache de books pour $GAME — ouvre SlotLab et importe-les"

# Un cache de books plus vieux que sa source = la math a bougé sans ré-import.
# Envoyer ça publierait des books qui ne correspondent plus aux LUT.
for z in "$PUBLISH"/books_*.jsonl.zst; do
  [[ -e "$z" ]] || continue
  mode=$(basename "$z" .jsonl.zst); mode=${mode#books_}
  blocks="$CACHE/books_$mode.blocks"
  [[ -f "$blocks" ]] || die "books_$mode absent du cache — ré-importe $GAME dans SlotLab"
  [[ "$z" -nt "$blocks" ]] && die "books_$mode.jsonl.zst est plus récent que le cache — ré-importe $GAME dans SlotLab avant de déployer"
done

step "Math (index.json + LUT) → $HOST"
rsync -a $DRY --itemize-changes --exclude '*.zst' "$PUBLISH/" "$HOST:$REMOTE/$GAME/publish/"

step "Build frontend → $HOST"
rsync -a $DRY --itemize-changes --delete "$BUILD/" "$HOST:$REMOTE/$GAME/build/"

step "Books (incrémental, ne renvoie que les modes modifiés)"
rsync -a $DRY --itemize-changes "$CACHE/" "$HOST:$REMOTE/$GAME/cache/"

if [[ $REBUILD_IMAGE -eq 1 ]]; then
  step "Sources SlotLab → $HOST puis reconstruction de l'image (~4 min)"
  rsync -a $DRY --itemize-changes --exclude target --exclude node_modules \
    --exclude .git --exclude dist --exclude Dockerfile "$REPO/" "$HOST:$SRC_REMOTE/"
  [[ -n "$DRY" ]] || ssh "$HOST" "pct exec 102 -- docker build -t slotlab-share:latest /mnt/storage/slotlab/src" 2>&1 | tail -3
  # Reconstruire l'image ne suffit pas : le conteneur continue de tourner sur
  # l'ancienne (docker compose up -d sans changement de tag est un no-op). Sans
  # ce recreate, un correctif de code est buildé puis jamais servi.
  [[ -n "$DRY" ]] || ssh "$HOST" \
    "pct exec 102 -- sh -c 'cd $STACK_DIR && docker compose up -d --force-recreate'" 2>&1 | tail -3
fi

[[ -n "$DRY" ]] && { echo; echo "(dry-run : rien n'a été envoyé)"; exit 0; }

if [[ $REBUILD_IMAGE -eq 1 ]]; then
  step "Attente du redémarrage du conteneur"
  for i in $(seq 1 30); do
    code=$(curl -s -m 5 -o /dev/null -w '%{http_code}' "$HUB/healthz" || true)
    [[ "$code" == "200" ]] && break
    sleep 2
  done
  [[ "$code" == "200" ]] || die "le conteneur ne répond pas sur $HUB/healthz (code $code)"
  echo "  conteneur recréé sur la nouvelle image — le hub a tout relu au démarrage"
else

step "Rechargement à chaud du hub"
# Le token de l'API de contrôle est celui qu'utilise l'onglet Deploy.
TOKEN=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['adminToken'])" "$DEPLOY_JSON" 2>/dev/null || true)
if [[ -z "$TOKEN" ]]; then
  die "token du hub introuvable dans $DEPLOY_JSON — renseigne les réglages dans l'onglet Deploy de SlotLab"
fi
# Un rechargement, pas un redémarrage : le registre relit les LUT et remappe
# les .blocks sans tuer les sessions en cours.
curl -s -m 30 -X POST -H "x-slotlab-token: $TOKEN" "$HUB/api/hub/reload" | tail -c 200; echo
fi

step "Vérification"
for i in $(seq 1 20); do
  code=$(curl -s -m 8 -o /dev/null -w '%{http_code}' "$PUBLIC/healthz" || true)
  [[ "$code" == "200" ]] && break
  sleep 2
done
[[ "$code" == "200" ]] || die "le serveur ne répond pas sur $PUBLIC/healthz (code $code)"
loc=$(curl -s -m 10 -D - -o /dev/null "$PUBLIC/play" | tr -d '\r' | grep -i '^location' || true)
[[ -n "$loc" ]] || die "$PUBLIC/play ne redirige pas"

printf '\n\033[32m✓ %s est à jour\033[0m\n  %s/play\n' "$GAME" "$PUBLIC"
if [[ "$loc" == *"/gate"* ]]; then
  echo "  protégé par un code d'accès — consultable et modifiable sur https://stake.mcberry.fr"
else
  echo "  ⚠ lien OUVERT : aucun code d'accès actif"
fi
