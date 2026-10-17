#!/usr/bin/env bash
# Installe pi avec son installeur officiel (https://pi.dev/install.sh).
# Lancé par l'assistant de démarrage de Pi Voice, dans son terminal intégré.
set -euo pipefail
export PATH="$HOME/.pi/agent/bin:$HOME/.local/bin:$HOME/bin:$HOME/.local/node/bin:$PATH"

b() { printf '\033[1m%s\033[0m\n' "$*"; }

if command -v pi >/dev/null 2>&1; then
  b "pi est déjà installé : $(command -v pi) ($(pi --version 2>/dev/null))"
  exit 0
fi

b "Installation de pi avec l'installeur officiel"
echo "Source : https://pi.dev/install.sh"
echo

if ! command -v curl >/dev/null 2>&1; then
  b "L'installeur a besoin de curl, absent de ce système."
  echo "Installation de curl (ton mot de passe administrateur va être demandé) :"
  sudo apt-get install -y curl
  echo
fi

curl -fsSL https://pi.dev/install.sh | sh

echo
if command -v pi >/dev/null 2>&1 || [ -x "$HOME/.pi/agent/bin/pi" ] || [ -x "$HOME/.local/bin/pi" ]; then
  b "pi est installé. Tu peux fermer ce terminal : Pi Voice passe à l'étape suivante."
else
  b "L'installation n'a pas abouti. Relis les messages ci-dessus."
  exit 1
fi
