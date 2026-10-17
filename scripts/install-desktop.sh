#!/usr/bin/env bash
# Ajoute Pi Voice au menu des applications (GNOME…) : un clic lance l'app,
# qui démarre elle-même les serveurs vocaux et rouvre le dernier projet.
# Désinstaller : rm ~/.local/share/applications/dev.egza.pivoice.desktop
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
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
