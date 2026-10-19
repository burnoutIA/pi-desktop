# /// script
# requires-python = ">=3.10,<3.15"
# dependencies = ["pocket-tts", "websockets>=13", "msgpack"]
# [tool.uv]
# extra-index-url = ["https://download.pytorch.org/whl/cpu"]
# index-strategy = "unsafe-best-match"
# ///
"""Synthèse vocale Pocket TTS (Kyutai, ~100 M paramètres) sur le processeur, avec le même
protocole que le TTS Kyutai de moshi-server : Pi Voice s'y connecte sans changement, et les ~8 Go
de VRAM du TTS 1,6B restent libres pour le modèle de langage.

    uv run scripts/pocket-tts-server.py [--port 8089] [--language french] [--threads 2]

Client : ws://127.0.0.1:8089/api/tts_streaming?voice=estelle&format=PcmMessagePack
  <- {"type": "Ready"}
  -> {"type": "Text", "text": "un"} … {"type": "Eos"}         (MessagePack, mot par mot)
  <- {"type": "Text", "text": "…"} puis {"type": "Audio", "pcm": [f32 24 kHz …]} …  puis fermeture

Pocket TTS lit des phrases, pas des mots : les mots reçus sont regroupés jusqu'à une fin de phrase
(ou une virgule après quelques mots, pour démarrer plus vite), puis lus pendant que les suivants
arrivent. Une voix inconnue (ex. une voix du TTS 1,6B) est remplacée par la voix par défaut.
"""
import argparse
import asyncio
import queue
import threading
import urllib.parse

import msgpack
import torch
import websockets
from pocket_tts import TTSModel

# Une phrase de moins de MIN_WORDS mots attend la suivante (des bribes lues seules sonnent hachées).
MIN_WORDS = 3
# Sans ponctuation, on lit quand même au-delà de MAX_WORDS mots.
MAX_WORDS = 24
# Le premier segment part dès une virgule après FIRST_COMMA_WORDS mots : le premier son arrive plus tôt.
FIRST_COMMA_WORDS = 4
END = (".", "!", "?", "…", ":", ";")


class Engine:
    def __init__(self, language: str, default_voice: str):
        self.model = TTSModel.load_model(language=language)
        self.default_voice = default_voice
        self.voices: dict[str, dict] = {}
        self.lock = threading.Lock()  # un seul modèle : les générations passent l'une après l'autre
        self.voice_state(default_voice)
        print(f"[pocket-tts] modèle {language} prêt, voix par défaut « {default_voice} »", flush=True)

    def voice_state(self, voice: str) -> dict:
        if voice not in self.voices:
            try:
                self.voices[voice] = self.model.get_state_for_audio_prompt(voice)
            except Exception as e:  # voix du TTS 1,6B, clonage non autorisé, fichier absent…
                if voice != self.default_voice:
                    print(f"[pocket-tts] voix « {voice} » indisponible ({str(e)[:60]}), voix par défaut", flush=True)
                    self.voices[voice] = self.voice_state(self.default_voice)
                else:
                    raise
        return self.voices[voice]

    def speak(self, voice: str, text: str, out: "queue.Queue", stop: threading.Event):
        """Génère `text` en flux ; chaque morceau audio part dans `out` (thread de génération)."""
        state = self.voice_state(voice)
        with self.lock:
            for chunk in self.model.generate_audio_stream(state, text, stop=stop):
                if stop.is_set():
                    return
                out.put(chunk.numpy().astype("float32").tolist())


def segments(words: list[str], first: bool, final: bool) -> tuple[list[str], list[str]]:
    """Découpe les mots en (segments prêts à lire, reste en attente)."""
    ready, cur, done_first = [], [], not first
    for w in words:
        cur.append(w)
        n = len(cur)
        # Le premier segment part dès sa ponctuation (« Salut ! ») : la voix démarre plus tôt.
        if (w.endswith(END) and n >= (2 if not done_first else MIN_WORDS)) or n >= MAX_WORDS or (
            not done_first and w.endswith(",") and n >= FIRST_COMMA_WORDS
        ):
            ready.append(" ".join(cur))
            cur, done_first = [], True
    if final and cur:
        ready.append(" ".join(cur))
        cur = []
    return ready, cur


async def handle(ws, engine: Engine):
    query = urllib.parse.parse_qs(urllib.parse.urlparse(ws.request.path).query)
    voice = (query.get("voice", [""])[0] or engine.default_voice).strip()
    loop = asyncio.get_running_loop()
    stop = threading.Event()
    texts: asyncio.Queue = asyncio.Queue()  # segments à lire, None = fin
    await ws.send(msgpack.packb({"type": "Ready"}))

    async def receive():
        pending, first = [], True
        try:
            async for raw in ws:
                if not isinstance(raw, bytes):
                    continue
                m = msgpack.unpackb(raw)
                eos = m.get("type") == "Eos"
                if m.get("type") == "Text" and m.get("text", "").strip():
                    pending.append(m["text"].strip())
                ready, pending = segments(pending, first, eos)
                for s in ready:
                    first = False
                    await texts.put(s)
                if eos:
                    break
        finally:
            await texts.put(None)

    async def speak():
        while (text := await texts.get()) is not None:
            await ws.send(msgpack.packb({"type": "Text", "text": text}))
            chunks: queue.Queue = queue.Queue()
            job = loop.run_in_executor(None, engine.speak, voice, text, chunks, stop)
            while True:
                try:
                    pcm = await asyncio.to_thread(chunks.get, True, 0.05)
                except queue.Empty:
                    if job.done() and chunks.empty():
                        break
                    continue
                await ws.send(msgpack.packb({"type": "Audio", "pcm": pcm}, use_single_float=True))
            await job

    try:
        await asyncio.gather(receive(), speak())
    except websockets.ConnectionClosed:
        pass  # voix coupée par le client : on arrête la génération en cours
    except Exception as e:
        try:
            await ws.send(msgpack.packb({"type": "Error", "message": str(e)}))
        except Exception:
            pass
    finally:
        stop.set()


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8089)
    p.add_argument("--language", default="french")
    p.add_argument("--voice", default="estelle")
    p.add_argument("--threads", type=int, default=2)
    a = p.parse_args()
    torch.set_num_threads(a.threads)
    engine = Engine(a.language, a.voice)
    async with websockets.serve(lambda ws: handle(ws, engine), a.host, a.port, max_size=2**22):
        print(f"[pocket-tts] listening on ws://{a.host}:{a.port}", flush=True)
        await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(main())
