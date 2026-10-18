#!/usr/bin/env bash
# Installe Pi Voice comme une app : menu des applications (GNOME…) sous Linux, Pi Voice.app dans
# ~/Applications sur Mac. Un clic lance l'app, qui démarre elle-même les serveurs vocaux et rouvre
# le dernier projet. Désinstaller (Linux) : rm ~/.local/share/applications/dev.egza.pivoice.desktop
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [ "$(uname)" = Darwin ]; then
  # Sur Mac : une vraie app (Pi Voice.app), pour que macOS lui demande l'accès au micro.
  # Désinstaller : rm -rf ~/Applications/"Pi Voice.app"
  export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
  (cd "$ROOT" && npx tauri build --bundles app)
  mkdir -p "$HOME/Applications"
  rm -rf "$HOME/Applications/Pi Voice.app"
  cp -R "$ROOT/src-tauri/target/release/bundle/macos/Pi Voice.app" "$HOME/Applications/"
  echo "Pi Voice installée dans ~/Applications (Launchpad, Spotlight). Au premier lancement, autorise le micro."
  exit 0
fi

DEST="$HOME/.local/share/applications/dev.egza.pivoice.desktop"
mkdir -p "$(dirname "$DEST")"
cat > "$DEST" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Pi Voice
GenericName=Agent de code vocal
Comment=Parle à pi, ton agent de code
Exec=$ROOT/scripts/run.sh
Icon=$ROOT/src-tauri/icons/icon.png
Terminal=false
Categories=Development;
Keywords=pi;voix;agent;code;jarvis;
StartupNotify=true
StartupWMClass=pi-desktop
DESKTOP
chmod +x "$DEST"
command -v update-desktop-database >/dev/null && update-desktop-database "$(dirname "$DEST")" 2>/dev/null || true
echo "Pi Voice ajouté au menu des applications ($DEST)"
