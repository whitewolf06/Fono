# Preparing the optional CUDA package

The approved channel is the existing `whitewolf06/fono` GitHub Releases and
the public key in `src-tauri/update-channel.json`. Preparing local files does
not sign them, publish a release, install a package, or prove GPU operation.

## Base resources

`prepare-release-resources.ps1` builds Vulkan with `-SkipCuda`, then calls
`cuda-package.py stage-base`. The raw build directory
`src-tauri/resources/stt-workers/` can retain CUDA products from another build.
The separate base directory `src-tauri/resources/stt-base/` contains exactly
`fono-stt-vulkan-worker.exe`. Unexpected base entries are rejected without
deleting them.

Tauri maps that exact file to
`resources/stt-workers/fono-stt-vulkan-worker.exe`. There is no worker resource
glob. Sherpa runtime and speech VAD remain in the base config. The CPU-only
Tauri override removes the exact Vulkan resource. `build.rs` validates the
base worker manifest and checks the Vulkan protocol during release builds.
CUDA files in the old raw directory cannot enter the bundle through that map.

The base NSIS size must be measured from a newly built installer. Source file
sizes, `cargo check`, and a browser mock do not establish the installer size.

## Unsigned preparation

Use Python 3.12+ on PATH, with the submodule and workers already prepared:

```powershell
python scripts/cuda-package.py prepare `
  --workers src-tauri/resources/stt-workers `
  --output build/cuda-package --revision 1
python scripts/cuda-package.py verify `
  --manifest build/cuda-package/fono-cuda-windows-x86_64.json
```

The producer snapshots the allowed files, uses sorted flat USTAR members,
fixed zero timestamps and ownership, and bzip2 compression. Identical inputs
produce identical archive and manifest bytes. App version comes from
`package.json`; protocol version comes from `fono-stt-protocol` source.
No environment override changes either version or the trusted channel.

Schema 1 has purpose `fono-cuda`, exact app/protocol versions, platform
`windows-x86_64`, a positive package revision, archive size/SHA-256, and the
exact per-file size/SHA-256 list. The manifest contains an asset name rather
than an arbitrary download URL. Assets for app `X.Y.Z` are:

```text
releases/download/vX.Y.Z/fono-cuda-X.Y.Z-windows-x86_64.tar.bz2
releases/download/vX.Y.Z/fono-cuda-windows-x86_64.json
releases/download/vX.Y.Z/fono-cuda-windows-x86_64.json.sig
```

The archive allowlist is intentionally closed for the current CUDA 13 runtime:

| File                                                      | Source/provenance                                                                                                                               |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `fono-stt-cuda-worker.exe`                                | Locally built `fono-stt-worker`, feature `cuda`; protocol verification is a separate release check                                              |
| `cublas64_13.dll`, `cublasLt64_13.dll`, `cudart64_13.dll` | Prepared CUDA Toolkit 13.3.1 runtime products                                                                                                   |
| `FONO-LICENSE.txt`                                        | Root `LICENSE`, original bytes                                                                                                                  |
| `WHISPER-CPP-LICENSE.txt`                                 | Pinned submodule license, original bytes                                                                                                        |
| `DEPENDENCY-NOTICES.txt`                                  | Complete collected source notices; includes the Rust bindings and bundled whisper.cpp notices, and may include unrelated app/build dependencies |
| `NVIDIA-CUDA-EULA.txt`, `NVIDIA-CUDA-third-party.txt`     | Original toolkit texts in `licenses/upstream/`; hashes checked against `upstream-sources.json`                                                  |
| `THIRD-PARTY-NOTICES.txt`                                 | Generated index identifying the component/license files and the exact app/protocol version                                                      |

The Python verifier inspects every archive member without extracting or
executing it. It rejects extra, missing, duplicate, non-regular or traversal
members, and mismatched archive/file hashes and sizes. Limits are 1 GiB for
the archive or a file, 2 GiB unpacked, and 64 KiB for the manifest.
Changing the CUDA DLL names requires an explicit producer/runtime allowlist
change. Fono's MIT does not replace the NVIDIA terms.

## Explicit signing and publication boundary

Only a release operator with an external private key should invoke:

```powershell
node scripts/sign-cuda-package.mjs `
  --manifest build/cuda-package/fono-cuda-windows-x86_64.json `
  --private-key-path C:/external-secure-location/fono.key
```

The signing wrapper first checks the schema, current app/protocol versions,
archive size/hash and every archive member. It then invokes the installed
official Tauri CLI with `--app-version`. The CLI loads the explicitly supplied
key file; the wrapper never prints key content. The result is verified with
the embedded public key, including the signed trusted version comment.
No publication occurs. The `.sig` signs the exact manifest bytes, whose hash
list authenticates the archive and payload.

Keep reviewed release files immutable after signing. Re-preparation refuses
an output directory containing the manifest signature; use a fresh unsigned
directory instead. Uploading signed assets, actual package installation,
Auto fallback, recovery and clean Windows/GPU acceptance are separate steps.

On Windows the installer service seals the payload directory with a protected
OWNER RIGHTS DACL before promotion. Ordinary ownership permits read/execute,
without adding children or restoring a permissive DACL. Open read guards
protect the signed files, and directory guards protect ancestor renames.
The owner retains DELETE for cleanup after the guards close; the live directory
handle blocks delete/rename while the package is in use.
The cached package layout and seal are checked again before worker creation.
This prevents a later unsigned app-local dependency from entering the payload.
During preparation, directory guards also retain staging parents, the operation
UUID and payload; destination parent guards bridge promotion into verification.
Once leaf guards close for atomic rename, cleanup never follows the vacated UUID.
A failed rename can retain a bounded, unaccepted staging directory for inspection.
Administrator/System recovery and previously granted write/security handles remain
outside this filesystem integrity boundary.

## Focused verification

```powershell
python scripts/cuda-package.test.py
node --test scripts/sign-cuda-package.test.mjs
```

These use inert fixtures and temporary test signing keys. They never execute
a real CUDA worker, access the release private key, or publish an asset.
