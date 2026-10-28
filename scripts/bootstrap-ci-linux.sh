#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
: "${GITHUB_ENV:?Set GITHUB_ENV to the CI environment file.}"

spec_values="$(python3 - "$repo_root/scripts/ci-native-dependencies.json" <<'PY'
import json
import re
import sys

try:
    with open(sys.argv[1], encoding="utf-8") as source:
        spec = json.load(source)["sherpaLinux"]
    version = spec["version"]
    if not isinstance(version, str) or not re.fullmatch(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)", version):
        raise ValueError("invalid Sherpa version")
    directory = f"sherpa-onnx-v{version}-linux-x64-shared-lib"
    archive_name = f"{directory}.tar.bz2"
    expected_url = f"https://github.com/k2-fsa/sherpa-onnx/releases/download/v{version}/{archive_name}"
    if spec["directory"] != directory or spec["url"] != expected_url:
        raise ValueError("expected the exact official Linux x64 Sherpa release URL and directory")
    digest = spec["sha256"]
    if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", digest):
        raise ValueError("SHA-256 must contain 64 hexadecimal characters")
    timeout = spec["downloadTimeoutSeconds"]
    if type(timeout) is not int or not 1 <= timeout <= 600:
        raise ValueError("download timeout must be an integer from 1 to 600 seconds")
except (OSError, KeyError, TypeError, ValueError) as error:
    raise SystemExit(f"Invalid Linux dependency manifest: {error}")

print("\n".join((version, expected_url, digest.lower(), str(timeout), archive_name)))
PY
)"
mapfile -t spec <<< "$spec_values"

download_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
if [[ "$download_root" == *$'\n'* || "$download_root" == *$'\r'* ]]; then
  echo 'The temporary directory path must not contain line breaks.' >&2
  exit 1
fi
archive_dir="$(mktemp -d "$download_root/fono-sherpa-linux.XXXXXXXX")"
archive_name="${spec[4]}"
partial="$archive_dir/$archive_name.partial"
archive="$archive_dir/$archive_name"

cleanup() {
  local result=$?
  if ((result != 0)); then
    rm -f -- "$partial"
    rmdir -- "$archive_dir" 2>/dev/null || true
  fi
}
trap cleanup EXIT

curl --fail --location --show-error \
  --proto '=https' --proto-redir '=https' \
  --connect-timeout 20 --max-time "${spec[3]}" \
  --retry 2 --retry-delay 1 --retry-max-time "${spec[3]}" \
  --output "$partial" "${spec[1]}"

if ! (cd -- "$archive_dir" && printf '%s  %s\n' "${spec[2]}" "$archive_name.partial" | sha256sum --check --status); then
  echo 'Sherpa Linux archive SHA-256 mismatch.' >&2
  exit 1
fi
mv -- "$partial" "$archive"
printf 'SHERPA_ONNX_ARCHIVE_DIR=%s\n' "$archive_dir" >> "$GITHUB_ENV"
printf 'Verified Sherpa %s Linux x64 archive.\n' "${spec[0]}"
