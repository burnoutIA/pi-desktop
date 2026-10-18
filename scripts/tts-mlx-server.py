# /// script
# requires-python = ">=3.12"
# dependencies = ["moshi_mlx==0.3.0", "msgpack>=1", "numpy", "websockets>=13"]
# ///
"""Synthèse vocale Kyutai sur Mac, avec MLX (GPU Apple), à la place du TTS de moshi-server.

    uv run scripts/tts-mlx-server.py [--port 8089] [--quantize 8]

Même protocole que moshi-server, l'app ne voit pas la différence :
  ws://127.0.0.1:8089/api/tts_streaming?voice=unmute-prod-website/developpeuse-3.wav&cfg_alpha=1.5
  -> MessagePack : {"type": "Text", "text": "<mot>"}…, puis {"type": "Eos"}
  <- MessagePack : {"type": "Ready"}, puis {"type": "Audio", "pcm": [float32, 24 kHz]}…
                   et {"type": "Text", "text": "<mot>"} quand un mot commence à être prononcé.

Le modèle est chargé une fois ; le port n'ouvre qu'une fois le modèle prêt et préchauffé.
Une seule synthèse à la fois : la suivante attend, et une connexion fermée arrête la sienne.
"""
import argparse
import asyncio
import json
import queue
import threading
import time
import urllib.parse
from concurrent.futures import ThreadPoolExecutor

import mlx.core as mx
import mlx.nn as nn
import msgpack
import numpy as np
import sentencepiece
import websockets
from moshi_mlx import models
from moshi_mlx.models.generate import LmGen
from moshi_mlx.models.tts import DEFAULT_DSM_TTS_REPO, DEFAULT_DSM_TTS_VOICE_REPO, TTSModel, script_to_entries
from moshi_mlx.modules.conditioner import ConditionTensor
from moshi_mlx.utils.loaders import hf_get
from moshi_mlx.utils.sampling import Sampler

PATH = "/api/tts_streaming"
DEFAULT_VOICE = "unmute-prod-website/developpeuse-3.wav"
DEFAULT_CFG = 2.0  # comme scripts/kyutai/tts.toml


def log(msg: str):
    print(f"[tts-mlx] {msg}", flush=True)


def load(repo: str, quantize: int | None, n_q: int) -> TTSModel:
    with open(hf_get("config.json", repo), "r") as f:
        raw_config = json.load(f)
    lm_config = models.LmConfig.from_config_dict(raw_config)
    # Bogue du cache KV circulaire dans moshi_mlx <= 0.3.0 (contournement de Kyutai).
    lm_config.transformer.max_seq_len = lm_config.transformer.context
    lm = models.Lm(lm_config)
    lm.set_dtype(mx.bfloat16)
    lm.load_pytorch_weights(str(hf_get(raw_config.get("moshi_name", "model.safetensors"), repo)), lm_config, strict=True)
    if quantize:
        log(f"quantification {quantize} bits")
        nn.quantize(lm.depformer, bits=quantize)
        for layer in lm.transformer.layers:
            nn.quantize(layer.self_attn, bits=quantize)
            nn.quantize(layer.gating, bits=quantize)
    if n_q < lm.dep_q:
        # Comme moshi (PyTorch) avec n_q = 24 : le depformer s'arrête aux n_q premiers codebooks,
        # les suivants entrent comme jetons « zéro ». Compromis qualité / vitesse prévu par Kyutai.
        lm.depformer.slices = lm.depformer.slices[:n_q]
        lm.cfg.depformer.num_slices = n_q
    tokenizer = sentencepiece.SentencePieceProcessor(str(hf_get(raw_config["tokenizer_name"], repo)))  # type: ignore
    # Mimi complet : son décodeur additionne autant de codebooks qu'on lui en donne.
    mimi = models.mimi.Mimi(models.mimi_202407(raw_config["dep_q"]))
    mimi.load_pytorch_weights(str(hf_get(raw_config["mimi_name"], repo)), strict=True)
    return TTSModel(
        lm, mimi, tokenizer, voice_repo=DEFAULT_DSM_TTS_VOICE_REPO, temp=0.6, cfg_coef=1.0,
        max_padding=8, initial_padding=2, final_padding=2, padding_bonus=0, raw_config=raw_config,
    )


