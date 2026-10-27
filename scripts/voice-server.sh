#!/usr/bin/env bash
# Lance le serveur de transcription en local, sur le processeur (aucune mémoire GPU) :
#   STT français  -> ws://127.0.0.1:8090/api/asr-streaming  (Parakeet TDT 0.6B v3)
# Le premier lancement installe ses dépendances (uv) et télécharge le modèle.
# Usage : scripts/voice-server.sh [stt]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WHAT="${1:-stt}"

# Lancé depuis le menu des applications, le PATH ne contient pas toujours nos outils.
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$HOME/.local/node/bin:$PATH"
mkdir -p "$HOME/.cache/pi-voice/logs"

# Un service déjà en route (lancé à la main ou par une autre instance) n'est pas relancé.
port_busy() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }

run() { # nom port script
  if port_busy "$2"; then echo "▶ $1 déjà actif sur le port $2"; return; fi
  echo "▶ $1 sur le port $2"
  (cd "$ROOT" && uv run -q "scripts/$3" --port "$2" 2>&1 | sed -u "s/^/[$1] /") &
}

case "$WHAT" in
  stt|all) run stt 8090 parakeet-stt-server.py ;;
  *) echo "usage : $0 [stt]"; exit 1 ;;
esac
trap 'kill 0' INT TERM
# Lancé par l'app : si elle disparaît (fermeture, plantage), on arrête tout.
if [ -n "${PIVOICE_PARENT:-}" ]; then
  ( while kill -0 "$PIVOICE_PARENT" 2>/dev/null; do sleep 2; done; kill -TERM 0 ) &
fi
wait
