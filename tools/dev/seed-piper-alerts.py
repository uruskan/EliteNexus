"""Render the default-rate Piper alerts included in the Windows test package."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import wave
from io import BytesIO

# Keep in sync with crates/nexus-app/src/tts/audio_cache.rs.
PROFILE = b"elite-nexus-piper-en-us-kristin-medium-v1\0"
ROOT = Path(__file__).resolve().parents[2]
SPEED = 1.0


def alert_phrases():
    state = (ROOT / "crates/nexus-model/src/state.rs").read_text(encoding="utf-8")
    app = (ROOT / "crates/nexus-app/src/app.rs").read_text(encoding="utf-8")
    phrases = set(re.findall(r'\.speak\("([^"]+)"\)', state))
    phrases.update(re.findall(r'\.speaker\.say\("([^"]+)"\)', app))
    for name in ("genera", "species"):
        table = json.loads((ROOT / f"data/exobiology/{name}.json").read_text(encoding="utf-8"))
        rows = table["genera"].values() if name == "genera" else table
        for row in rows:
            if distance := row.get("colony_range_m"):
                phrases.add(f"Sample one logged. Move {distance} metres.")
                phrases.add(f"Second sample collected. Move another {distance} metres for the final sample.")
    return sorted(phrases)


def generate(folder, phrases):
    out = folder / "alerts"
    out.mkdir(exist_ok=True)
    with subprocess.Popen(
        [str(folder / "nexus-piper.exe")], cwd=folder,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        creationflags=subprocess.BELOW_NORMAL_PRIORITY_CLASS,
    ) as worker:
        try:
            for phrase in phrases:
                text = phrase.encode("utf-8")
                key = hashlib.sha256(PROFILE + struct.pack("<f", SPEED) + text).hexdigest()
                path = out / f"{key}.wav.gz"
                if path.is_file():
                    with wave.open(BytesIO(gzip.decompress(path.read_bytes()))) as wav:
                        if wav.getnframes() > 0:
                            print(f"Already rendered {phrase}", flush=True)
                            continue
                worker.stdin.write(b"1 1 " + text + b"\n")
                worker.stdin.flush()
                header = worker.stdout.read(4)
                if len(header) != 4:
                    raise RuntimeError(f"Piper exited while rendering {phrase!r}")
                size, = struct.unpack("<I", header)
                if not 46 <= size <= 4 * 1024 * 1024:
                    raise RuntimeError(f"Unexpected WAV size for {phrase!r}: {size}")
                data = worker.stdout.read(size)
                with wave.open(BytesIO(data)) as wav:
                    if len(data) != size or wav.getnframes() <= 0:
                        raise RuntimeError(f"Invalid audio for {phrase!r}")
                with tempfile.NamedTemporaryFile(dir=out, delete=False) as pending:
                    pending.write(gzip.compress(data, compresslevel=9, mtime=0))
                Path(pending.name).replace(path)
                print(f"{phrase} ({size:,} bytes -> {path.stat().st_size:,} bytes)", flush=True)
        finally:
            worker.stdin.close()
            worker.wait(timeout=20)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("piper_folder", type=Path)
    args = parser.parse_args()
    phrases = alert_phrases()
    print(f"Rendering {len(phrases)} phrases", flush=True)
    generate(args.piper_folder.resolve(), phrases)
