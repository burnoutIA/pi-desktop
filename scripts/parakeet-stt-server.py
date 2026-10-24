# /// script
# requires-python = ">=3.10,<3.15"
# dependencies = ["onnx-asr[cpu,hub]", "websockets>=13", "msgpack", "numpy", "scipy"]
# ///
"""Transcription Parakeet TDT 0.6B v3 (NVIDIA, multilingue) sur le processeur, avec le protocole du STT
Kyutai de moshi-server : Pi Voice s'y connecte sans changement de format. Sur la voix de l'utilisateur,
6,9 % de mots faux contre 18,6 % pour Kyutai STT 1B, et aucune mémoire GPU.

    uv run scripts/parakeet-stt-server.py [--port 8090] [--threads 4]

Client : ws://127.0.0.1:8090/api/asr-streaming
  <- {"type": "Ready"}
  -> {"type": "Audio", "pcm": [f32 24 kHz …]}       (trames de 80 ms, MessagePack)
  <- {"type": "Step", "prs": [p, p, p, p]}          une par trame : p = probabilité de pause
  <- {"type": "Word", "text": "…", "start_time": s}  après chaque pause (segment transcrit)
  -> {"type": "Flush"}   fin de dictée : on transcrit tout de suite ce qui reste
  <- {"type": "Flushed"} c'est fait, les derniers mots sont partis

Parakeet transcrit une phrase entière, pas mot à mot : un segment est transcrit quand la voix s'arrête
(~0,6 s de silence), ou à la demande (Flush).
"""
import argparse
import asyncio
import threading
import urllib.parse
from pathlib import Path

import msgpack
import numpy as np
import scipy.signal
import websockets

RATE_IN = 24000
RATE_ASR = 16000
STEP = 0.08  # une trame Kyutai = 80 ms
# Silence qui termine un segment (trames de 80 ms), et longueur au-delà de laquelle on coupe dès
# une courte pause (un très long segment retarderait les mots affichés).
SEGMENT_SILENCE = 8
LONG_SEGMENT_S = 20.0
SHORT_SILENCE = 3
# Pas de transcription sans un minimum de voix : évite les mots inventés sur du silence.
MIN_VOICED = 3
# Kyutai horodate ses mots ~0,5 s après leur début réel ; Pi Voice en tient compte (filtre de volume).
KYUTAI_LAG = 0.5
# Durées de pause des 4 probabilités envoyées à chaque trame (protocole Kyutai).
PAUSE_HEADS_S = (0.5, 1.0, 2.0, 3.0)
MODEL_DIR = Path.home() / ".cache" / "pi-voice" / "models" / "parakeet-tdt-0.6b-v3-onnx"


def load_model(threads: int):
    import onnx_asr
    import onnxruntime as ort
    from huggingface_hub import snapshot_download

    # Copie locale (le cache Hugging Face, fait de liens symboliques, est refusé par onnxruntime).
    snapshot_download(
        "istupakov/parakeet-tdt-0.6b-v3-onnx",
        local_dir=MODEL_DIR,
        allow_patterns=["config.json", "vocab.txt", "nemo128.onnx", "encoder-model.onnx", "encoder-model.onnx.data",
                        "decoder_joint-model.onnx"],
    )
    so = ort.SessionOptions()
    so.intra_op_num_threads = threads
    model = onnx_asr.load_model("nemo-parakeet-tdt-0.6b-v3", MODEL_DIR, sess_options=so)
    model.recognize(np.zeros(RATE_ASR, dtype=np.float32), sample_rate=RATE_ASR)  # chauffe
    return model


class Engine:
    def __init__(self, threads: int):
        self.model = load_model(threads)
        self.lock = threading.Lock()  # un seul modèle : une transcription à la fois
        print("[parakeet] modèle prêt", flush=True)

    def transcribe(self, audio16: np.ndarray) -> str:
        with self.lock:
            return self.model.recognize(audio16, sample_rate=RATE_ASR).strip()


