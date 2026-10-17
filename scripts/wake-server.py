# /// script
# requires-python = ">=3.11"
# dependencies = ["vosk>=0.3.45", "websockets>=13"]
# ///
"""Détecteur du nom de l'agent (mot de réveil), sur processeur, avec Vosk.

    uv run scripts/wake-server.py [--port 8091]

Client : ws://127.0.0.1:8091/?name=jarvis
  -> envoie du PCM int16 little-endian, mono, 16 kHz (trames binaires de n'importe quelle taille)
  <- reçoit du JSON : {"type": "ready", "name": "jarvis", "known": true}
                       {"type": "wake", "name": "jarvis"}

La grammaire contient le nom ET les ~3000 mots français les plus fréquents : une phrase
ordinaire trouve ses vrais mots au lieu d'être forcée vers le nom (sinon « je vais » donne
« jarvis »). Testé : 9/9 détections, 0 fausse alerte sur des phrases pièges.
"""
import argparse
import asyncio
import json
import os
import sys
import urllib.parse
import urllib.request
import zipfile

import websockets
from vosk import KaldiRecognizer, Model, SetLogLevel

SetLogLevel(-1)
CACHE = os.path.expanduser("~/.cache/pi-voice/vosk")
MODEL_NAME = "vosk-model-small-fr-0.22"
MODEL_URL = f"https://alphacephei.com/vosk/models/{MODEL_NAME}.zip"
# Fréquences de mots français (OpenSubtitles, CC-BY-SA 4.0), servent de « concurrents » au nom.
FREQ_URL = "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/fr/fr_50k.txt"
COMPETITORS = 3000
RATE = 16000


def ensure_files() -> tuple[str, str]:
    os.makedirs(CACHE, exist_ok=True)
    model_dir = os.path.join(CACHE, MODEL_NAME)
    if not os.path.isdir(model_dir):
        print(f"[wake] téléchargement du modèle Vosk ({MODEL_NAME}, ~40 Mo)…", flush=True)
        zpath = model_dir + ".zip"
        urllib.request.urlretrieve(MODEL_URL, zpath)
        with zipfile.ZipFile(zpath) as z:
            z.extractall(CACHE)
        os.remove(zpath)
    freq = os.path.join(CACHE, "fr_50k.txt")
    if not os.path.exists(freq):
        urllib.request.urlretrieve(FREQ_URL, freq)
    return model_dir, freq


class Grammars:
    def __init__(self, model: Model, freq_path: str):
        self.model = model
        self.words: list[str] = []
        with open(freq_path, encoding="utf-8") as f:
            for line in f:
                w = line.split()[0]
                if model.vosk_model_find_word(w) >= 0:
                    self.words.append(w)
                if len(self.words) >= COMPETITORS + 50:
                    break
        self.cache: dict[str, str] = {}

    def for_name(self, name: str) -> str:
        if name not in self.cache:
            others = [w for w in self.words if w != name][:COMPETITORS]
            self.cache[name] = json.dumps([name, *others, "[unk]"], ensure_ascii=False)
        return self.cache[name]


async def handle(ws, model: Model, grammars: Grammars):
    query = urllib.parse.parse_qs(urllib.parse.urlparse(ws.request.path).query)
    name = (query.get("name", ["pi"])[0] or "pi").strip().lower()
    known = model.vosk_model_find_word(name) >= 0
    await ws.send(json.dumps({"type": "ready", "name": name, "known": known}))
    if not known:
        print(f"[wake] « {name} » absent du dictionnaire Vosk : détection impossible", flush=True)
    rec = KaldiRecognizer(model, RATE, grammars.for_name(name))
    print(f"[wake] client connecté, nom « {name} »", flush=True)
    fired = False  # un seul réveil par énoncé
    async for msg in ws:
        if not isinstance(msg, bytes):
            continue
        if rec.AcceptWaveform(msg):
            words = json.loads(rec.Result()).get("text", "").split()
            if name in words and not fired:
                await ws.send(json.dumps({"type": "wake", "name": name}))
            fired = False
        else:
            partial = json.loads(rec.PartialResult()).get("partial", "").split()
            if name in partial and not fired:
                fired = True
                await ws.send(json.dumps({"type": "wake", "name": name}))
    print("[wake] client déconnecté", flush=True)


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8091)
    a = p.parse_args()
    model_dir, freq = ensure_files()
    model = Model(model_dir)
    grammars = Grammars(model, freq)
    async with websockets.serve(lambda ws: handle(ws, model, grammars), a.host, a.port, max_size=2**20):
        print(f"[wake] listening on ws://{a.host}:{a.port}", flush=True)
        await asyncio.Future()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        sys.exit(0)
