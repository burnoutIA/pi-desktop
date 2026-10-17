# Pi Voice

Surcouche vocale pour [pi](https://github.com/earendil-works/pi), un agent de code. Tu lui parles, tu vois ton texte s'écrire en direct, pi travaille juste à côté, puis il te répond à voix haute. Un égaliseur s'anime pendant qu'il parle, et `Espace` lui coupe la parole à tout moment.

Tout est open source et tourne en local. Seul le modèle de langage reste celui que pi utilise déjà.

| Brique | Outil | Licence |
|---|---|---|
| App de bureau | Tauri 2 (Rust) + Svelte 5 (TypeScript) | MIT / Apache-2.0 |
| Agent | `pi --mode rpc` | MIT |
| Transcription (STT) | Kyutai `stt-1b-en_fr` (streaming, détection de fin de phrase) | CC-BY 4.0 |
| Synthèse (TTS) | Kyutai `tts-1.6b-en_fr` (streaming) | CC-BY 4.0 |
| Serveur vocal | `moshi-server` (Rust + CUDA) | MIT / Apache-2.0 |
| Réveil par le nom | Vosk `vosk-model-small-fr-0.22` (processeur) | Apache-2.0 |

## Prérequis (une seule fois)

```bash
# Rust (déjà installé dans ~/.cargo si tu as suivi la mise en place)
wget -qO- https://sh.rustup.rs | sh

# Dépendances système de Tauri + audio
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  libssl-dev librsvg2-dev libayatana-appindicator3-dev libasound2-dev

npm install
```

Pour le serveur vocal, il faut un GPU NVIDIA (CUDA 12), `uv` et `gcc-13`. La RTX 3090 convient largement.

## Lancer

```bash
scripts/install-desktop.sh   # une fois : ajoute « Pi Voice » au menu des applications
```

Ensuite, lance **Pi Voice depuis le menu** (ou épingle-la au dock). L'app :
- démarre elle-même les serveurs vocaux s'ils ne tournent pas (prêts en ~10 s, plusieurs minutes la toute première fois, le temps de compiler et de télécharger les modèles) ;
- les arrête quand tu la fermes, ce qui libère ~11 Go de mémoire GPU. Des serveurs que tu as lancés toi-même ne sont jamais arrêtés ;
- rouvre le dernier projet. `Ctrl+O` en ouvre un autre.

En ligne de commande : `scripts/run.sh [dossier]`, ou `scripts/run.sh dev` en mode développement. `scripts/run.sh` nettoie les variables qu'un éditeur installé en snap (VSCodium, VS Code) transmet à son terminal : sans ça, WebKitGTK plante au démarrage. Les serveurs seuls : `scripts/voice-server.sh`. Le journal du démarrage de la voix est dans `~/.cache/pi-voice/voice-server.log`.

Pour vérifier la voix seule : `uv run scripts/test-voice.py`. Il fait dire une phrase au TTS, puis la redonne au STT.

## Utilisation

- **Micro** (ou `Espace`) : parle, et le texte s'écrit dans la zone du bas. Quand tu fais une pause, une barre se remplit pendant 0,9 s puis le texte est envoyé. Pendant ce délai, clique dans le texte ou tape pour le corriger avant l'envoi.
- Un second clic sur le micro envoie tout de suite.
- **Le micro n'écoute que sur demande** (`Espace` ou clic), jamais pendant que pi parle. Appuyer sur `Espace` pendant qu'il parle coupe sa voix et ouvre le micro.
- **Mode mains libres** (réglages, onglet Voix) : dis **« Jarvis, ta demande »**, avec le nom choisi dans l'onglet Agent.
  - Rien ne part vers pi sans le nom : ni sa propre voix, ni la télé, ni les gens autour.
  - Si tu marques une pause après le nom, il répond par une phrase courte préenregistrée (« Oui ? », « Je t'écoute »…) et attend ta demande.
  - Dire son nom pendant qu'il parle le coupe, et la suite de ta phrase devient la nouvelle demande.
  - Les mots nettement plus faibles que ta voix (voix lointaines, bruit de fond) sont ignorés. L'écart toléré se règle dans l'onglet Agent.
  - Le nom est détecté par Vosk (petit modèle français, sur le processeur). Kyutai ne s'allume qu'après le nom.
- **Pendant que pi travaille** : ce que tu dis est envoyé comme consigne (`steer`). Il la prend en compte au prochain tour, sans perdre son travail.
- Dire **« stop »** (ou « arrête ») ou appuyer sur `Ctrl+.` arrête pi net.
- `Échap` coupe la voix, annule l'envoi automatique ou ferme le micro.
- `Ctrl+O` ouvre un autre projet.
- **À gauche**, l'arbre des fichiers avec leur état git. Un point lumineux marque les fichiers que pi a touchés pendant la session.
- **À droite**, le diff du fichier par rapport au dernier commit. Il suit automatiquement le dernier fichier modifié par pi, sauf si tu en as ouvert un toi-même.

### Ce qui est lu à voix haute

L'app ajoute une consigne au prompt de pi : terminer sa réponse finale par un court résumé oral entre les balises `<oral>…</oral>`. La synthèse vocale démarre dès que ce bloc commence à arriver, sans attendre la fin du texte. S'il n'y a pas de bloc `<oral>`, l'app lit le début de la réponse, sans le code.

## Réglages (icône en haut à droite)

- Voix française : « Développeuse », « Fabien », ou des voix du dataset CML-TTS.
- Modèle (ceux que ta config de pi connaît), voix, lecture vocale, mode mains libres.
- Adresses des serveurs STT et TTS.

## Architecture

```
src/                     Interface Svelte
  App.svelte             Orchestration : micro, envoi, raccourcis
  lib/session.svelte.ts  Reconstruit la conversation à partir des événements de pi
  components/            Égaliseur, micro, conversation, arbre, diff, réglages
src-tauri/src/
  pi.rs                  Processus `pi --mode rpc`, JSONL découpé sur LF uniquement
  voice.rs               Clients STT/TTS Kyutai, fin de tour, interruption, anti-écho
  speech.rs              Extraction du bloc <oral> pendant le streaming
  audio.rs               Micro et haut-parleur (cpal), rééchantillonnage, FFT de l'égaliseur
  files.rs               Arbre (.gitignore respecté), git status, diff, surveillance du dossier
scripts/voice-server.sh  Lance STT (8090), TTS (8089) et le détecteur de réveil (8091)
scripts/wake-server.py   Détecteur du nom de l'agent (Vosk)
```

Tout l'audio passe par Rust. L'égaliseur reçoit environ 45 fois par seconde 40 bandes de fréquence calculées par FFT. Elles sont dessinées en miroir (graves au centre), et deviennent une ligne de points dans le silence.

## Limites connues

- **Écho.** En push-to-talk, le micro n'écoute pas pendant que pi parle. Juste après une coupure, un filtre ignore les mots que pi venait de dire.
- **Micro.** Le flux du micro reste ouvert pour démarrer sans délai, mais ses mots ne comptent qu'après `Espace`. L'audio ne va qu'au serveur local.
- **Voix limitées.** Le TTS Kyutai n'utilise que les voix fournies dans `kyutai/tts-voices`. Pas de clonage de voix.
