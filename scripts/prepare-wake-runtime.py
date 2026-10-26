"""Reproducible Windows x64 inference bundle, with a closed file manifest.

Never traverses a developer environment, training corpus or owner recording
directory. Downloaded archives are pinned before extraction; inference source
is selected from the restored detector without altering its algorithm.
"""
from __future__ import annotations
import argparse
import ast
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "tools/wake-training"
PINS = ROOT / "tools/wake-release/dependencies.json"
SOURCE_PINS = {
    "live_worker.py": "5f6e17258ac8370ab60d0b13f51fe8b08e024a5f1ce7d179c8333a8af50189ed",
    "live_runtime.py": "dfda02afc5c309dca8223568b96d6ab2366810c3a27cee41f990c68ed0eb7005",
    "lira-live-policy.json": "dd7d64a86045d97fa06da87c47711f082129df970c399aa5a6aa974623fc7642",
}
CNN = "a9c4e2595669605325b97b7aeec62781c58073e9966a3850d360328bddaa541c"
POINT = "d3ea4079b329755bcdd42618cec915aa4952554479eb4b0999689558305ee47e"


def digest(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(4 * 1024**2), b""):
            result.update(block)
    return result.hexdigest()


def fetched(pin, cache):
    name = Path(urllib.parse.unquote(urllib.parse.urlparse(pin["url"]).path)).name
    destination = cache / name
    if destination.exists() and digest(destination) == pin["sha256"]:
        return destination
    temporary = destination.with_suffix(destination.suffix + ".download")
    with urllib.request.urlopen(pin["url"], timeout=60) as response, temporary.open("wb") as target:
        if urllib.parse.urlparse(response.url).scheme != "https":
            raise ValueError("HTTPS required")
        shutil.copyfileobj(response, target)
    if digest(temporary) != pin["sha256"]:
        temporary.unlink()
        raise ValueError("dependency archive hash mismatch")
    temporary.replace(destination)
    return destination


def safe_name(name):
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or "\\" in name or ":" in name:
        raise ValueError("unsafe archive member")
    return path


def copy_archive_file(target, source):
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("wb") as output:
        shutil.copyfileobj(source, output)


def python_member(parts):
    if len(parts) < 2 or parts[0] != "python":
        return False
    relative = parts[1:]
    if len(relative) == 1:
        return relative[0] in {"python.exe", "python3.dll", "python312.dll", "vcruntime140.dll", "vcruntime140_1.dll", "LICENSE.txt"}
    if relative[0] == "DLLs":
        return relative[-1].endswith((".pyd", ".dll")) and not relative[-1].startswith(("_test", "_tkinter", "tcl", "tk"))
    if relative[0] != "Lib":
        return False
    if any(part in {"test", "tests", "__pycache__", "site-packages", "ensurepip", "idlelib", "tkinter", "turtledemo", "venv"} for part in relative):
        return False
    return relative[-1].endswith(".py")


def unpack_python(archive, output):
    with tarfile.open(archive, "r:gz") as source:
        for member in source.getmembers():
            path = safe_name(member.name)
            if not python_member(path.parts) or member.isdir():
                continue
            if not member.isfile() or member.issym() or member.islnk():
                raise ValueError("linked runtime archive member")
            with source.extractfile(member) as content:
                copy_archive_file(output.joinpath(*path.parts[1:]), content)


