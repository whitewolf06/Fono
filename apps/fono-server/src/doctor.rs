//! Preflight diagnostics. Temporary permission probes are removed immediately;
//! native recognition runs in a bounded, isolated child without an HTTP server.

use std::collections::HashSet;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::{backends, config::ServerConfig, models, tokens};

mod filesystem;
mod probe;
#[cfg(test)]
mod tests;
mod tls;

pub use probe::{run_native_probe, ProbeReply, ProbeResult};

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub skip_self_test: bool,
    pub self_test_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            skip_self_test: false,
            self_test_timeout: Duration::from_secs(120),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub required: bool,
    pub ok: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct ModelCheck {
    pub id: String,
    pub required: bool,
    pub state: &'static str,
}

#[derive(Debug, Serialize)]
pub struct WorkerCheck {
    pub device: String,
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct Storage {
    pub area: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    pub recommended_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub product_version: &'static str,
    pub ready: bool,
    pub cuda_build: bool,
    pub devices: Vec<backends::CudaDevice>,
    pub models: Vec<ModelCheck>,
    pub workers: Vec<WorkerCheck>,
    pub storage: Vec<Storage>,
    pub self_test: ProbeResult,
    pub checks: Vec<Check>,
}

impl Report {
    fn new() -> Self {
        Self {
            product_version: env!("FONO_PRODUCT_VERSION"),
            ready: false,
            cuda_build: backends::cuda_enabled(),
            devices: Vec::new(),
            models: Vec::new(),
            workers: Vec::new(),
            storage: Vec::new(),
            self_test: ProbeResult::skipped(),
            checks: Vec::new(),
        }
    }

    fn check(
        &mut self,
        name: &'static str,
        required: bool,
        ok: bool,
        detail: impl Into<String>,
        remediation: &'static str,
    ) {
        self.checks.push(Check {
            name,
            required,
            ok,
            detail: detail.into(),
            remediation: (!ok).then_some(remediation),
        });
    }

