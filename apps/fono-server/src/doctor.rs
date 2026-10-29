//! Read-only installation checks. This command never starts the HTTP service.

use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;

use crate::{backends, config::ServerConfig, models, tokens};

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub required: bool,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub ready: bool,
    pub cuda_build: bool,
    pub devices: Vec<backends::CudaDevice>,
    pub checks: Vec<Check>,
}

pub fn inspect(config: &ServerConfig) -> Report {
    let mut checks = Vec::new();
    let mut add = |name, required, ok, detail| {
        checks.push(Check {
            name,
            required,
            ok,
            detail,
        })
    };
    match tokens::list(&config.server.tokens_file) {
        Ok(entries) => add(
            "tokens",
            true,
            !entries.is_empty(),
            format!("{} active credentials", entries.len()),
        ),
        Err(error) => add("tokens", true, false, error.to_string()),
    }
    let required: HashSet<_> = std::iter::once(config.models.default.as_str())
        .chain(config.models.preload.iter().map(String::as_str))
        .collect();
    match models::verify(config) {
        Ok(rows) => {
            let verified = rows
                .iter()
                .filter(|row| row.state == models::DiskState::Verified)
                .count();
            let ok = rows.iter().all(|row| {
                row.state != models::DiskState::Invalid
                    && (!required.contains(row.id.as_str())
                        || row.state == models::DiskState::Verified)
            });
            add(
                "model_hashes",
                true,
                ok,
                format!("{verified} verified models"),
            );
        }
        Err(_) => add(
            "model_hashes",
            true,
            false,
            "Cannot verify the model store".into(),
        ),
    }
    let required_cuda = backends::configured_cuda_devices(config);
    let devices = match backends::cuda_devices() {
        Ok(devices) => {
            let ok = required_cuda
                .iter()
                .all(|index| devices.iter().any(|device| device.index == *index));
            add(
                "cuda_devices",
                !required_cuda.is_empty(),
                ok,
                format!("{} visible NVIDIA devices", devices.len()),
            );
            devices
        }
        Err(error) => {
            add("cuda_devices", !required_cuda.is_empty(), false, error);
            Vec::new()
        }
    };
    if backends::cuda_enabled() {
        match backends::probe_worker(Arc::new(fono_stt::NoopActivity)) {
            Ok(()) => add(
                "cuda_worker",
                !required_cuda.is_empty(),
                true,
                format!(
                    "Protocol {} handshake verified",
                    fono_stt_protocol::PROTOCOL_VERSION
                ),
            ),
            Err(error) => add("cuda_worker", !required_cuda.is_empty(), false, error),
        }
    } else {
        add(
            "cuda_worker",
            !required_cuda.is_empty(),
            false,
            "CUDA worker support is unavailable in this build".into(),
        );
    }
    let ready = checks.iter().all(|check| check.ok || !check.required);
    Report {
        ready,
        cuda_build: backends::cuda_enabled(),
        devices,
        checks,
    }
}
