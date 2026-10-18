#!/usr/bin/env bash
# Installe (la première fois) puis lance les serveurs vocaux Kyutai en local :
#   STT français  -> ws://127.0.0.1:8090/api/asr-streaming
#   TTS français  -> ws://127.0.0.1:8089/api/tts_streaming
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
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$HOME/.local/node/bin:/opt/homebrew/bin:$PATH"

# Un service déjà en route (lancé à la main ou par une autre instance) n'est pas relancé.
port_busy() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
mkdir -p "$DIR" "$HOME/.cache/pi-voice/logs"
cd "$DIR"

for f in pyproject.toml uv.lock; do
  [ -f "$f" ] || curl -fsSLO "https://raw.githubusercontent.com/kyutai-labs/moshi/$MOSHI_COMMIT/rust/moshi-server/$f"
done
[ -d .venv ] || uv venv -q
if [ "$(uname)" = Darwin ]; then
  # xformers n'a pas de version macOS (et moshi s'en passe) : on le retire, et le verrou figé pour CUDA ne s'applique plus.
  sed -i '' '/"xformers"/d' pyproject.toml
  uv sync -q
else
  uv sync -q --locked
fi
# shellcheck disable=SC1091
source .venv/bin/activate
PYLIB="$(python -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')"
if [ "$(uname)" = Darwin ]; then
  export DYLD_LIBRARY_PATH="$PYLIB${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
  # Le Python embarqué dans moshi-server ne trouve ni sa bibliothèque standard ni le venv sans ça.
  PYTHONHOME="$(python -c 'import sys; print(sys.base_prefix)')"
  PYTHONPATH="$(python -c 'import site; print(site.getsitepackages()[0])')"
  export PYTHONHOME PYTHONPATH
else
  export LD_LIBRARY_PATH="$PYLIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

if [ ! -x "$DIR/bin/moshi-server" ] && [ "$(uname)" = Darwin ]; then
  echo "▶ Compilation de moshi-server $MOSHI_SERVER_VERSION avec Metal (plusieurs minutes la première fois)…"
  export CXXFLAGS="${CXXFLAGS:-} -include cstdint"
  export CMAKE_POLICY_VERSION_MINIMUM=3.5
  cargo install --locked --features metal --root "$DIR" "moshi-server@$MOSHI_SERVER_VERSION"
elif [ ! -x "$DIR/bin/moshi-server" ]; then
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
  "$DIR/bin/moshi-server" worker --config "$3" --addr 127.0.0.1 --port "$2" 2>&1 | sed -u "s/^/[$1] /" &
}

# Les scripts uv ont leur propre Python : rien du venv de moshi-server ne doit fuir vers eux.
uv_script() { (cd "$ROOT" && unset PYTHONHOME PYTHONPATH DYLD_LIBRARY_PATH VIRTUAL_ENV && uv run -q "$@"); }

# Sur Mac, le TTS de moshi-server (PyTorch) est 5× trop lent : on prend l'implémentation MLX de
# Kyutai. Quantifiée en 4 bits par défaut pour tenir le temps réel sur une puce de base ;
# PIVOICE_TTS_BITS=8 pour plus de qualité si la machine suit.
run_tts() {
  if [ "$(uname)" != Darwin ]; then run tts 8089 "$CONFIGS/tts.toml"; return; fi
  if port_busy 8089; then echo "▶ tts déjà actif sur le port 8089"; return; fi
  echo "▶ tts (MLX) sur le port 8089 (le premier lancement télécharge les modèles depuis Hugging Face)"
  uv_script scripts/tts-mlx-server.py --port 8089 --quantize "${PIVOICE_TTS_BITS:-4}" 2>&1 | sed -u "s/^/[tts] /" &
}

run_wake() {
  if port_busy 8091; then echo "▶ réveil déjà actif sur le port 8091"; return; fi
  echo "▶ réveil sur le port 8091 (le premier lancement télécharge le modèle Vosk, ~40 Mo)"
  uv_script scripts/wake-server.py --port 8091 2>&1 | sed -u "s/^/[wake] /" &
}

case "$WHAT" in
  stt) run stt 8090 "$CONFIGS/stt.toml" ;;
  tts) run_tts ;;
  wake) run_wake ;;
  all) run stt 8090 "$CONFIGS/stt.toml"; run_tts; run_wake ;;
  *) echo "usage : $0 [stt|tts|wake|all]"; exit 1 ;;
esac
trap 'kill 0' INT TERM
# Lancé par l'app : si elle disparaît (fermeture, plantage), on arrête tout pour libérer le GPU.
if [ -n "${PIVOICE_PARENT:-}" ]; then
  ( while kill -0 "$PIVOICE_PARENT" 2>/dev/null; do sleep 2; done; kill -TERM 0 ) &
fi
wait
