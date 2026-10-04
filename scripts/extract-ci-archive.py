"""Validate and unpack the pinned CI Sherpa archive using Python's stdlib."""

import argparse
import hashlib
import pathlib
import re
import sys
import tarfile
import time

MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
MAX_EXPANDED_BYTES = 256 * 1024 * 1024
MAX_MEMBERS = 256
WINDOWS_RESERVED = {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"} | {
    f"{prefix}{number}"
    for prefix in ("COM", "LPT")
    for number in (*range(1, 10), "¹", "²", "³")
}


def require_supported_python():
    if sys.version_info < (3, 12):
        raise RuntimeError("CI archive extraction requires Python 3.12 or newer.")


def validate_member(member, expected_root):
    if member.sparse is not None:
        raise ValueError("Archive contains a sparse entry.")
    if member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE):
        raise ValueError("Archive contains a link or special entry.")
    name = member.name
    if "\\" in name or ":" in name or name.startswith("/"):
        raise ValueError("Archive contains an unsafe Windows path.")
    parts = name.rstrip("/").split("/")
    if not parts or parts[0] != expected_root:
        raise ValueError("Archive entry is outside the expected directory.")
    for part in parts:
        if not part or part in (".", "..") or part.endswith((".", " ")):
            raise ValueError("Archive contains an unsafe path component.")
        if part.split(".", 1)[0].upper() in WINDOWS_RESERVED:
            raise ValueError("Archive contains a reserved Windows name.")
        if any(character in '<>"|?*' for character in part):
            raise ValueError("Archive contains an invalid Windows filename.")
        if any(ord(character) < 32 for character in part):
            raise ValueError("Archive contains a control character in a path.")
    if member.isfile() and (name.endswith("/") or len(parts) == 1):
        raise ValueError("Archive file conflicts with the expected directory.")
    if member.size < 0 or (member.isdir() and member.size != 0):
        raise ValueError("Archive contains an invalid entry size.")
    return "/".join(parts).casefold()


def extract_archive(archive, destination, expected_root, expected_sha256):
    require_supported_python()
    if not re.fullmatch(r"[A-Za-z0-9_-][A-Za-z0-9._-]*", expected_root):
        raise ValueError("Invalid expected archive directory.")
    if not re.fullmatch(r"[0-9a-f]{64}", expected_sha256):
        raise ValueError("Expected SHA-256 must be pinned.")
    if archive.stat().st_size > MAX_ARCHIVE_BYTES:
        raise ValueError("Compressed archive exceeds its size limit.")
    with archive.open("rb") as source:
        actual_hash = hashlib.file_digest(source, "sha256").hexdigest()
    if actual_hash != expected_sha256:
        raise ValueError("Archive SHA-256 mismatch.")
    if destination.exists() or destination.is_symlink():
        raise ValueError("Archive destination must be a new directory.")
    print("Inspecting verified Sherpa archive headers.", flush=True)
    with tarfile.open(archive, "r:bz2") as package:
        members = []
        paths = set()
        file_paths = set()
        expanded_bytes = 0
        # Inspect each header before the iterator skips its body.
        for member in package:
            if len(members) >= MAX_MEMBERS:
                raise ValueError("Archive exceeds its member limit.")
            path = validate_member(member, expected_root)
            if path in paths:
                raise ValueError("Archive contains a duplicate path.")
            paths.add(path)
            if member.isfile():
                file_paths.add(path)
            expanded_bytes += member.size
            if expanded_bytes > MAX_EXPANDED_BYTES:
                raise ValueError("Archive exceeds its expanded size limit.")
            members.append(member)
        if not members:
            raise ValueError("Archive is empty.")
        for path in paths:
            for parent in pathlib.PurePosixPath(path).parents:
                parent_name = str(parent)
                if parent_name == ".":
                    break
                if parent_name in file_paths:
                    raise ValueError("Archive file is used as a parent directory.")
        print(f"Validated {len(members)} members, {expanded_bytes} expanded bytes; extracting.", flush=True)
        destination.mkdir(parents=True, exist_ok=False)
        package.extractall(destination, members=members, filter="data")
    print("Verified Sherpa archive extracted.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=pathlib.Path)
    parser.add_argument("--destination", required=True, type=pathlib.Path)
    parser.add_argument("--root", required=True)
    parser.add_argument("--sha256", required=True)
    arguments = parser.parse_args()
    print(f"Python {sys.version.split()[0]}: {sys.executable}", flush=True)
    started = time.perf_counter()
    extract_archive(arguments.archive, arguments.destination, arguments.root, arguments.sha256)
    print(f"Archive validation and extraction finished in {time.perf_counter() - started:.2f}s.", flush=True)


if __name__ == "__main__":
    main()
