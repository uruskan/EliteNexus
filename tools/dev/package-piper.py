"""Package a Windows x64 Elite Nexus executable with the optional Piper voice."""
import argparse
import hashlib
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from urllib.request import urlretrieve
import zipfile

ROOT = Path(__file__).resolve().parents[2]
RELEASES = "https://github.com/k2-fsa/sherpa-onnx/releases/download"
RUNTIME = "sherpa-onnx-v1.12.20-win-x64-shared"
VOICE = "vits-piper-en_US-kristin-medium"
ORT_WHEEL = (
    "https://files.pythonhosted.org/packages/43/24/"
    "37ca26db9e135817cbda65bae34af02a6f5c394a3968d07f2bf190e788fd/"
    "onnxruntime-1.17.1-cp311-cp311-win_amd64.whl"
)
SHERPA_LICENSE = "https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.12.20/LICENSE"


def fetch(path, url, expected):
    print(f"Downloading {path.name}", flush=True)
    urlretrieve(url, path)
    with path.open("rb") as downloaded:
        digest = hashlib.file_digest(downloaded, "sha256").hexdigest()
    if digest != expected:
        raise RuntimeError(f"SHA-256 mismatch for {path.name}")


def main():
    if sys.platform != "win32" or not shutil.which("cl.exe"):
        raise SystemExit("Run in an x64 Visual Studio Developer PowerShell with MSVC installed.")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path, help="Built EliteNexus.exe")
    parser.add_argument("--out", required=True, type=Path, help="New portable output directory")
    args = parser.parse_args()
    exe, out = args.exe.resolve(), args.out.resolve()
    if not exe.is_file() or out.exists():
        parser.error("--exe must exist and --out must be a new directory")
    out.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="piper-package-", dir=out.parent) as tmp:
        work = Path(tmp)
        runtime_archive = work / "runtime.tar.bz2"
        voice_archive = work / "voice.tar.bz2"
        wheel = work / "onnxruntime.whl"
        fetch(runtime_archive, f"{RELEASES}/v1.12.20/{RUNTIME}.tar.bz2",
              "2aa9c11f13d6a20308afedcaa8c04a678a1aaebc206b7609e4df7d7910582f56")
        fetch(voice_archive, f"{RELEASES}/tts-models/{VOICE}.tar.bz2",
              "c2206f572df2956c50b1ae3367eebce3853c663e890cba8048cd62b1e4dbe6c7")
        fetch(wheel, ORT_WHEEL, "53e4e06c0a541696ebdf96085fd9390304b7b04b748a19e02cf3b35c869a1e76")
        for archive in (runtime_archive, voice_archive):
            subprocess.run(["tar.exe", "-xf", str(archive), "-C", str(work)], check=True)

        stage = work / "package"
        piper = stage / "piper"
        piper.mkdir(parents=True)
        shutil.move(work / VOICE, piper / "model")
        runtime = work / RUNTIME
        for name in ("sherpa-onnx-c-api.dll", "onnxruntime.dll", "onnxruntime_providers_shared.dll"):
            shutil.copyfile(runtime / "lib" / name, piper / name)
        with zipfile.ZipFile(wheel) as archive:
            for name in ("LICENSE", "ThirdPartyNotices.txt"):
                (piper / f"ONNXRuntime-{name}").write_bytes(archive.read(f"onnxruntime/{name}"))
        urlretrieve(SHERPA_LICENSE, piper / "LICENSE-sherpa-onnx.txt")

        subprocess.run([
            "cl.exe", "/nologo", "/std:c++17", "/O2", "/MT", "/EHsc",
            f"/I{runtime / 'include'}", f"/Fo{work / 'piper-worker.obj'}",
            f"/Fe{piper / 'nexus-piper.exe'}", str(ROOT / "tools/dev/piper-worker.cpp"),
            "/link", str(runtime / "lib/sherpa-onnx-c-api.lib"),
        ], cwd=work, check=True)
        subprocess.run([sys.executable, str(ROOT / "tools/dev/seed-piper-alerts.py"), str(piper)], check=True)

        shutil.copyfile(exe, stage / "EliteNexus.exe")
        for name in ("README.md", "LICENSE", "NOTICE.md", "docs/USER_GUIDE.md"):
            path = ROOT / name
            shutil.copyfile(path, stage / path.name)
        shutil.copyfile(ROOT / "data/fonts/inter/LICENSE.txt", stage / "Inter-LICENSE.txt")
        stage.rename(out)
    print(f"Piper package ready: {out}")


if __name__ == "__main__":
    main()
