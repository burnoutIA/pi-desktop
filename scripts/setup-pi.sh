#!/usr/bin/env bash
# Installe le pi propre à Pi Voice et ses outils dans runtime/ (comme harn) :
#   - pi            runtime/pi          (npm)
#   - ketch         runtime/ketch       (recherche web, docs, code, lecture de pages ; binaire GitHub)
#   - Playwright    runtime/playwright  (pilotage d'un navigateur ; npm + Chromium)
# L'app n'utilise jamais le pi global. Versions fixées ci-dessous : pour en changer, modifie-les
# et relance le script (ce qui est déjà à jour est sauté).
# Lancé par l'assistant de démarrage de Pi Voice, dans son terminal intégré.
set -euo pipefail
export PATH="$HOME/.local/node/bin:$HOME/.npm-global/bin:$HOME/.local/bin:$PATH"

PI_VERSION="1.3.0"
KETCH_VERSION="0.20.0"
KETCH_SHA256="791720b097f86d7b369f3353e3ea8337ece3f59f8e1275d459b5694ab27b6565" # linux_x86_64
PLAYWRIGHT_MCP_VERSION="0.0.83"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RT="$ROOT/runtime"

b() { printf '\033[1m%s\033[0m\n' "$*"; }
pkg_version() { node -p "require('$1/package.json').version" 2>/dev/null || true; }
npm_install() { mkdir -p "$1" && npm install --prefix "$1" --ignore-scripts --no-fund --no-audit --save-exact "$2"; }

if ! command -v npm >/dev/null 2>&1; then
  b "pi a besoin de Node.js (version 22 ou plus) et de npm, absents de ce système."
  echo "Installe-les (par exemple : sudo apt install nodejs npm), puis relance."
  exit 1
fi

# ---- pi ----
if [ "$(pkg_version "$RT/pi/node_modules/@earendil-works/pi-coding-agent")" = "$PI_VERSION" ]; then
  b "pi $PI_VERSION : déjà installé"
else
  b "Installation de pi $PI_VERSION"
  npm_install "$RT/pi" "@earendil-works/pi-coding-agent@$PI_VERSION"
fi

# ---- ketch ----
if "$RT/ketch/ketch" version 2>/dev/null | grep -q "v$KETCH_VERSION"; then
  b "ketch $KETCH_VERSION : déjà installé"
elif [ "$(uname -sm)" != "Linux x86_64" ]; then
  b "ketch : pas de build prévu pour $(uname -sm), recherche web indisponible."
else
  b "Installation de ketch $KETCH_VERSION (recherche web)"
  archive="$(mktemp --suffix=.tar.gz)"
  url="https://github.com/1broseidon/ketch/releases/download/v$KETCH_VERSION/ketch_${KETCH_VERSION}_linux_x86_64.tar.gz"
  node -e 'fetch(process.argv[1]).then(async r => { if (!r.ok) throw new Error(r.status + " " + r.statusText); require("fs").writeFileSync(process.argv[2], Buffer.from(await r.arrayBuffer())); })' "$url" "$archive"
  echo "$KETCH_SHA256  $archive" | sha256sum -c --quiet
  rm -rf "$RT/ketch" && mkdir -p "$RT/ketch"
  tar -xzf "$archive" -C "$RT/ketch" ketch
  rm -f "$archive"
fi

# ---- Playwright ----
if [ "$(pkg_version "$RT/playwright/node_modules/@playwright/mcp")" = "$PLAYWRIGHT_MCP_VERSION" ]; then
  b "Playwright MCP $PLAYWRIGHT_MCP_VERSION : déjà installé"
else
  b "Installation de Playwright MCP $PLAYWRIGHT_MCP_VERSION (navigateur)"
  npm_install "$RT/playwright" "@playwright/mcp@$PLAYWRIGHT_MCP_VERSION"
fi
b "Navigateur Chromium de Playwright"
node "$RT/playwright/node_modules/playwright/cli.js" install chromium

echo
if [ -x "$RT/pi/node_modules/.bin/pi" ]; then
  b "pi est installé ($("$RT/pi/node_modules/.bin/pi" --version 2>/dev/null)). Tu peux fermer ce terminal : Pi Voice passe à l'étape suivante."
else
  b "L'installation n'a pas abouti. Relis les messages ci-dessus."
  exit 1
fi
