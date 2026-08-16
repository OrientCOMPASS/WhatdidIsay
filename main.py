#!/usr/bin/env python3
import sys
import os
import argparse
import queue
import threading
import time
import numpy as np
from datetime import datetime
from faster_whisper import WhisperModel

class TranscriptWriter:
    def __init__(self, save_dir):
        self.save_dir = save_dir
        self.fh = None
        self.cur_date = None
        self.cur_minute = None

    def write(self, real_time, text):
        date_str = real_time.strftime("%Y%m%d")
        minute_str = real_time.strftime("%H:%M")
        sec = real_time.second

        if date_str != self.cur_date:
            if self.fh:
                self.fh.close()
            self.fh = open(
                os.path.join(self.save_dir, f"{date_str}.txt"), "a", encoding="utf-8"
            )
            self.cur_date = date_str
            self.cur_minute = None

        if minute_str != self.cur_minute:
            self.fh.write(f"{minute_str}\n")
            self.cur_minute = minute_str

        self.fh.write(f"\t{sec} {text}\n")
        self.fh.flush()

    def close(self):
        if self.fh:
            self.fh.close()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", type=str, default="large-v2")
    parser.add_argument("--language", type=str, default="zh")
    parser.add_argument("--device", type=str, default="cpu")
    parser.add_argument("--compute-type", type=str, default="int8")
    parser.add_argument("--save-dir", type=str, default="./records")
    parser.add_argument("--sample-rate", type=int, default=48000)
    parser.add_argument("--channels", type=int, default=2)
    args = parser.parse_args()

    os.makedirs(args.save_dir, exist_ok=True)

    model_path = args.model
    model_bin = os.path.join(model_path, "model.bin")
    if not os.path.exists(model_bin):
        os.makedirs(model_path, exist_ok=True)
        print(f"[Main] Model not found at {model_path}.", file=sys.stderr, flush=True)
        print(f"[Main] Downloading from ModelScope...", file=sys.stderr, flush=True)
        try:
            from modelscope import snapshot_download
            snapshot_download(
                'pengzhendong/faster-whisper-large-v2',
                local_dir=model_path
            )
            print("[Main] Download complete.", file=sys.stderr, flush=True)
        except Exception as e:
            print(f"[Main] ERROR: Download failed: {e}", file=sys.stderr, flush=True)
            sys.exit(1)

    TARGET_SR = 16000
    DOWN_RATIO = args.sample_rate // TARGET_SR
    audio_queue = queue.Queue(maxsize=300)
    first_audio_ts = [None]

    def reader_loop():
        leftover = b''
        got_first = False
        try:
            while True:
                raw = sys.stdin.buffer.read1(65536)
                if not raw:
                    break
                if not got_first:
                    first_audio_ts[0] = time.time()
                    got_first = True
                raw = leftover + raw
                valid_len = len(raw) - (len(raw) % 4)
                leftover = raw[valid_len:]
                raw = raw[:valid_len]
                if not raw:
                    continue

                interleaved = np.frombuffer(raw, dtype=np.float32)
                mono = interleaved.reshape(-1, args.channels).mean(axis=1).astype(np.float32)
                mono = np.nan_to_num(mono, nan=0.0, posinf=0.0, neginf=0.0)

                if len(mono) < DOWN_RATIO:
                    continue

                trim = len(mono) - (len(mono) % DOWN_RATIO)
                mono = mono[:trim].reshape(-1, DOWN_RATIO).mean(axis=1).astype(np.float32)

                audio_queue.put(mono)
        except Exception:
            pass
        finally:
            audio_queue.put(None)

    t = threading.Thread(target=reader_loop, daemon=True)
    t.start()

    print(f"[Main] Loading model from: {model_path}", file=sys.stderr, flush=True)
    model = WhisperModel(model_path, device=args.device, compute_type=args.compute_type)
    print("[Main] Model loaded.", file=sys.stderr, flush=True)
    print("READY", file=sys.stdout, flush=True)

    def stream_generator():
        while True:
            chunk = audio_queue.get()
            if chunk is None:
                break
            yield chunk

    writer = TranscriptWriter(args.save_dir)

    try:
        segments = model.transcribe_stream(
            audio_stream=stream_generator(),
            language=args.language,
            initial_prompt="以下是普通话的句子。" if args.language == "zh" else "",
            vad_filter=True,
            condition_on_previous_text=True,
            beam_size=5,
        )
        for seg in segments:
            text = seg.text.strip()
            if not text:
                continue
            if first_audio_ts[0] is not None:
                real_time = datetime.fromtimestamp(first_audio_ts[0] + seg.start)
            else:
                real_time = datetime.now()
            writer.write(real_time, text)
            print(f"[ASR] {text}", file=sys.stderr, flush=True)
    finally:
        writer.close()

if __name__ == "__main__":
    main()