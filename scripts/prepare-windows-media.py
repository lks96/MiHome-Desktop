#!/usr/bin/env python3
"""Build the minimal, replaceable LGPL Windows video helper from verified source.

Requires a Linux host with x86_64-w64-mingw32 GCC/binutils and nasm. No vendor
SDK, GPL codec library, cloud credential or downloaded executable is used.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
VERSION = "8.1.3"
SHA256 = "7138d28c96d9d3e3af4ee3d8cad72741f8ffb40da90c1112235dea3ecd3178a3"
URL = f"https://ffmpeg.org/releases/ffmpeg-{VERSION}.tar.xz"
CACHE = Path(os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache"))) / "ffmpeg"
TARGET = ROOT / "src-tauri/resources/ffmpeg"
FLAGS = [
    "--target-os=mingw32", "--arch=x86_64", "--cross-prefix=x86_64-w64-mingw32-",
    "--cc=x86_64-w64-mingw32-gcc-posix", "--enable-cross-compile",
    "--disable-everything", "--disable-autodetect", "--disable-network", "--disable-doc",
    "--disable-debug", "--disable-ffplay", "--disable-ffprobe", "--disable-avdevice",
    "--disable-swresample", "--disable-pthreads", "--enable-w32threads",
    "--enable-shared", "--disable-static", "--enable-small", "--enable-ffmpeg",
    "--enable-protocol=file,pipe", "--enable-demuxer=h264,hevc", "--enable-parser=h264,hevc",
    "--enable-decoder=h264,hevc", "--enable-mediafoundation", "--enable-d3d11va",
    "--enable-encoder=h264_mf", "--enable-muxer=hls,mpegts",
    "--enable-filter=buffer,buffersink,format,scale,null,fps",
    "--enable-bsf=h264_mp4toannexb,hevc_mp4toannexb,extract_extradata",
    "--extra-ldflags=-static",
]

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    cross = Path(os.environ.get("FFMPEG_CROSS_ROOT", "/workspace/.toolchains/ffmpeg-cross"))
    env = os.environ.copy()
    if cross.is_dir():
        env["PATH"] = str(cross / "usr/bin") + os.pathsep + env["PATH"]
    for executable in ("x86_64-w64-mingw32-gcc-posix", "x86_64-w64-mingw32-ld", "nasm", "make"):
        if not shutil.which(executable, path=env["PATH"]):
            raise SystemExit(f"Missing build dependency: {executable}; see docs/WINDOWS_BUILD.md")
    manifest_path = TARGET / "BUILD_INFO.json"
    if manifest_path.is_file():
        manifest = json.loads(manifest_path.read_text())
        if manifest.get("source_sha256") == SHA256 and manifest.get("configure") == FLAGS and manifest.get("builder_sha256") == digest(Path(__file__)):
            if all((TARGET / name).is_file() and digest(TARGET / name) == value for name,value in manifest["files"].items()):
                print("Verified Windows media helper already prepared")
                return
    CACHE.mkdir(parents=True, exist_ok=True)
    archive = CACHE / f"ffmpeg-{VERSION}.tar.xz"
    if not archive.is_file():
        partial = archive.with_suffix(".download")
        with urllib.request.urlopen(URL, timeout=120) as response, partial.open("wb") as stream:
            shutil.copyfileobj(response, stream)
        partial.replace(archive)
    if digest(archive) != SHA256:
        raise SystemExit("FFmpeg source checksum mismatch")
    source = CACHE / "minimal-build"
    if not (source / "configure").is_file():
        with tarfile.open(archive) as package:
            package.extractall(CACHE / "source", filter="data")
        (CACHE / "source" / f"ffmpeg-{VERSION}").rename(source)
    prefix = CACHE / "minimal-install"
    subprocess.run(["./configure", *FLAGS, f"--prefix={prefix}"], cwd=source, env=env, check=True)
    subprocess.run(["make", f"-j{os.environ.get('CARGO_BUILD_JOBS','3')}"], cwd=source, env=env, check=True)
    subprocess.run(["make", "install"], cwd=source, env=env, check=True)
    (TARGET / "bin").mkdir(parents=True, exist_ok=True)
    for file in (prefix / "bin").iterdir():
        if file.suffix.lower() in (".exe", ".dll"):
            shutil.copy2(file, TARGET / "bin" / file.name)
    # Complete corresponding FFmpeg source and exact configuration accompany the binaries.
    shutil.copy2(archive, TARGET / archive.name)
    shutil.copy2(source / "COPYING.LGPLv2.1", TARGET / "COPYING.LGPLv2.1")
    shutil.copy2(source / "LICENSE.md", TARGET / "FFmpeg-LICENSE.md")
    shutil.copy2(ROOT / "scripts/prepare-windows-media.py", TARGET / "rebuild.py")
    for copyright_file in (cross / "usr/share/doc/gcc-mingw-w64-base/copyright", Path("/usr/share/doc/gcc-mingw-w64-base/copyright")):
        if copyright_file.is_file():
            shutil.copy2(copyright_file, TARGET / "MinGW-GCC-COPYRIGHT.txt")
            break
    mingw_notice = cross / "usr/share/doc/mingw-w64-common/copyright"
    if not mingw_notice.is_file():
        mingw_notice = Path("/usr/share/doc/mingw-w64-common/copyright")
    if mingw_notice.is_file():
        shutil.copy2(mingw_notice, TARGET / "MinGW-RUNTIME-COPYRIGHT.txt")
    (TARGET / "NOTICE.txt").write_text(
        f"FFmpeg {VERSION} — Copyright the FFmpeg developers. LGPL 2.1 or later.\n"
        "Used as a separate process. Shared DLLs may be replaced/rebuilt; reverse engineering\n"
        "for debugging modifications to these LGPL components is permitted.\n"
        "No GPL/nonfree codecs enabled. h264_mf uses Windows Media Foundation.\n"
        "GCC runtime uses the GCC Runtime Library Exception; see MinGW-GCC-COPYRIGHT.txt.\n"
        f"Unmodified corresponding source: ffmpeg-{VERSION}.tar.xz (included).\n"
        f"Upstream: {URL}\nBuild/rebuild: scripts/prepare-windows-media.py in MiHome-Desktop.\n"
        "See BUILD_INFO.json for configuration and checksums.\n", encoding="utf-8")
    files = {str(p.relative_to(TARGET)): digest(p) for p in TARGET.rglob("*") if p.is_file() and p != manifest_path}
    manifest_path.write_text(json.dumps({"version": VERSION, "source_url": URL,
        "source_sha256": SHA256, "builder_sha256":digest(Path(__file__)), "configure": FLAGS, "files": files}, indent=2)+"\n")
    print("Prepared Windows media helper:", TARGET)

if __name__ == "__main__":
    main()