    fn required_checks_pass(&self) -> bool {
        self.checks.iter().all(|check| check.ok || !check.required)
    }
}

pub fn inspect(config: &ServerConfig, config_path: &Path, options: Options) -> Report {
    let mut report = inspect_preflight(config);
    if options.skip_self_test {
        report.check(
            "self_test",
            false,
            false,
            "Recognition probe skipped by request",
            "Run doctor without --skip-self-test to verify native recognition.",
        );
    } else if !report.required_checks_pass() {
        report.check(
            "self_test",
            true,
            false,
            "Recognition probe skipped after failed preflight checks",
            "Resolve failed required checks and run doctor again.",
        );
    } else {
        report.self_test = probe::run_child(config, config_path, options.self_test_timeout);
        report.check("self_test", true, report.self_test.status == "passed",
            report.self_test.detail,
            "Check the selected model/backend and available memory; increase --self-test-timeout-seconds only for a known slow model.");
    }
    report.ready = report.required_checks_pass();
    report
}

fn inspect_preflight(config: &ServerConfig) -> Report {
    let mut report = Report::new();
    let runtime_valid = crate::runtime::required_preload(config).is_ok();
    report.check(
        "model_configuration",
        true,
        runtime_valid,
        if runtime_valid {
            "Model startup policy is valid"
        } else {
            "Model startup policy is invalid"
        },
        "Keep the default model resident and fit default/preload entries within models.max_loaded.",
    );
    match tokens::list(&config.server.tokens_file) {
        Ok(entries) => report.check("tokens", true, !entries.is_empty(),
            format!("{} active credentials", entries.len()),
            "Create a credential with fono-server token create <name> using a one-off writable credential mount."),
        Err(error) => report.check("tokens", true, false, error.to_string(),
            "Check server.tokens_file, its ownership and mode 0600, or create a credential with the token CLI."),
    }
    let required: HashSet<_> = std::iter::once(config.models.default.as_str())
        .chain(config.models.preload.iter().map(String::as_str))
        .collect();
    match models::verify(config) {
        Ok(rows) => {
            let required_ok = rows.iter().all(|row| {
                !required.contains(row.id.as_str()) || row.state == models::DiskState::Verified
            });
            let optional_ok = rows.iter().all(|row| {
                required.contains(row.id.as_str()) || row.state != models::DiskState::Invalid
            });
            let verified = rows
                .iter()
                .filter(|row| row.state == models::DiskState::Verified)
                .count();
            report.models = rows
                .into_iter()
                .filter(|row| {
                    required.contains(row.id.as_str()) || row.state != models::DiskState::Missing
                })
                .map(|row| ModelCheck {
                    required: required.contains(row.id.as_str()),
                    id: row.id,
                    state: row.state.as_str(),
                })
                .collect();
            report.check("model_hashes", true, required_ok, format!("{verified} verified installed models"),
                "Install or repair each missing/invalid required model with fono-server models pull <id>.");
            report.check(
                "optional_model_hashes",
                false,
                optional_ok,
                if optional_ok {
                    "Optional model artifacts are valid or absent"
                } else {
                    "An optional model artifact is invalid"
                },
                "Repair the invalid optional model with the model CLI before selecting it.",
            );
        }
        Err(_) => report.check(
            "model_hashes",
            true,
            false,
            "Cannot verify the model store",
            "Check models.dir permissions and run fono-server models verify.",
        ),
    }
    filesystem::inspect(config, &mut report);
    inspect_network(config, &mut report);
    tls::inspect(config, &mut report);
    inspect_workers(config, &mut report);
    report
}

fn inspect_network(config: &ServerConfig, report: &mut Report) {
    let loopback = config.server.bind.ip().is_loopback();
    report.check("network_policy", true,
        loopback || (config.server.trusted_private_network && !config.server.allowed_hosts.is_empty()),
        "Non-loopback access requires a trusted private network and explicit hosts",
        "Set an explicit allowed_hosts list and trusted_private_network only for a protected network.");
    report.check("network_exposure", false, loopback || config.tls.is_some(),
        if loopback { "Listener is restricted to loopback" } else if config.tls.is_some() { "Listener is configured for TLS" } else { "Plain HTTP is reachable beyond loopback" },
        "Restrict the published interface and firewall; use an encrypted tunnel or TLS reverse proxy.");
    let (ok, detail, hint) = match TcpListener::bind(config.server.bind) {
        Ok(listener) => { drop(listener); (true, "Configured listener can bind", "") }
        Err(error) => match error.kind() {
            std::io::ErrorKind::AddrInUse => (false, "Configured port is already in use", "Stop the existing listener or select a free server.bind; use /v1/health to inspect an already running service."),
            std::io::ErrorKind::AddrNotAvailable => (false, "Configured interface is unavailable", "Select an IP assigned to this host in server.bind."),
            std::io::ErrorKind::PermissionDenied => (false, "Listener bind permission denied", "Choose an unprivileged port or grant the service explicit bind permission."),
            _ => (false, "Cannot bind the configured listener", "Check server.bind, address family support and host network policy."),
        },
    };
    report.check("listener", true, ok, detail, hint);
}

fn inspect_workers(config: &ServerConfig, report: &mut Report) {
    let requested = backends::configured_cuda_devices(config);
    match backends::cuda_devices() {
        Ok(devices) => {
            let ok = requested
                .iter()
                .all(|index| devices.iter().any(|device| device.index == *index));
            let detail = if report.cuda_build {
                format!(
                    "{} NVIDIA devices observed through driver inventory",
                    devices.len()
                )
            } else {
                "NVIDIA inventory is not enabled in this CPU build".into()
            };
            report.check("cuda_devices", !requested.is_empty(), ok, detail,
                "Check the NVIDIA driver and CDI device mapping; select a visible cuda:N ordinal in models.placement.");
            report.devices = devices;
        }
        Err(error) => report.check(
            "cuda_devices",
            !requested.is_empty(),
            false,
            error,
            "Check nvidia-smi, the NVIDIA driver and container CDI device mapping.",
        ),
    }
    let selections: Vec<_> = if requested.is_empty() {
        vec![None]
    } else {
        requested.iter().copied().map(Some).collect()
    };
    for selection in selections {
        let result = backends::installed_worker().and_then(|package| {
            let package = selection.map_or_else(
                || package.clone(),
                |index| package.clone().with_cuda_device(index),
            );
            fono_stt::probe_cuda_worker(package, Arc::new(fono_stt::NoopActivity))
                .map_err(|_| "CUDA worker handshake failed".to_owned())
        });
        report.workers.push(WorkerCheck {
            device: selection.map_or_else(|| "cuda".into(), |index| format!("cuda:{index}")),
            ok: result.is_ok(),
        });
    }
    let ok = report.workers.iter().all(|worker| worker.ok);
    report.check(
        "cuda_worker",
        !requested.is_empty(),
        ok,
        if ok {
            format!(
                "Protocol {} handshake verified for {} worker selections",
                fono_stt_protocol::PROTOCOL_VERSION,
                report.workers.len()
            )
        } else {
            "CUDA worker installation or protocol handshake is unavailable".into()
        },
        "Use the matching CUDA image/worker and expose the NVIDIA driver libraries through CDI.",
    );
}
