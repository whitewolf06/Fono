# Dependency and supply-chain policy

`src-tauri/Cargo.lock` and `src-tauri/rust-toolchain.toml` are release inputs and
must be reviewed in the same change as dependency updates.

Before merging a Rust dependency update, bootstrap the verified native runtime
from the repository root in PowerShell 7:

```powershell
./scripts/bootstrap-ci-resources.ps1
```

Then run from `src-tauri`:

```powershell
cargo fmt --all --check
../scripts/test-native.ps1 -CargoArguments @('--locked', '--workspace', '--all-targets', '--target-dir', 'target-codex-review')
cargo clippy --locked --workspace --all-targets --target-dir target-codex-review -- -D warnings
cargo deny --config deny.toml check
```

The audit targets the shipped Windows triple (`x86_64-pc-windows-msvc`). Workspace
crates are marked `publish = false` and ignored only for their own missing
license field; every third-party crate is still checked against the allowlist.

Run the feature matrix from [testing.md](testing.md) as well. Review new
licenses, duplicate major versions, build scripts, native libraries, advisories,
and changes to bundled worker/DLL manifests. An advisory exception must name an
owner, explain why the affected code is unreachable or mitigated, and include an
expiry date in `deny.toml`.

Do not regenerate `Cargo.lock` merely to reduce diff noise. Release builds must
use `--locked`; dependency changes are accepted only after a clean-target build
and the desktop/installer qualification appropriate to the release.

Record the reviewed revision, tool versions, audit date and each gate's result
with the dependency change. Advisory data changes independently of the lockfile;
a previous successful audit does not establish the current status. A missing
`cargo-deny`, unavailable advisory database or skipped command is an unperformed
check, not a pass. Follow each current advisory to the affected dependency chain
and a safe upgrade or scoped mitigation. Do not add a blanket ignore; exceptions
must follow the owner, rationale and expiry requirements above.

Historical follow-up (2026-08-11): the advisory gate reported
`RUSTSEC-2025-0075`, `RUSTSEC-2025-0080`, `RUSTSEC-2025-0081`,
`RUSTSEC-2025-0098` and `RUSTSEC-2025-0100` through
`tauri-utils → urlpattern → unic-*`. These findings need a fresh audit of the
current lockfile before closing the follow-up; documentation cleanup alone does
not resolve them or establish their current status.
