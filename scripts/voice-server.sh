#!/usr/bin/env bash
# Installe (la première fois) puis lance les serveurs vocaux Kyutai en local :
#   STT français  -> ws://127.0.0.1:8090/api/asr-streaming
#   TTS français  -> ws://127.0.0.1:8089/api/tts_streaming  (Pocket TTS sur le processeur ;
#                    PIVOICE_TTS=kyutai pour le TTS Kyutai 1,6B sur le GPU, ~8 Go de VRAM)
#   Réveil (nom)  -> ws://127.0.0.1:8091  (Vosk, processeur, pour le mode mains libres)
# Usage : scripts/voice-server.sh [stt|tts|wake|all]   (all par défaut)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/voice-server"
CONFIGS="$ROOT/scripts/kyutai"
MOSHI_SERVER_VERSION=0.6.4
# Le TTS embarque un composant Python : on reprend l'environnement figé par Kyutai (projet unmute).
MOSHI_COMMIT=9837ca328d58deef5d7a4fe95a0fb49c902ec0ae
WHAT="${1:-all}"

# Lancé depuis le menu des applications, le PATH ne contient pas toujours nos outils.
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$HOME/.local/node/bin:$PATH"

# Un service déjà en route (lancé à la main ou par une autre instance) n'est pas relancé.
port_busy() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
mkdir -p "$DIR" "$HOME/.cache/pi-voice/logs"
cd "$DIR"

for f in pyproject.toml uv.lock; do
  [ -f "$f" ] || wget -q "https://raw.githubusercontent.com/kyutai-labs/moshi/$MOSHI_COMMIT/rust/moshi-server/$f"
done
[ -d .venv ] || uv venv -q
uv sync -q --locked
# shellcheck disable=SC1091
source .venv/bin/activate
LD_LIBRARY_PATH="$(python -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LD_LIBRARY_PATH

if [ ! -x "$DIR/bin/moshi-server" ]; then
  echo "▶ Compilation de moshi-server $MOSHI_SERVER_VERSION avec CUDA (10 à 20 min la première fois)…"
  # nvcc 12.x n'accepte pas GCC 15 : on lui donne GCC 13. Fix sentencepiece pour GCC récents.
  export NVCC_CCBIN="${NVCC_CCBIN:-$(command -v gcc-13 || command -v gcc)}"
  export CXXFLAGS="${CXXFLAGS:-} -include cstdint"
  # CMake 4 refuse les vieux CMakeLists (libopus embarqué) sans cette option.
  export CMAKE_POLICY_VERSION_MINIMUM=3.5
  export CUDA_COMPUTE_CAP="${CUDA_COMPUTE_CAP:-$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader | head -1 | tr -d .)}"
  cargo install --locked --features cuda --root "$DIR" "moshi-server@$MOSHI_SERVER_VERSION"
fi

run() { # nom port config
  if port_busy "$2"; then echo "▶ $1 déjà actif sur le port $2"; return; fi
  echo "▶ $1 sur le port $2 (le premier lancement télécharge les modèles depuis Hugging Face)"
  "$DIR/bin/moshi-server" worker --config "$CONFIGS/$3" --addr 127.0.0.1 --port "$2" 2>&1 | sed -u "s/^/[$1] /" &
}

run_pocket_tts() {
  if port_busy 8089; then echo "▶ tts déjà actif sur le port 8089"; return; fi
  echo "▶ tts Pocket (processeur) sur le port 8089 (le premier lancement télécharge le modèle, ~400 Mo)"
  (cd "$ROOT" && uv run -q scripts/pocket-tts-server.py --port 8089 2>&1 | sed -u "s/^/[tts] /") &
}

run_tts() {
  if [ "${PIVOICE_TTS:-pocket}" = kyutai ]; then run tts 8089 tts.toml; else run_pocket_tts; fi
}

run_wake() {
  if port_busy 8091; then echo "▶ réveil déjà actif sur le port 8091"; return; fi
  echo "▶ réveil sur le port 8091 (le premier lancement télécharge le modèle Vosk, ~40 Mo)"
  (cd "$ROOT" && uv run -q scripts/wake-server.py --port 8091 2>&1 | sed -u "s/^/[wake] /") &
}

case "$WHAT" in
  stt) run stt 8090 stt.toml ;;
  tts) run_tts ;;
  wake) run_wake ;;
  all) run stt 8090 stt.toml; run_tts; run_wake ;;
  *) echo "usage : $0 [stt|tts|wake|all]"; exit 1 ;;
esac
trap 'kill 0' INT TERM
# Lancé par l'app : si elle disparaît (fermeture, plantage), on arrête tout pour libérer le GPU.
if [ -n "${PIVOICE_PARENT:-}" ]; then
  ( while kill -0 "$PIVOICE_PARENT" 2>/dev/null; do sleep 2; done; kill -TERM 0 ) &
fi
wait
