"""Deterministic, unsigned release preparation. Never signs or executes a worker."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import sys
import tarfile
import tempfile

CUDA_FILES = (
    "cublas64_13.dll",
    "cublasLt64_13.dll",
    "cudart64_13.dll",
    "fono-stt-cuda-worker.exe",
)
NOTICES = (
    "THIRD-PARTY-NOTICES.txt",
    "NVIDIA-CUDA-EULA.txt",
    "NVIDIA-CUDA-third-party.txt",
    "FONO-LICENSE.txt",
    "WHISPER-CPP-LICENSE.txt",
    "DEPENDENCY-NOTICES.txt",
)
MAX_ARCHIVE = 1024**3
MAX_FILE = 1024**3
MAX_TOTAL = 2 * 1024**3


def no_links(path):
    """Check every existing ancestor before reads, mkdirs or writes."""
    if sys.version_info < (3, 12):
        raise ValueError("Python 3.12+ is required for Windows junction-safe paths")
    path = Path(path).absolute()
    for part in (path, *path.parents):
        if part.is_symlink() or (hasattr(part, "is_junction") and part.is_junction()):
            raise ValueError(f"Reparse/symlink path is forbidden: {part}")
    return path


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular_bytes(path, maximum=MAX_FILE):
    path = no_links(path)
    if not path.is_file() or not 0 < path.stat().st_size <= maximum:
        raise ValueError(f"Invalid regular file/size: {path}")
    data = path.read_bytes()
    if not 0 < len(data) <= maximum:
        raise ValueError(f"Invalid file size after read: {path}")
    return data


def checked_output(directory):
    directory = no_links(directory)
    directory.mkdir(parents=True, exist_ok=True)
    return directory


def atomic_bytes(path, data):
    no_links(path)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(data)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def stage_base(source, output):
    source = no_links(source)
    output = checked_output(output)
    if source == output:
        raise ValueError("Base output must differ from raw worker source")
    name = "fono-stt-vulkan-worker.exe"
    unexpected = sorted(item.name for item in output.iterdir() if item.name != name)
    if unexpected:
        raise ValueError(f"Unexpected base files; no files removed: {unexpected}")
    data = regular_bytes(source / name)
    atomic_bytes(output / name, data)
    return {"files": [{"path": name, "size": len(data), "sha256": digest(data)}]}


def prepare(repo, workers, output, revision):
    repo, workers = no_links(repo), no_links(workers)
    if not isinstance(revision, int) or not 0 < revision <= 2**53 - 1:
        raise ValueError("revision must be a positive safe integer")
    version = json.loads(regular_bytes(repo / "package.json", 64 * 1024))["version"]
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version):
        raise ValueError("Invalid exact stable app version")
    protocol_source = regular_bytes(repo / "src-tauri/crates/fono-stt-protocol/src/lib.rs", 256 * 1024).decode()
    matches = re.findall(r"pub const PROTOCOL_VERSION:\s*u16\s*=\s*([0-9]+)\s*;", protocol_source)
    if len(matches) != 1 or not 0 < int(matches[0]) <= 65535:
        raise ValueError("Invalid STT protocol version")
    # Only the known raw build products may occur at this release boundary.
    allowed_source = set(CUDA_FILES) | {"fono-stt-vulkan-worker.exe"}
    for item in workers.iterdir():
        no_links(item)
        if not item.is_file() or item.name not in allowed_source:
            raise ValueError(f"Unexpected worker source entry: {item.name}")
    files = {name: regular_bytes(workers / name) for name in CUDA_FILES}
    inventory = json.loads(regular_bytes(repo / "licenses/native-inventory.json", 256 * 1024))
    if inventory["cudaToolkitVersion"] != "13.3.1":
        raise ValueError("CUDA toolkit provenance differs from the notice version")
    runtime_records = {entry["file"].rsplit("/", 1)[-1]: entry for entry in inventory["files"] if entry["file"].startswith("src-tauri/resources/stt-workers/")}
    for name in CUDA_FILES:
        if name.endswith(".dll"):
            record = runtime_records.get(name)
            if not record or record["bytes"] != len(files[name]) or record["sha256"] != digest(files[name]):
                raise ValueError(f"CUDA runtime differs from reviewed toolkit inventory: {name}")
    notice_sources = {
        "NVIDIA-CUDA-EULA.txt": "licenses/upstream/NVIDIA-CUDA-13.3.1-EULA.txt",
        "NVIDIA-CUDA-third-party.txt": "licenses/upstream/NVIDIA-CUDA-13.3.1-third-party.txt",
        "FONO-LICENSE.txt": "LICENSE",
        "WHISPER-CPP-LICENSE.txt": "src-tauri/vendor/whisper.cpp/LICENSE",
        "DEPENDENCY-NOTICES.txt": "licenses/dependency-notices.txt",
    }
    upstream = json.loads(regular_bytes(repo / "licenses/upstream-sources.json", 256 * 1024))
    pinned = {item["file"]: item["sha256"] for item in upstream["sources"]}
    for name, path in notice_sources.items():
        data = regular_bytes(repo / path, 16 * 1024 * 1024)
        if path.startswith("licenses/upstream/") and digest(data) != pinned.get(path):
            raise ValueError(f"License provenance hash differs: {path}")
        files[name] = data
    files["THIRD-PARTY-NOTICES.txt"] = (
        "Fono optional CUDA package\n"
        f"App {version}; STT protocol {matches[0]}; Windows x86_64; CUDA Toolkit 13.3.1.\n\n"
        "Fono worker: MIT; see FONO-LICENSE.txt.\n"
        "whisper.cpp/ggml: MIT; see WHISPER-CPP-LICENSE.txt.\n"
        "Rust/native dependency notices: DEPENDENCY-NOTICES.txt. This complete\n"
        "source inventory also contains unrelated app/build dependencies.\n"
        "NVIDIA CUDA/cuBLAS runtime: NVIDIA CUDA Toolkit EULA; see NVIDIA-CUDA-EULA.txt.\n"
        "NVIDIA third-party attributions: NVIDIA-CUDA-third-party.txt.\n"
        "CUDA runtime is not relicensed under Fono's MIT license.\n"
        "These files are the original notices; preparing this archive does not publish it.\n"
    ).encode()
    if sum(map(len, files.values())) > MAX_TOTAL:
        raise ValueError("Unpacked CUDA package exceeds limit")
    output = checked_output(output)
    no_links(output / "fono-cuda-windows-x86_64.json.sig")
    if (output / "fono-cuda-windows-x86_64.json.sig").exists():
        raise ValueError("Refusing to overwrite a signed output; use a fresh unsigned directory")
    archive_name = f"fono-cuda-{version}-windows-x86_64.tar.bz2"
    archive_path = output / archive_name
    no_links(archive_path)
    # File bytes are snapshotted once, then both hashes and archive use the
    # same immutable bytes. Names, metadata and order are deterministic.
    archive_bytes = io.BytesIO()
    with tarfile.open(fileobj=archive_bytes, mode="w:bz2", format=tarfile.USTAR_FORMAT) as archive:
        for name, data in sorted(files.items()):
            item = tarfile.TarInfo(name)
            item.size, item.mtime, item.mode = len(data), 0, 0o644
            item.uid = item.gid = 0
            item.uname = item.gname = ""
            archive.addfile(item, io.BytesIO(data))
    data = archive_bytes.getvalue()
    if len(data) > MAX_ARCHIVE:
        raise ValueError("CUDA archive exceeds 1 GiB")
    manifest = {
        "schema_version": 1,
        "purpose": "fono-cuda",
        "app_version": version,
        "protocol_version": int(matches[0]),
        "platform": "windows-x86_64",
        "revision": revision,
        "archive": {"name": archive_name, "size": len(data), "sha256": digest(data)},
        "files": [{"path": name, "size": len(content), "sha256": digest(content)} for name, content in sorted(files.items())],
        "notices": list(NOTICES),
    }
    manifest_bytes = (json.dumps(manifest, indent=2, ensure_ascii=True) + "\n").encode()
    if len(manifest_bytes) > 64 * 1024:
        raise ValueError("Manifest exceeds 64 KiB")
    atomic_bytes(archive_path, data)
    atomic_bytes(output / "fono-cuda-windows-x86_64.json", manifest_bytes)
    return manifest


def verify_archive(manifest_path):
    manifest_path = no_links(manifest_path)
    no_links(Path(str(manifest_path) + ".sig"))
    manifest = json.loads(regular_bytes(manifest_path, 64 * 1024))
    version = manifest.get("app_version", "")
    archive_name = f"fono-cuda-{version}-windows-x86_64.tar.bz2"
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) or manifest["archive"]["name"] != archive_name:
        raise ValueError("Invalid archive asset name")
    required = set(CUDA_FILES) | set(NOTICES)
    file_records = manifest["files"]
    if len(file_records) != len(required) or {entry["path"] for entry in file_records} != required:
        raise ValueError("Unexpected signed file allowlist")
    records = {entry["path"]: entry for entry in file_records}
    archive_path = no_links(manifest_path.parent / archive_name)
    # Verify one open file handle, then rewind that same handle for inspection.
    # No extraction or executable/worker launch occurs during packaging.
    with archive_path.open("rb") as stream:
        archive_digest = hashlib.sha256()
        size = 0
        while chunk := stream.read(1024 * 1024):
            size += len(chunk)
            if size > MAX_ARCHIVE:
                raise ValueError("CUDA archive exceeds size limit")
            archive_digest.update(chunk)
        if size != manifest["archive"]["size"] or archive_digest.hexdigest() != manifest["archive"]["sha256"]:
            raise ValueError("Archive hash/size differs from signed manifest")
        stream.seek(0)
        seen, total = set(), 0
        with tarfile.open(fileobj=stream, mode="r:bz2") as archive:
            for member in archive:
                if not member.isfile() or member.name not in required or member.name in seen:
                    raise ValueError(f"Unsafe/unexpected archive member: {member.name}")
                expected = records[member.name]
                if not 0 < member.size <= MAX_FILE or member.size != expected["size"]:
                    raise ValueError("Archive member size differs")
                seen.add(member.name)
                total += member.size
                if total > MAX_TOTAL:
                    raise ValueError("Archive unpacked size exceeds limit")
                file_digest = hashlib.sha256()
                extracted = archive.extractfile(member)
                while chunk := extracted.read(1024 * 1024):
                    file_digest.update(chunk)
                if file_digest.hexdigest() != expected["sha256"]:
                    raise ValueError(f"Archive member hash differs: {member.name}")
        if seen != required:
            raise ValueError("Missing archive members")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    base = commands.add_parser("stage-base")
    base.add_argument("--workers", type=Path, required=True)
    base.add_argument("--output", type=Path, required=True)
    package = commands.add_parser("prepare")
    package.add_argument("--repo", type=Path, default=Path(__file__).resolve().parent.parent)
    package.add_argument("--workers", type=Path, required=True)
    package.add_argument("--output", type=Path, required=True)
    package.add_argument("--revision", type=int, default=1)
    verify = commands.add_parser("verify")
    verify.add_argument("--manifest", type=Path, required=True)
    arguments = parser.parse_args()
    if arguments.command == "stage-base":
        result = stage_base(arguments.workers, arguments.output)
    elif arguments.command == "prepare":
        result = prepare(arguments.repo, arguments.workers, arguments.output, arguments.revision)
    else:
        result = verify_archive(arguments.manifest)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