class FastLmGen(LmGen):
    """LmGen sans ses vérifications `if (…).any()` : chacune force une synchronisation avec le GPU,
    une trentaine par pas (une par codebook), soit une bonne part du temps de calcul sur Mac."""

    def _step(self, other_audio_tokens, ct=None, cross_attention_src=None):
        if self.step_idx >= self.max_steps:
            raise ValueError(f"reached max-steps {self.max_steps}")
        if self.step_idx == 0:
            text_tokens = mx.full((self.batch_size, 1), self.model.cfg.text_out_vocab_size, dtype=mx.int32)
        else:
            text_tokens = self.gen_sequence[:, 0, self.step_idx - 1 : self.step_idx]
        self.gen_sequence[:, 1 + self.main_codebooks :, self.step_idx] = other_audio_tokens
        audio_tokens = []
        for cb_idx, delay in enumerate(self.audio_delays):
            gen_idx = self.step_idx - 1 - delay
            if gen_idx >= 0:
                audio_tokens.append(self.gen_sequence[:, cb_idx + 1, gen_idx][None])
            else:
                audio_tokens.append(mx.array([[self.audio_padding_token]]))
        text_tokens, audio_tokens, transformer_out = self.model._sample(
            text_tokens, audio_tokens, self.text_sampler, self.audio_sampler, ct=ct,
            cross_attention_src=cross_attention_src, cfg_coef=self.cfg_coef,
            on_text_hook=self.on_text_hook, on_audio_hook=self.on_audio_hook,
        )
        self.gen_sequence[:, 0, self.step_idx] = text_tokens.squeeze(-1)
        for cb_idx, delay in enumerate(self.audio_delays[: self.main_codebooks]):
            gen_idx = self.step_idx - delay
            if gen_idx >= 0:
                self.gen_sequence[:, cb_idx + 1, gen_idx] = audio_tokens[:, cb_idx, 0]
        self.step_idx += 1
        return text_tokens, transformer_out

    def last_audio_tokens(self):
        # Pendant les premiers pas (retards des codebooks), rien n'est encore prêt : connu sans GPU.
        gen_idx = self.step_idx - 1 - self.max_delay
        if gen_idx < 0:
            return None
        return self.gen_sequence[:, 1 : 1 + self.main_codebooks, gen_idx]


_conditionings: dict = {}


def conditioning(tts: TTSModel, voice: str, cfg: float):
    """Conditionnement (voix + CFG) d'une synthèse, calculé une fois par voix."""
    valid = tts.valid_cfg_conditionings
    cfg_cond = (cfg if cfg in valid else DEFAULT_CFG if DEFAULT_CFG in valid else None) if valid else None
    key = (voice, cfg_cond)
    if key not in _conditionings:
        voices = [tts.get_voice_path(voice)] if tts.multi_speaker else []
        attr = tts.make_condition_attributes(voices, cfg_cond)
        provider = tts.lm.condition_provider
        assert provider is not None
        ct = cross = None
        for k, value in attr.text.items():
            t = provider.condition_tensor(k, value)
            ct = t if ct is None else ConditionTensor(ct.tensor + t.tensor)
        for k, value in attr.tensor.items():
            cross = provider.conditioners[k].condition(value)
        mx.eval([x.tensor if isinstance(x, ConditionTensor) else x for x in (ct, cross) if x is not None])
        _conditionings[key] = (ct, cross)
    return _conditionings[key]


class Synth:
    """Une synthèse en flux : on lui donne des mots, elle rend des trames audio de 80 ms."""

    def __init__(self, tts: TTSModel, voice: str, cfg: float, on_pcm, on_word):
        self.tts, self.on_pcm, self.on_word = tts, on_pcm, on_word
        for c in tts.lm.transformer_cache:
            c.reset()
        for c in tts.lm.depformer_cache:
            c.reset()
        tts.mimi.reset_all()
        self.offset = 0
        self.first = True
        self.state = tts.machine.new_state([])

        self.ct, self.cross = conditioning(tts, voice, cfg)

        # moshi_mlx 0.3 : les jetons arrivent en [batch, …] (batch = 1 ici).
        def on_audio(tokens):
            for q in range(tokens.shape[1]):
                if self.offset < tts.lm.delays[q] + tts.delay_steps:
                    tokens[:, q] = tts.machine.token_ids.zero

        def on_text(tokens):
            o, consumed = tts.machine.process(self.offset, self.state, tokens.tolist()[0][0])
            if consumed:
                self.on_word(self.state.transcript[-1][0])
            tokens[:] = mx.array([[o]], dtype=mx.int64)

        self.gen = FastLmGen(
            tts.lm, max_steps=tts.max_gen_length, text_sampler=Sampler(temp=tts.temp),
            audio_sampler=Sampler(temp=tts.temp), cfg_coef=1.0, on_text_hook=on_text, on_audio_hook=on_audio,
        )
        self.missing = tts.lm.n_q - tts.lm.dep_q

    def _step(self):
        tokens = mx.ones((1, self.missing), dtype=mx.int64) * self.tts.machine.token_ids.zero
        self.gen.step(tokens, ct=self.ct, cross_attention_src=self.cross)
        frame = self.gen.last_audio_tokens()
        self.offset += 1
        if frame is not None and not (frame == -1).any():
            pcm = self.tts.mimi.decode_step(frame[:, :, None])
            self.on_pcm(np.array(mx.clip(pcm[0, 0], -1, 1), dtype=np.float32))

    def word(self, text: str, cancel: threading.Event):
        try:
            entries = script_to_entries(
                self.tts.tokenizer, self.tts.machine.token_ids, self.tts.mimi.frame_rate, [text],
                multi_speaker=self.first and self.tts.multi_speaker, padding_between=1,
            )
        except AssertionError:
            return  # mot vide une fois nettoyé (ponctuation seule…)
        self.first = False
        for e in entries:
            self.state.entries.append(e)
            # On garde d'avance les mots dont le modèle a besoin (second flux de texte).
            while len(self.state.entries) > self.tts.machine.second_stream_ahead and not cancel.is_set():
                self._step()

    def finish(self, cancel: threading.Event):
        while (self.state.entries or self.state.end_step is None) and not cancel.is_set():
            self._step()
        for _ in range(self.tts.delay_steps + max(self.tts.lm.delays) + 8):
            if cancel.is_set():
                return
            self._step()


