"""Package the native core and check the manifest against the final bytes."""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED

repo = Path(__file__).resolve().parents[2]
platform_id, core_path, build_path, upstream_path, media_path, out_path = sys.argv[1:]
core, build, upstream, media, out = map(Path, (core_path, build_path, upstream_path, media_path, out_path))
out.mkdir(parents=True, exist_ok=True)
apple = platform_id.startswith("macos")
windows = platform_id.startswith("windows")


def command(*args):
    return subprocess.check_output(args, text=True)


def stage_libraries(root):
    shutil.copy2(core, root / core.name)
    if not (apple or windows):
        return
    candidates = {}
    for directory in [media / "lib", media / "bin", build / "sdl", build]:
        if directory.is_dir():
            for file in sorted(directory.rglob("*.dylib" if apple else "*.dll")):
                candidates[file.name.lower()] = file
    queue = [root / core.name]
    staged = {core.name.lower()}
    while queue:
        library = queue.pop()
        if apple:
            names = [line.strip().split(" (compatibility", 1)[0] for line in command("otool", "-L", library).splitlines()[2:]]
        else:
            names = re.findall(r"DLL Name:\s*([^\s]+)", command("objdump", "-p", library), re.I)
        for name in names:
            filename = Path(name).name
            if apple and (name.startswith(("/System/", "/usr/lib/"))):
                continue
            candidate = candidates.get(filename.lower())
            if windows and not candidate:
                if filename.lower() in ("libwinpthread-1.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll"):
                    candidate = Path(shutil.which(filename) or "")
                else:
                    continue
            if not candidate or not candidate.is_file():
                raise RuntimeError(f"Unresolved bundled dependency of {library.name}: {name}")
            if filename.lower() not in staged:
                target = root / filename
                shutil.copy2(candidate, target)
                staged.add(filename.lower())
                queue.append(target)
            if apple and name != "@loader_path/" + filename:
                subprocess.run(["install_name_tool", "-change", name, "@loader_path/" + filename, library], check=True)
        if apple:
            subprocess.run(["install_name_tool", "-id", "@rpath/" + library.name, library], check=True)
    if apple:
        for dylib in root.glob("*.dylib"):
            subprocess.run(["codesign", "--force", "--sign", "-", dylib], check=True)


with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    stage_libraries(root)
    legal = root / "licenses"
    for project, source, names in [
        ("ShinkiEMU", repo, ["LICENSE-APACHE", "LICENSE-MIT"]),
        ("upstream", upstream, ["LICENSE", "NOTICE.md", "THIRD_PARTY_NOTICES.md", "third_party/stb/README.md",
                               "third_party/stb/stb_vorbis.c", "third_party/wa2_subtitles/LICENSE",
                               "third_party/wa2_subtitles/FONT-NOTICE.txt"]),
        ("SDL2", upstream / "tools/SDL-release-2.32.10", ["LICENSE.txt"]),
        ("FFmpeg", upstream / "third_party/ffmpeg", ["COPYING.LGPLv2.1"]),
    ]:
        for name in names:
            dest = legal / project / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source / name, dest)
    if windows:
        msys = Path(shutil.which("gcc")).resolve().parents[1]
        for pattern in ["gcc*", "*winpthreads*"]:
            for notice in (msys / "share/licenses").glob(pattern):
                if notice.is_dir():
                    shutil.copytree(notice, legal / "toolchain" / notice.name, dirs_exist_ok=True)
    (legal / "BUILD.txt").write_text(
        "Engine: https://github.com/dorakyuraduang/whitealbum2-Android at e5b9dcd22123fdb51a5c3db24d76f81afbb3ce68\n"
        "Adapter, engine changes and build: Tools/leaf in the accompanying ShinkiEMU source branch.\n"
        "FFmpeg (bundled builds): https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz\n"
        "FFmpeg source archive SHA-256: 05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41\n"
        "FFmpeg libraries can be replaced independently of the engine.\n", encoding="utf-8")
    def sha(data):
        return hashlib.sha256(data).hexdigest()
    files = {p.relative_to(root).as_posix(): p.read_bytes() for p in root.rglob("*") if p.is_file()}
    manifest = dict(schema="shinki.emu.core_package.v1", plugin_id="whitealbum2-Android",
                    version="0.1.0-trial.3", abi="shinki.emu.native.c.v3", library=core.name,
                    files=[dict(path=n, sha256=sha(b)) for n,b in sorted(files.items())])
    package = out / ("ShinkiEMU-Leaf-" + platform_id + ".zip")
    with ZipFile(package, "w", ZIP_DEFLATED) as z:
        z.writestr("core.json", json.dumps(manifest, indent=2))
        for name, data in files.items():
            z.writestr(name, data)
    with ZipFile(package) as z:
        assert z.testzip() is None
        for record in manifest["files"]:
            assert sha(z.read(record["path"])) == record["sha256"]
    print(package)