class Session:
    """Un flux micro : découpage en segments sur les pauses, niveau de bruit appris au fil de l'eau."""

    def __init__(self):
        self.buf: list[np.ndarray] = []
        self.buf_start = 0.0  # instant (s, depuis le début du flux) du début du segment
        self.t = 0.0  # audio reçu (s)
        self.voiced = 0  # trames de voix dans le segment
        self.silence = 0  # trames de silence d'affilée
        self.noise = 0.003

    def push(self, pcm24: np.ndarray) -> np.ndarray:
        rms = float(np.sqrt(np.mean(pcm24 ** 2))) if len(pcm24) else 0.0
        voice = rms > max(0.006, self.noise * 3.0)
        if not voice:
            self.noise = 0.95 * self.noise + 0.05 * rms  # le bruit de fond n'apprend que sur le silence
        self.silence = 0 if voice else self.silence + 1
        if voice:
            self.voiced += 1
        a16 = scipy.signal.resample_poly(pcm24, 2, 3).astype(np.float32)
        if not self.buf:
            self.buf_start = self.t
        self.buf.append(a16)
        self.t += len(pcm24) / RATE_IN
        return a16

    def segment_ready(self) -> bool:
        if self.voiced < MIN_VOICED:
            return False
        long = sum(len(b) for b in self.buf) / RATE_ASR > LONG_SEGMENT_S
        return self.silence >= (SHORT_SILENCE if long else SEGMENT_SILENCE)

    def take(self) -> tuple[np.ndarray | None, float, float]:
        """Le segment à transcrire (None s'il n'y a pas de voix), son début et sa durée."""
        audio = np.concatenate(self.buf) if self.buf else np.zeros(0, dtype=np.float32)
        voiced, start = self.voiced, self.buf_start
        self.buf, self.voiced = [], 0
        if voiced < MIN_VOICED:
            return None, start, 0.0
        return audio, start, len(audio) / RATE_ASR

    def pause_probabilities(self) -> list[float]:
        """Comme les 4 têtes de Kyutai : pause d'au moins 0,5 s, 1 s, 2 s, 3 s. Pi Voice termine la
        phrase sur la tête 2 s : une simple respiration au milieu d'une longue phrase ne la coupe pas."""
        s = self.silence * STEP
        return [min(1.0, s / h) for h in PAUSE_HEADS_S]


async def handle(ws, engine: Engine):
    path = urllib.parse.urlparse(ws.request.path).path
    if not path.endswith("/api/asr-streaming"):
        await ws.close(code=1008, reason="unknown path")
        return
    loop = asyncio.get_running_loop()
    s = Session()
    send_lock = asyncio.Lock()
    pending: list[asyncio.Task] = []

    async def send(msg):
        async with send_lock:
            await ws.send(msgpack.packb(msg, use_single_float=True))

    async def emit(audio: np.ndarray | None, start: float, dur: float):
        if audio is None:
            return
        text = await loop.run_in_executor(None, engine.transcribe, audio)
        words = text.split()
        for i, w in enumerate(words):
            t = start + (i / max(1, len(words))) * dur + KYUTAI_LAG
            await send({"type": "Word", "text": w, "start_time": round(t, 2)})

    await send({"type": "Ready"})
    step = 0
    try:
        async for raw in ws:
            if not isinstance(raw, bytes):
                continue
            m = msgpack.unpackb(raw)
            kind = m.get("type")
            if kind == "Audio":
                s.push(np.asarray(m.get("pcm", []), dtype=np.float32))
                await send({"type": "Step", "step_idx": step, "prs": s.pause_probabilities()})
                step += 1
                if s.segment_ready():
                    pending.append(asyncio.create_task(emit(*s.take())))
            elif kind == "Flush":
                # Les segments déjà partis d'abord, puis le reste : les mots arrivent dans l'ordre.
                await asyncio.gather(*pending)
                pending.clear()
                await emit(*s.take())
                await send({"type": "Flushed"})
            pending = [t for t in pending if not t.done()]
        await asyncio.gather(*pending)
        await emit(*s.take())  # fin du flux : rien ne se perd
    except websockets.ConnectionClosed:
        pass


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8090)
    p.add_argument("--threads", type=int, default=4)
    a = p.parse_args()
    engine = Engine(a.threads)
    async with websockets.serve(lambda ws: handle(ws, engine), a.host, a.port, max_size=2**22):
        print(f"[parakeet] listening on ws://{a.host}:{a.port}", flush=True)
        await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(main())
