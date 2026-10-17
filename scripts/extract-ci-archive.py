"""Validate and unpack the pinned CI Sherpa archive using Python's stdlib."""

import argparse
import hashlib
import io
import pathlib
import re
import shutil
import stat
import sys
import tarfile
import tempfile
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


def is_reparse(entry_stat):
    return bool(getattr(entry_stat, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT)


def verify_cached_files(package, members, destination):
    """Validate real bytes against the pinned archive, without modifying a live cache."""
    root_stat = destination.lstat()
    if not stat.S_ISDIR(root_stat.st_mode) or is_reparse(root_stat):
        raise ValueError("Sherpa cache must be an ordinary directory, without links.")
    expected = {}
    for member in members:
        relative = pathlib.PurePosixPath(member.name.rstrip("/"))
        expected[relative.as_posix().casefold()] = member.isfile()
        for parent in relative.parents:
            if str(parent) != ".":
                expected[parent.as_posix().casefold()] = False
    actual = {}
    pending = [destination]
    while pending:
        directory = pending.pop()
        for entry in directory.iterdir():
            entry_stat = entry.lstat()
            if is_reparse(entry_stat) or not (
                stat.S_ISDIR(entry_stat.st_mode) or stat.S_ISREG(entry_stat.st_mode)
            ):
                raise ValueError("Sherpa cache contains a link or special entry.")
            name = entry.relative_to(destination).as_posix().casefold()
            is_file = stat.S_ISREG(entry_stat.st_mode)
            if name in actual or name not in expected or expected[name] != is_file:
                raise ValueError("Sherpa cache contains an unexpected entry.")
            actual[name] = entry
            if not is_file:
                pending.append(entry)
    if actual.keys() != expected.keys():
        raise ValueError("Sherpa cache is incomplete.")
    for member in members:
        if not member.isfile():
            continue
        entry = actual[member.name.casefold()]
        if entry.stat().st_size != member.size:
            raise ValueError("Sherpa cache file size differs from the pinned archive.")
        with package.extractfile(member) as source, entry.open("rb") as cached:
            trusted_hash = hashlib.file_digest(source, "sha256").digest()
            cached_hash = hashlib.file_digest(cached, "sha256").digest()
        if trusted_hash != cached_hash:
            raise ValueError("Sherpa cache SHA-256 differs from the pinned archive.")


def publish_cache(staging, destination):
    # Windows scanners can briefly hold a newly extracted DLL without delete
    # sharing. Keep publication atomic and bounded; never fall back to copying.
    deadline = time.monotonic() + 5
    while True:
        if destination.exists() or destination.is_symlink():
            raise FileExistsError("Sherpa cache appeared during initialization; refusing to replace it.")
        try:
            staging.rename(destination)
            return
        except PermissionError as error:
            remaining = deadline - time.monotonic()
            if sys.platform != "win32" or getattr(error, "winerror", None) not in (5, 32, 33) or remaining <= 0:
                raise
            time.sleep(min(0.1, remaining))


def initialize_cache(package, members, destination):
    """Publish a complete cache atomically; the caller serializes its final path."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    intended_parent = destination.parent.resolve(strict=True)
    staging = pathlib.Path(tempfile.mkdtemp(prefix=".sherpa-staging-", dir=intended_parent))
    published = False
    try:
        package.extractall(staging, members=members, filter="data")
        verify_cached_files(package, members, staging)
        publish_cache(staging, destination)
        published = True
    finally:
        if not published:
            # Only this invocation's unique staging directory belongs to us.
            # A killed helper can leave a sibling behind; never discover or
            # remove those siblings, which may still have a live writer.
            try:
                if staging.parent.resolve() != intended_parent:
                    raise ValueError("Refusing to clean staging outside its intended parent.")
                staging_stat = staging.lstat()
                if not stat.S_ISDIR(staging_stat.st_mode) or is_reparse(staging_stat):
                    raise ValueError("Refusing to clean linked staging.")
                shutil.rmtree(staging)
            except (OSError, ValueError) as error:
                print(f"Owned Sherpa staging cleanup deferred: {error}", file=sys.stderr, flush=True)


def extract_archive(archive, destination, expected_root, expected_sha256, reuse_existing=False):
    require_supported_python()
    if not re.fullmatch(r"[A-Za-z0-9_-][A-Za-z0-9._-]*", expected_root):
        raise ValueError("Invalid expected archive directory.")
    if not re.fullmatch(r"[0-9a-f]{64}", expected_sha256):
        raise ValueError("Expected SHA-256 must be pinned.")
    with archive.open("rb") as source:
        archive_bytes = source.read(MAX_ARCHIVE_BYTES + 1)
    if len(archive_bytes) > MAX_ARCHIVE_BYTES:
        raise ValueError("Compressed archive exceeds its size limit.")
    actual_hash = hashlib.sha256(archive_bytes).hexdigest()
    if actual_hash != expected_sha256:
        raise ValueError("Archive SHA-256 mismatch.")
    destination_exists = destination.exists() or destination.is_symlink()
    if destination_exists and not reuse_existing:
        raise ValueError("Archive destination must be a new directory.")
    print("Inspecting verified Sherpa archive headers.", flush=True)
    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:bz2") as package:
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
        if destination_exists:
            verify_cached_files(package, members, destination)
            print("Existing Sherpa cache verified against every pinned archive file; reusing.", flush=True)
        else:
            print(f"Validated {len(members)} members, {expanded_bytes} expanded bytes; extracting.", flush=True)
            initialize_cache(package, members, destination)
            print("Verified Sherpa archive extracted.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=pathlib.Path)
    parser.add_argument("--destination", required=True, type=pathlib.Path)
    parser.add_argument("--root", required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--reuse", action="store_true", help="Reuse an existing cache only after verifying all contents.")
    arguments = parser.parse_args()
    print(f"Python {sys.version.split()[0]}: {sys.executable}", flush=True)
    started = time.perf_counter()
    extract_archive(arguments.archive, arguments.destination, arguments.root, arguments.sha256, arguments.reuse)
    print(f"Archive validation and extraction finished in {time.perf_counter() - started:.2f}s.", flush=True)


if __name__ == "__main__":
    main()
