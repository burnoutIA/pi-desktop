#!/usr/bin/env bash
# Lance Pi Voice. Nettoie l'environnement hérité d'un éditeur installé en snap
# (VSCodium, VS Code…) : ses GTK_PATH/GIO_MODULE_DIR font planter WebKitGTK.
# Usage : scripts/run.sh [dossier-du-projet]      (ou : scripts/run.sh dev)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

for v in GTK_PATH GTK_EXE_PREFIX GTK_IM_MODULE_FILE GIO_MODULE_DIR GDK_PIXBUF_MODULE_FILE \
  GDK_PIXBUF_MODULEDIR LOCPATH GSETTINGS_SCHEMA_DIR; do unset "$v"; done
[ -n "${XDG_DATA_DIRS_VSCODE_SNAP_ORIG:-}" ] && export XDG_DATA_DIRS="$XDG_DATA_DIRS_VSCODE_SNAP_ORIG"
[ -n "${XDG_CONFIG_DIRS_VSCODE_SNAP_ORIG:-}" ] && export XDG_CONFIG_DIRS="$XDG_CONFIG_DIRS_VSCODE_SNAP_ORIG"
# Toujours le même dossier de données, qu'on lance depuis le menu ou depuis le terminal d'un
# éditeur snap (qui redirige XDG_DATA_HOME vers ~/snap/…) : sinon les réglages se dédoublent.
export XDG_DATA_HOME="$HOME/.local/share"
export PATH="$HOME/.cargo/bin:$PATH"

# Migration : réglages enregistrés autrefois dans le dossier snap de l'éditeur.
STORE="$XDG_DATA_HOME/dev.egza.pivoice/localstorage"
if ! pgrep -f "target/release/pi-desktop" >/dev/null && ! grep -qa agentConfig "$STORE"/*.localstorage 2>/dev/null; then
  for old in "$HOME"/snap/*/current/.local/share/dev.egza.pivoice/localstorage; do
    if grep -qa agentConfig "$old"/*.localstorage 2>/dev/null; then
      mkdir -p "$STORE"
      rm -f "$STORE"/*.localstorage-shm "$STORE"/*.localstorage-wal
      cp "$old"/*.localstorage "$STORE"/
      echo "Réglages récupérés depuis $old"
      break
    fi
  done
fi

if [ "${1:-}" = "dev" ]; then
  cd "$ROOT" && exec npm run tauri dev
fi
BIN="$ROOT/src-tauri/target/release/pi-desktop"
[ -x "$BIN" ] || { echo "Binaire absent : lance d'abord « npx tauri build --no-bundle »"; exit 1; }
exec "$BIN" "$@"