def run_session(tts: TTSModel, voice: str, cfg: float, words: queue.Queue, cancel: threading.Event, emit):
    t0 = time.perf_counter()
    n = 0

    def on_pcm(pcm):
        nonlocal n
        n += len(pcm)
        emit({"type": "Audio", "pcm": pcm.tolist()})

    synth = Synth(tts, voice, cfg, on_pcm, lambda w: emit({"type": "Text", "text": w}))
    while not cancel.is_set():
        w = words.get()
        if w is None:
            synth.finish(cancel)
            break
        synth.word(w, cancel)
    dt = time.perf_counter() - t0
    if n:
        log(f"{n / 24000:.1f} s d'audio en {dt:.1f} s{' (interrompu)' if cancel.is_set() else ''}")


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8089)
    parser.add_argument("--hf-repo", default=DEFAULT_DSM_TTS_REPO)
    parser.add_argument("--quantize", type=int, help="ex. 8 pour 8 bits")
    parser.add_argument("--n-q", type=int, default=24, help="codebooks audio générés (32 max, 24 comme moshi-server)")
    args = parser.parse_args()
    mx.random.seed(299792458)

    log(f"chargement de {args.hf_repo}")
    tts = load(args.hf_repo, args.quantize, args.n_q)
    # Un seul fil pour MLX : les synthèses passent l'une après l'autre.
    worker = ThreadPoolExecutor(max_workers=1)
    log("préchauffage")
    warm = queue.Queue()
    for w in ["Bonjour.", None]:
        warm.put(w)
    await asyncio.get_running_loop().run_in_executor(
        worker, run_session, tts, DEFAULT_VOICE, DEFAULT_CFG, warm, threading.Event(), lambda m: None
    )

    async def handler(ws):
        url = urllib.parse.urlparse(ws.request.path)
        if url.path != PATH:
            await ws.close(code=1008, reason="chemin inconnu")
            return
        q = urllib.parse.parse_qs(url.query)
        voice = q.get("voice", [DEFAULT_VOICE])[0]
        try:
            cfg = float(q.get("cfg_alpha", [DEFAULT_CFG])[0])
        except ValueError:
            cfg = DEFAULT_CFG
        loop = asyncio.get_running_loop()
        out: asyncio.Queue = asyncio.Queue()
        words: queue.Queue = queue.Queue()
        cancel = threading.Event()

        def emit(msg):
            loop.call_soon_threadsafe(out.put_nowait, msgpack.packb(msg, use_single_float=True))

        async def reader():
            try:
                async for raw in ws:
                    if not isinstance(raw, bytes):
                        continue
                    m = msgpack.unpackb(raw)
                    if m.get("type") == "Text":
                        for w in str(m.get("text", "")).split():
                            words.put(w)
                    elif m.get("type") == "Eos":
                        words.put(None)
                        return
            finally:
                words.put(None)  # sans effet si Eos déjà reçu ; débloque la synthèse sinon

        async def writer():
            while True:
                b = await out.get()
                if b is None:
                    return
                await ws.send(b)

        def session():
            try:
                run_session(tts, voice, cfg, words, cancel, emit)
            except Exception as e:  # voix inconnue, etc.
                log(f"erreur : {e!r}")
                emit({"type": "Error", "message": str(e)})
            finally:
                loop.call_soon_threadsafe(out.put_nowait, None)

        await ws.send(msgpack.packb({"type": "Ready"}))
        rd = asyncio.create_task(reader())
        wr = asyncio.create_task(writer())
        job = loop.run_in_executor(worker, session)
        try:
            await wr
        except websockets.ConnectionClosed:
            pass
        finally:
            cancel.set()
            rd.cancel()
            await asyncio.wait([job])
            await ws.close()

    async with websockets.serve(handler, "127.0.0.1", args.port, max_size=None, compression=None):
        log(f"listening on ws://127.0.0.1:{args.port}{PATH}")
        await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(main())
