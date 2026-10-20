#!/usr/bin/env bash
# Installe le pi propre à Pi Voice dans runtime/pi (comme harn) : l'app n'utilise jamais le pi
# global. Version fixée ici ; pour monter de version, change PI_VERSION et relance le script.
# Lancé par l'assistant de démarrage de Pi Voice, dans son terminal intégré.
set -euo pipefail
export PATH="$HOME/.local/node/bin:$HOME/.npm-global/bin:$HOME/.local/bin:$PATH"

PI_VERSION="1.3.0"
PACKAGE="@earendil-works/pi-coding-agent"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="$ROOT/runtime/pi"

b() { printf '\033[1m%s\033[0m\n' "$*"; }

if ! command -v npm >/dev/null 2>&1; then
  b "pi a besoin de Node.js (version 22 ou plus) et de npm, absents de ce système."
  echo "Installe-les (par exemple : sudo apt install nodejs npm), puis relance."
  exit 1
fi

b "Installation de pi $PI_VERSION dans $PREFIX"
mkdir -p "$PREFIX"
npm install --prefix "$PREFIX" --ignore-scripts --no-fund --no-audit --save-exact "$PACKAGE@$PI_VERSION"

echo
if [ -x "$PREFIX/node_modules/.bin/pi" ]; then
  b "pi est installé ($("$PREFIX/node_modules/.bin/pi" --version 2>/dev/null)). Tu peux fermer ce terminal : Pi Voice passe à l'étape suivante."
else
  b "L'installation n'a pas abouti. Relis les messages ci-dessus."
  exit 1
fi
