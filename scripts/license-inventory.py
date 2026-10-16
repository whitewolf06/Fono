"""Offline lockfile/license inventory; does not resolve, build or download packages.

Requires Python 3.11+. --write collects locally available upstream notice files;
--check verifies the saved inventory covers the exact current lockfiles.
Missing package sources/notices are evidence gaps, never a license grant.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tomllib

LOCKFILES = ("package-lock.json", "src-tauri/Cargo.lock", "tools/fono-setup/Cargo.lock")
NOTICE_NAME = re.compile(r"^(?:licen[sc]e|copying|notice|copyright|unlicense)(?:[._-].*)?$", re.I)
SKIP_DIRECTORIES = {".git", "node_modules", "target", "__pycache__"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def notice_files(directory):
    if not directory.is_dir():
        return []
    result = []
    for current, dirs, files in os.walk(directory, followlinks=False):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRECTORIES and not (Path(current) / d).is_symlink())
        for name in sorted(files):
            file = Path(current) / name
            if NOTICE_NAME.match(name) and not file.is_symlink():
                result.append(file)
    return result


def collect(directory, texts):
    result = []
    for file in notice_files(directory):
        content = file.read_bytes()
        sha = digest(content)
        # Keep complete bytes decoded as UTF-8; foreign encodings remain visible
        # through replacement characters and are reported for manual correction.
        text = content.decode("utf-8", errors="replace")
        texts[sha] = text
        entry = {"file": file.relative_to(directory).as_posix(), "sha256": sha}
        if "\ufffd" in text:
            entry["encodingReviewRequired"] = True
        result.append(entry)
    return result


def crate_source(registry, name, version):
    candidates = sorted(registry.glob(f"index.crates.io-*/{name}-{version}")) if registry.is_dir() else []
    return candidates[0] if candidates else None


def inventory(root, cargo_home):
    root = root.resolve()
    texts = {}
    npm = []
    issues = []
    lock = json.loads((root / LOCKFILES[0]).read_text(encoding="utf-8"))
    for package_path, package in sorted(lock["packages"].items()):
        if not package_path:
            continue
        directory = (root / package_path).resolve()
        if not directory.is_relative_to(root / "node_modules"):
            raise ValueError("npm lock package path must remain inside node_modules")
        entry = {
            "path": package_path,
            "version": package.get("version"),
            "declaredLicense": package.get("license"),
            "dev": package.get("dev", False),
            "optional": package.get("optional", False),
            "integrity": package.get("integrity"),
            "notices": [],
        }
        for platform_field in ("os", "cpu"):
            if platform_field in package:
                entry[platform_field] = package[platform_field]
        manifest_path = directory / "package.json"
        if manifest_path.is_file():
            installed = json.loads(manifest_path.read_text(encoding="utf-8"))
            if installed.get("version") != entry["version"]:
                entry["sourceStatus"] = "installed-version-mismatch"
            else:
                entry["sourceStatus"] = "available"
                entry["installedLicense"] = installed.get("license")
                entry["notices"] = collect(directory, texts)
                if installed.get("license") != entry["declaredLicense"]:
                    issues.append({"package": package_path, "issue": "npm-license-declaration-mismatch"})
        else:
            entry["sourceStatus"] = "not-installed"
        if not entry["notices"]:
            issues.append({"package": package_path, "issue": "no-collected-license-text", "sourceStatus": entry["sourceStatus"]})
        npm.append(entry)

    cargo = {}
    for lock_path in LOCKFILES[1:]:
        lock_data = tomllib.loads((root / lock_path).read_text(encoding="utf-8"))
        for package in lock_data["package"]:
            if not package.get("source"):
                continue  # First-party/path workspaces are governed separately.
            key = (package["name"], package["version"], package["source"])
            if key in cargo:
                cargo[key]["lockfiles"].append(lock_path)
                continue
            entry = {k: package[k] for k in ("name", "version", "source", "checksum") if k in package}
            entry["lockfiles"] = [lock_path]
            entry["notices"] = []
            directory = None
            if package["source"] in ("registry+https://github.com/rust-lang/crates.io-index", "registry+https://index.crates.io/"):
                directory = crate_source(cargo_home / "registry/src", package["name"], package["version"])
            if directory:
                manifest = tomllib.loads((directory / "Cargo.toml").read_text(encoding="utf-8"))["package"]
                entry["sourceStatus"] = "available"
                entry["declaredLicense"] = manifest.get("license")
                if manifest.get("license-file"):
                    entry["declaredLicenseFile"] = manifest["license-file"]
                entry["notices"] = collect(directory, texts)
            else:
                entry["sourceStatus"] = "not-cached"
                entry["declaredLicense"] = None
            if not entry["declaredLicense"]:
                issues.append({"package": f"{key[0]}@{key[1]}", "issue": "no-license-declaration-in-local-source"})
            if not entry["notices"]:
                issues.append({"package": f"{key[0]}@{key[1]}", "issue": "no-collected-license-text", "sourceStatus": entry["sourceStatus"]})
            cargo[key] = entry

    vendored = [{
        "path": "src-tauri/vendor/whisper.cpp",
        "source": "https://github.com/ggml-org/whisper.cpp",
        "notices": collect(root / "src-tauri/vendor/whisper.cpp", texts),
    }]
    result = {
        "schemaVersion": 1,
        "scope": "All registry packages in the three locks, including build/dev/optional and unselected targets; not a linked-binary SBOM or legal approval. Third-party native/model notices are listed separately in THIRD_PARTY_NOTICES.md.",
        "inputs": [{"file": path, "sha256": digest((root / path).read_bytes())} for path in LOCKFILES],
        "npm": npm,
        "cargo": [cargo[key] for key in sorted(cargo)],
        "vendored": vendored,
        "issues": issues,
    }
    notices = "Collected upstream notice files (complete text, deduplicated by original byte SHA-256).\nSee dependency-inventory.json for component/file mapping and evidence gaps.\n"
    for sha, text in sorted(texts.items()):
        notices += f"\n{'=' * 72}\nSHA-256: {sha}\n{'=' * 72}\n{text}"
        if not text.endswith("\n"):
            notices += "\n"
    return result, notices


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--cargo-home", type=Path, default=Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")))
    args = parser.parse_args()
    target = args.root / "licenses/dependency-inventory.json"
    if args.check:
        saved = json.loads(target.read_text(encoding="utf-8"))
        if [item.get("file") for item in saved.get("inputs", [])] != list(LOCKFILES):
            sys.exit("License inventory must cover all three lockfiles.")
        for item in saved["inputs"]:
            if digest((args.root / item["file"]).read_bytes()) != item["sha256"]:
                sys.exit(f"License inventory is stale for {item['file']}; regenerate with --write.")
        print("License inventory matches all three lockfiles; recorded evidence gaps still require review.")
        return
    result, notices = inventory(args.root, args.cargo_home)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    (target.parent / "dependency-notices.txt").write_text(notices, encoding="utf-8", newline="\n")
    print(f"Inventoried {len(result['npm'])} npm and {len(result['cargo'])} Cargo registry packages; {len(result['issues'])} evidence gaps (see inventory issues).")


if __name__ == "__main__":
    main()
