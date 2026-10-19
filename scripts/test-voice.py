# /// script
# requires-python = ">=3.11"
# dependencies = ["msgpack", "websockets", "numpy"]
# ///
"""Aller-retour de test : TTS français -> fichier WAV -> STT.

    uv run scripts/test-voice.py ["texte"] [--voice chemin/voix.wav]
"""
import argparse
import asyncio
import time
import urllib.parse
import wave

import msgpack
import numpy as np
import websockets

RATE = 24000
AUTH = "auth_id=public_token"


async def tts(url: str, voice: str, text: str) -> tuple[np.ndarray, float]:
    q = urllib.parse.urlencode({"voice": voice, "format": "PcmMessagePack", "cfg_alpha": 1.5})
    chunks, first = [], None
    t0 = time.perf_counter()
    async with websockets.connect(f"{url}/api/tts_streaming?{q}&{AUTH}") as ws:
        async def send():
            for w in text.split():
                await ws.send(msgpack.packb({"type": "Text", "text": w}))
            await ws.send(msgpack.packb({"type": "Eos"}))

        sender = asyncio.create_task(send())
        async for raw in ws:
            m = msgpack.unpackb(raw)
            if m["type"] == "Audio":
                first = first or time.perf_counter() - t0
                chunks.append(np.array(m["pcm"], dtype=np.float32))
            elif m["type"] == "Error":
                raise RuntimeError(m["message"])
        await sender
    return np.concatenate(chunks), first or 0.0


async def stt(url: str, audio: np.ndarray) -> str:
    words = []
    async with websockets.connect(f"{url}/api/asr-streaming?{AUTH}") as ws:
        async def send():
            # Audio + 2 s de silence pour laisser sortir les derniers mots (retard de 0,5 s).
            pcm = np.concatenate([audio, np.zeros(RATE * 2, dtype=np.float32)])
            for i in range(0, len(pcm), 1920):
                chunk = pcm[i : i + 1920]
                await ws.send(msgpack.packb({"type": "Audio", "pcm": chunk.tolist()}, use_single_float=True))
                await asyncio.sleep(0.01)
            await asyncio.sleep(1.0)
            await ws.close()

        sender = asyncio.create_task(send())
        try:
            async for raw in ws:
                m = msgpack.unpackb(raw)
                if m["type"] == "Word":
                    words.append(m["text"])
        except websockets.ConnectionClosed:
            pass
        await sender
    return " ".join(words)


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("text", nargs="?", default="Bonjour ! J'ai terminé la page d'accueil et corrigé le bug du formulaire.")
    p.add_argument("--voice", default="estelle")
    p.add_argument("--tts", default="ws://127.0.0.1:8089")
    p.add_argument("--stt", default="ws://127.0.0.1:8090")
    p.add_argument("--out", default="test-voice.wav")
    a = p.parse_args()

    audio, first = await tts(a.tts, a.voice, a.text)
    with wave.open(a.out, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes((np.clip(audio, -1, 1) * 32767).astype(np.int16).tobytes())
    print(f"TTS : {len(audio) / RATE:.1f} s d'audio, premier son après {first * 1000:.0f} ms -> {a.out}")
    print(f"STT : {await stt(a.stt, audio)}")


asyncio.run(main())
