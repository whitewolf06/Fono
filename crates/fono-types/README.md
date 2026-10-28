# fono-types

Shared speech recognition values for Fono Desktop and the planned server. The
crate owns model identities, Whisper artifact metadata, recognition results,
acceleration preferences, transcription request/result DTOs and `CoreError`.
Decoding values are reexports of `fono-stt-protocol`, so workers and applications
use the same Rust types and JSON contract.

Dependencies are Serde, Serde JSON, thiserror and the worker protocol without its
inference feature. There is no application runtime, Tauri, audio device access,
model loading, network or filesystem I/O. The managed quantized Whisper catalog
is embedded from this crate's `resources/` directory.

`SttModelSelection::validate` checks a nonempty path and managed Whisper identity.
The caller must also check non-Whisper identities against its enabled model
registry. Desktop keeps this check in `types::validate_stt_selection`; `Settings`
and its migration remain in the desktop crate.

`AccelerationMode::use_gpu` receives the caller's compiled capabilities. The
shared crate does not infer application features from its own compilation.

`CoreError` carries stable machine codes, a safe authored message and retry
information. Do not put raw source errors, credentials, local paths or transcript
text in its message. Desktop adapts it to the existing protected `{code, params}`
IPC format. The legacy transcription HTTP error stays `{code, message}`; moving
its types does not change the transcription protocol version or worker version.

Run the portable contracts independently:

```sh
cargo test --locked -p fono-types --all-targets
cargo clippy --locked -p fono-types --all-targets -- -D warnings
cargo tree --locked -p fono-types -e normal,build
```

Workspace builds can unify the protocol's inference feature when another member
needs it. The standalone graph above checks the dependency boundary of this
crate. Desktop native tests separately cover settings migration, offline backend
availability and IPC adaptation.