def unpack_wheel(archive, output):
    with zipfile.ZipFile(archive) as source:
        for member in source.infolist():
            path = safe_name(member.filename)
            if member.is_dir():
                continue
            if (member.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError("linked wheel member")
            # Wheel scripts/model test data are not inference dependencies.
            if any(x in {"test", "tests", "__pycache__", "datasets", "tools", "transformers", "quantization"} for x in path.parts):
                continue
            if any(x.endswith(".data") for x in path.parts) or path.suffix in {".pyc", ".onnx"}:
                continue
            with source.open(member) as content:
                copy_archive_file(output.joinpath(*path.parts), content)


def stage_crt(pin, output):
    vswhere = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = subprocess.check_output([str(vswhere), "-latest", "-products", "*", "-property", "installationPath"], text=True).strip()
    root = Path(installation) / "VC/Redist/MSVC" / pin["directory_version"] / "x64/Microsoft.VC143.CRT"
    for name, expected in pin["files"].items():
        source = root / name
        if not source.is_file() or source.is_symlink() or digest(source) != expected:
            raise ValueError("pinned Visual C++ x64 redistributable is required; see tools/wake-release/dependencies.json")
        shutil.copyfile(source, output / name)


def selected_source(path, names, header):
    source = path.read_text(encoding="utf-8")
    tree = ast.parse(source)
    result, found = [header], set()
    for node in tree.body:
        node_names = {node.name} if isinstance(node, (ast.FunctionDef, ast.ClassDef)) else {
            target.id for target in getattr(node, "targets", []) if isinstance(target, ast.Name)}
        if node_names & set(names):
            found.update(node_names & set(names))
            result.append(ast.get_source_segment(source, node))
    if found != set(names):
        raise ValueError("inference source selection changed")
    return "\n\n".join(result) + "\n"


def stage_sources(output, cache, pins):
    for name, expected in SOURCE_PINS.items():
        if digest(SOURCE / name) != expected:
            raise ValueError("owner-accepted detector source changed")
    point = json.loads((SOURCE / "lira-ctc-exact/config.json").read_text(encoding="utf-8"))
    config = json.loads((ROOT / "tools/wake-release/candidate-config.json").read_text(encoding="utf-8"))
    if (digest(SOURCE / "lira-ctc-exact/config.json") != POINT
            or digest(SOURCE / "candidate.py") != config["frontend_sha256"]
            or digest(SOURCE / "lira_ctc.py") != point["source_sha256"]):
        raise ValueError("frozen model provenance changed")
    worker = output / "worker"
    worker.mkdir()
    (worker / "candidate.py").write_text(selected_source(SOURCE / "candidate.py",
        ["RATE", "WINDOW", "STRIDE", "TAIL", "THRESHOLD", "COOLDOWN", "mel_matrix"],
        '"""Inference subset generated verbatim from the frozen Fono detector."""'), encoding="utf-8")
    (worker / "lira_ctc.py").write_text(selected_source(SOURCE / "lira_ctc.py",
        ["TARGET", "whole_phrase", "greedy_text"], '"""Exact CTC decision; no training or transcript output."""'), encoding="utf-8")
    (worker / "live_worker.py").write_text(selected_source(SOURCE / "live_worker.py",
        ["MAX_REJECTED_RETRIES", "FOCUSED_SAMPLES", "classify_contexts", "Stream", "exact_read", "reply"],
        '"""Unmodified streaming detector, with development I/O excluded."""\nimport json\nimport time\nimport candidate as common'), encoding="utf-8")
    shutil.copyfile(SOURCE / "live_runtime.py", worker / "live_runtime.py")
    shutil.copyfile(ROOT / "tools/wake-release/worker.py", worker / "worker.py")
    shutil.copyfile(SOURCE / "lira-live-policy.json", output / "policy.json")
    models = output / "models"
    models.mkdir()
    cnn = ROOT / "tools/wake-release/candidate.onnx"
    if digest(cnn) != CNN:
        raise ValueError("CNN model changed")
    shutil.copyfile(cnn, models / "candidate.onnx")
    archive = fetched(pins["tone"], cache)
    names = {"model.onnx": "tone.onnx", "tokens.txt": "tokens.txt", "LICENSE": "T-one-LICENSE"}
    found = set()
    with tarfile.open(archive, "r:bz2") as source:
        for member in source.getmembers():
            path = safe_name(member.name)
            if len(path.parts) != 2 or path.parts[0] != "sherpa-onnx-streaming-t-one-russian-2025-09-08" or path.name not in names:
                continue
            if not member.isfile() or path.name in found:
                raise ValueError("invalid model member")
            with source.extractfile(member) as content:
                copy_archive_file(models / names[path.name], content)
            if digest(models / names[path.name]) != point["model_identity"][path.name]:
                raise ValueError("T-one model changed")
            found.add(path.name)
    if found != set(names):
        raise ValueError("incomplete model archive")
    shutil.copyfile(ROOT / "LICENSE", worker / "LICENSE-Fono")
    shutil.copyfile(ROOT / "tools/wake-release/APACHE-2.0.txt", models / "APACHE-2.0.txt")
    licenses = output / "licenses"
    licenses.mkdir()
    for name in ["LIBFFI.txt", "ZLIB.txt", "OPENSSL.txt"]:
        shutil.copyfile(ROOT / "tools/wake-release/licenses" / name, licenses / name)
    return point


def prepare(output, cache):
    allowed = ROOT / "src-tauri/resources/lira"
    if output.resolve() != allowed.resolve() or output.is_symlink() or output.is_junction():
        raise ValueError("only the designated release staging directory is allowed")
    output.parent.mkdir(parents=True, exist_ok=True)
    if any(parent.is_symlink() or parent.is_junction() for parent in output.parents if parent != ROOT.parent):
        raise ValueError("linked staging parent")
    if output.exists():
        shutil.rmtree(output)
    output.mkdir()
    cache.mkdir(parents=True, exist_ok=True)
    pins = json.loads(PINS.read_text(encoding="utf-8"))
    unpack_python(fetched(pins["python"], cache), output / "runtime")
    stage_crt(pins["crt"], output / "runtime")
    for pin in pins["wheels"]:
        unpack_wheel(fetched(pin, cache), output / "packages")
    # The DLL-specific path file ignores registry, PYTHONPATH, user site and cwd.
    # No import site, .pth processing, pip or package installation at runtime.
    (output / "runtime/python312._pth").write_text(
        "Lib\nDLLs\n.\n../packages\n../worker\n", encoding="ascii")
    point = stage_sources(output, cache, pins)
    shutil.copyfile(ROOT / "tools/wake-release/THIRD-PARTY-NOTICES.txt", output / "THIRD-PARTY-NOTICES.txt")
    manifest = {"schema": "fono-lira-runtime@1", "protocol": 2, "cnn": CNN,
        "verifier": POINT, "tone": point["model_identity"]["model.onnx"],
        "tokens": point["model_identity"]["tokens.txt"], "detector_sources": SOURCE_PINS,
        "versions": {"python": pins["python"]["version"], **{p["name"]: p["version"] for p in pins["wheels"]}},
        "dependencies_sha256": digest(PINS), "files": []}
    for path in sorted(output.rglob("*")):
        if path.is_file():
            manifest["files"].append({"path": path.relative_to(output).as_posix(), "size": path.stat().st_size, "sha256": digest(path)})
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"files": len(manifest["files"]), "bytes": sum(f["size"] for f in manifest["files"]),
        "manifest_sha256": digest(output / "manifest.json")}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=ROOT / "src-tauri/resources/lira")
    parser.add_argument("--cache", type=Path, default=ROOT / "build/release-dependencies/lira")
    args = parser.parse_args()
    prepare(args.output, args.cache)
