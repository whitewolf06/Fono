# Dependency and supply-chain policy

`src-tauri/Cargo.lock` and `src-tauri/rust-toolchain.toml` are release inputs and
must be reviewed in the same change as dependency updates.

Before merging a Rust dependency update, run from `src-tauri`:

```powershell
cargo fmt --all --check
cargo test --workspace --target-dir target-codex-review
cargo clippy --workspace --all-targets --target-dir target-codex-review -- -D warnings
cargo deny --config deny.toml check
```

The audit targets the shipped Windows triple (`x86_64-pc-windows-msvc`). Workspace
crates are marked `publish = false` and ignored only for their own missing
license field; every third-party crate is still checked against the allowlist.

Run the documented feature matrix from `docs/testing.md` as well. Review new
licenses, duplicate major versions, build scripts, native libraries, advisories,
and changes to bundled worker/DLL manifests. An advisory exception must name an
owner, explain why the affected code is unreachable or mitigated, and include an
expiry date in `deny.toml`.

Do not regenerate `Cargo.lock` merely to reduce diff noise. Release builds must
use `--locked`; dependency changes are accepted only after a clean-target build
and the desktop/installer qualification appropriate to the release.

Current audit status (2026-08-11): licenses, bans and sources pass for the
Windows release target. Advisories remain blocked by `RUSTSEC-2025-0075`,
`RUSTSEC-2025-0080`, `RUSTSEC-2025-0081`, `RUSTSEC-2025-0098` and
`RUSTSEC-2025-0100`, all through `tauri-utils → urlpattern → unic-*` and all
reported without a safe upgrade. Do not add a blanket ignore: an exception
requires a named owner, mitigation rationale and expiry, or the Tauri chain
must be upgraded.
