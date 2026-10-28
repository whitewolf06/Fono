//! Side-effect-free server configuration parsing and validation.
//!
//! Runtime checks (installed models, tokens, TLS files and devices) belong to
//! the commands that use them. This module only reads the requested TOML file.

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use toml::{Table, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub server: ServerSettings,
    pub tls: Option<TlsSettings>,
    pub models: ModelSettings,
    pub queue: QueueSettings,
    pub metrics: MetricsSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSettings {
    pub bind: SocketAddr,
    pub tokens_file: PathBuf,
    pub max_upload_mb: usize,
    pub max_audio_seconds: u64,
    pub shutdown_grace: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsSettings {
    pub cert: PathBuf,
    pub key: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSettings {
    pub dir: PathBuf,
    pub default: String,
    pub preload: Vec<String>,
    pub max_loaded: usize,
    pub placement: Vec<ModelPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPlacement {
    pub model: String,
    pub device: String,
    pub residency: ModelResidency,
    pub idle_unload_after: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelResidency {
    Resident,
    IdleUnload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSettings {
    pub max_queued: usize,
    pub max_inflight_per_token: usize,
    pub cpu_parallel: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsSettings {
    pub enabled: bool,
}

/// The field path is safe to print. Messages never contain the rejected value,
/// which may have come from a secret-bearing configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub path: String,
    pub message: String,
}

impl ConfigError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ConfigError {}

/// Read and validate a config file without writing files, loading models or
/// contacting the network. Path selection and environment overrides are host
/// concerns; SRV-02 will apply the latter before semantic validation.
pub fn load_and_validate(path: &Path) -> Result<ServerConfig, ConfigError> {
    let source = std::fs::read_to_string(path)
        .map_err(|_| ConfigError::new("config", "cannot read configuration file"))?;
    parse_and_validate(&source)
}

fn parse_and_validate(source: &str) -> Result<ServerConfig, ConfigError> {
    let root: Table = toml::from_str(source).map_err(|error: toml::de::Error| {
        ConfigError::new(
            syntax_error_path(source, error.span()),
            "invalid TOML syntax",
        )
    })?;
    known_keys(&root, &["server", "tls", "models", "queue", "metrics"], "")?;

    let server = section(&root, "server")?;
    if let Some(table) = server {
        known_keys(
            table,
            &[
                "bind",
                "tokens_file",
                "max_upload_mb",
                "max_audio_seconds",
                "shutdown_grace",
            ],
            "server",
        )?;
    }
    let bind =
        match string_or(server, "bind", "server.bind", "127.0.0.1:17832")?.parse::<SocketAddr>() {
            Ok(address) => address,
            Err(_) => {
                return Err(ConfigError::new(
                    "server.bind",
                    "expected an IP address and port",
                ))
            }
        };
    let tokens_file = path_or(
        server,
        "tokens_file",
        "server.tokens_file",
        "/etc/fono/tokens",
    )?;
    let max_upload_mb = positive_usize_or(server, "max_upload_mb", "server.max_upload_mb", 100)?;
    if max_upload_mb.checked_mul(1024 * 1024).is_none() {
        return Err(ConfigError::new(
            "server.max_upload_mb",
            "upload byte limit is too large",
        ));
    }
    let max_audio_seconds =
        positive_u64_or(server, "max_audio_seconds", "server.max_audio_seconds", 300)?;
    let shutdown_grace = duration_or(server, "shutdown_grace", "server.shutdown_grace", "10s")?;

    let tls_table = section(&root, "tls")?;
    let tls = if let Some(table) = tls_table {
        known_keys(table, &["cert", "key"], "tls")?;
        let cert = optional_path(table, "cert", "tls.cert")?;
        let key = optional_path(table, "key", "tls.key")?;
        match (cert, key) {
            (Some(cert), Some(key)) => Some(TlsSettings { cert, key }),
            (Some(_), None) => {
                return Err(ConfigError::new("tls.key", "required when tls.cert is set"))
            }
            (None, Some(_)) => {
                return Err(ConfigError::new("tls.cert", "required when tls.key is set"))
            }
            (None, None) => None,
        }
    } else {
        None
    };

    let models = section(&root, "models")?;
    if let Some(table) = models {
        known_keys(
            table,
            &["dir", "default", "preload", "max_loaded", "placement"],
            "models",
        )?;
    }
    let default = string_or(models, "default", "models.default", "whisper:large_turbo")?;
    let preload = string_list_or(models, "preload", "models.preload", &[default.as_str()])?;
    let max_loaded = positive_usize_or(models, "max_loaded", "models.max_loaded", 2)?;
    if preload.len() > max_loaded {
        return Err(ConfigError::new(
            "models.preload",
            "preload count exceeds models.max_loaded",
        ));
    }
    let placement = model_placements(models)?;
    let models = ModelSettings {
        dir: path_or(models, "dir", "models.dir", "/var/lib/fono/models")?,
        default,
        preload,
        max_loaded,
        placement,
    };

    let queue = section(&root, "queue")?;
    if let Some(table) = queue {
        known_keys(
            table,
            &["max_queued", "max_inflight_per_token", "cpu_parallel"],
            "queue",
        )?;
    }
    let queue = QueueSettings {
        max_queued: positive_usize_or(queue, "max_queued", "queue.max_queued", 32)?,
        max_inflight_per_token: positive_usize_or(
            queue,
            "max_inflight_per_token",
            "queue.max_inflight_per_token",
            2,
        )?,
        cpu_parallel: positive_usize_or(queue, "cpu_parallel", "queue.cpu_parallel", 2)?,
    };

    let metrics = section(&root, "metrics")?;
    if let Some(table) = metrics {
        known_keys(table, &["enabled"], "metrics")?;
    }
    let enabled = match metrics.and_then(|table| table.get("enabled")) {
        None => false,
        Some(Value::Boolean(enabled)) => *enabled,
        Some(_) => return Err(ConfigError::new("metrics.enabled", "expected a boolean")),
    };

    Ok(ServerConfig {
        server: ServerSettings {
            bind,
            tokens_file,
            max_upload_mb,
            max_audio_seconds,
            shutdown_grace,
        },
        tls,
        models,
        queue,
        metrics: MetricsSettings { enabled },
    })
}

/// Infer only known field names from the parser's source span. Never include
/// arbitrary source text in an error: configuration values may contain secrets.
fn syntax_error_path(source: &str, span: Option<std::ops::Range<usize>>) -> String {
    let Some(offset) = span.map(|range| range.start.min(source.len())) else {
        return "config".to_owned();
    };
    let Some(before_error) = source.get(..offset) else {
        return "config".to_owned();
    };
    let line_start = before_error.rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[offset..]
        .find('\n')
        .map_or(source.len(), |index| offset + index);
    let Some(line) = source.get(line_start..line_end) else {
        return "config".to_owned();
    };
    let Some((key, _)) = line.split_once('=') else {
        return "config".to_owned();
    };
    let key = key.trim();
    let mut section = "";
    let mut placement_index = None;
    let mut placement_count = 0;
    for prior_line in source[..line_start].lines() {
        match prior_line.trim() {
            "[server]" => section = "server",
            "[tls]" => section = "tls",
            "[models]" => section = "models",
            "[queue]" => section = "queue",
            "[metrics]" => section = "metrics",
            "[[models.placement]]" => {
                section = "models.placement";
                placement_index = Some(placement_count);
                placement_count += 1;
            }
            header if header.starts_with('[') => section = "",
            _ => {}
        }
    }
    let known = match section {
        "server" => matches!(
            key,
            "bind" | "tokens_file" | "max_upload_mb" | "max_audio_seconds" | "shutdown_grace"
        ),
        "tls" => matches!(key, "cert" | "key"),
        "models" => matches!(
            key,
            "dir" | "default" | "preload" | "max_loaded" | "placement"
        ),
        "models.placement" => {
            matches!(key, "model" | "device" | "residency" | "idle_unload_after")
        }
        "queue" => matches!(
            key,
            "max_queued" | "max_inflight_per_token" | "cpu_parallel"
        ),
        "metrics" => key == "enabled",
        _ => false,
    };
    if !known {
        return "config".to_owned();
    }
    if section == "models.placement" {
        return format!("models.placement[{}].{key}", placement_index.unwrap_or(0));
    }
    format!("{section}.{key}")
}

fn section<'a>(root: &'a Table, name: &str) -> Result<Option<&'a Table>, ConfigError> {
    match root.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_table()
            .map(Some)
            .ok_or_else(|| ConfigError::new(name, "expected a table")),
    }
}

fn known_keys(table: &Table, allowed: &[&str], prefix: &str) -> Result<(), ConfigError> {
    for key in table.keys() {
        if !allowed.contains(&key.as_str()) {
            let path = if prefix.is_empty() {
                key.to_owned()
            } else {
                format!("{prefix}.{key}")
            };
            return Err(ConfigError::new(path, "unknown configuration field"));
        }
    }
    Ok(())
}

fn string_or(
    table: Option<&Table>,
    key: &str,
    path: &str,
    default: &str,
) -> Result<String, ConfigError> {
    match table.and_then(|table| table.get(key)) {
        None => Ok(default.to_owned()),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.to_owned()),
        Some(_) => Err(ConfigError::new(path, "expected a non-empty string")),
    }
}

fn required_string(table: &Table, key: &str, path: &str) -> Result<String, ConfigError> {
    match table.get(key) {
        None => Err(ConfigError::new(path, "required field is missing")),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.to_owned()),
        Some(_) => Err(ConfigError::new(path, "expected a non-empty string")),
    }
}

fn path_or(
    table: Option<&Table>,
    key: &str,
    path: &str,
    default: &str,
) -> Result<PathBuf, ConfigError> {
    string_or(table, key, path, default).map(PathBuf::from)
}

fn optional_path(table: &Table, key: &str, path: &str) -> Result<Option<PathBuf>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(PathBuf::from(value))),
        Some(_) => Err(ConfigError::new(path, "expected a non-empty path")),
    }
}

fn positive_u64_or(
    table: Option<&Table>,
    key: &str,
    path: &str,
    default: u64,
) -> Result<u64, ConfigError> {
    match table.and_then(|table| table.get(key)) {
        None => Ok(default),
        Some(Value::Integer(value)) => u64::try_from(*value)
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| ConfigError::new(path, "expected a positive integer")),
        Some(_) => Err(ConfigError::new(path, "expected a positive integer")),
    }
}

fn positive_usize_or(
    table: Option<&Table>,
    key: &str,
    path: &str,
    default: usize,
) -> Result<usize, ConfigError> {
    let value = positive_u64_or(table, key, path, default as u64)?;
    usize::try_from(value).map_err(|_| ConfigError::new(path, "integer is too large"))
}

fn duration_or(
    table: Option<&Table>,
    key: &str,
    path: &str,
    default: &str,
) -> Result<Duration, ConfigError> {
    let raw = string_or(table, key, path, default)?;
    parse_duration(&raw, path)
}

fn parse_duration(raw: &str, path: &str) -> Result<Duration, ConfigError> {
    let unit_start = raw
        .find(|character: char| !character.is_ascii_digit())
        .ok_or_else(|| ConfigError::new(path, "expected a positive duration with unit"))?;
    let (number, unit) = raw.split_at(unit_start);
    let multiplier = match unit {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        _ => {
            return Err(ConfigError::new(
                path,
                "expected duration unit ms, s, m or h",
            ))
        }
    };
    let value = number
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .and_then(|value| value.checked_mul(multiplier))
        .ok_or_else(|| ConfigError::new(path, "expected a positive duration in range"))?;
    Ok(Duration::from_millis(value))
}

fn string_list_or(
    table: Option<&Table>,
    key: &str,
    path: &str,
    default: &[&str],
) -> Result<Vec<String>, ConfigError> {
    let Some(value) = table.and_then(|table| table.get(key)) else {
        return Ok(default.iter().map(|value| (*value).to_owned()).collect());
    };
    let list = value
        .as_array()
        .ok_or_else(|| ConfigError::new(path, "expected an array of model identifiers"))?;
    let mut items = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        match item {
            Value::String(value) if !value.trim().is_empty() => {
                if items.iter().any(|existing| existing == value) {
                    return Err(ConfigError::new(
                        format!("{path}[{index}]"),
                        "duplicate model identifier",
                    ));
                }
                items.push(value.to_owned());
            }
            _ => {
                return Err(ConfigError::new(
                    format!("{path}[{index}]"),
                    "expected a non-empty model identifier",
                ))
            }
        }
    }
    Ok(items)
}

fn model_placements(models: Option<&Table>) -> Result<Vec<ModelPlacement>, ConfigError> {
    let Some(value) = models.and_then(|table| table.get("placement")) else {
        return Ok(Vec::new());
    };
    let entries = value
        .as_array()
        .ok_or_else(|| ConfigError::new("models.placement", "expected an array of tables"))?;
    let mut placements = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let prefix = format!("models.placement[{index}]");
        let table = entry
            .as_table()
            .ok_or_else(|| ConfigError::new(&prefix, "expected a table"))?;
        known_keys(
            table,
            &["model", "device", "residency", "idle_unload_after"],
            &prefix,
        )?;
        let model = required_string(table, "model", &format!("{prefix}.model"))?;
        let device = required_string(table, "device", &format!("{prefix}.device"))?;
        if !valid_device(&device) {
            return Err(ConfigError::new(
                format!("{prefix}.device"),
                "expected cpu or cuda:<non-negative integer>",
            ));
        }
        let residency = match table.get("residency") {
            None => ModelResidency::Resident,
            Some(Value::String(value)) if value == "resident" => ModelResidency::Resident,
            Some(Value::String(value)) if value == "idle_unload" => ModelResidency::IdleUnload,
            _ => {
                return Err(ConfigError::new(
                    format!("{prefix}.residency"),
                    "expected resident or idle_unload",
                ))
            }
        };
        let idle_unload_after = match table.get("idle_unload_after") {
            None => None,
            Some(Value::String(value)) => Some(parse_duration(
                value,
                &format!("{prefix}.idle_unload_after"),
            )?),
            Some(_) => {
                return Err(ConfigError::new(
                    format!("{prefix}.idle_unload_after"),
                    "expected a positive duration with unit",
                ))
            }
        };
        match (residency, idle_unload_after) {
            (ModelResidency::IdleUnload, None) => {
                return Err(ConfigError::new(
                    format!("{prefix}.idle_unload_after"),
                    "required for idle_unload residency",
                ))
            }
            (ModelResidency::Resident, Some(_)) => {
                return Err(ConfigError::new(
                    format!("{prefix}.idle_unload_after"),
                    "only valid for idle_unload residency",
                ))
            }
            _ => {}
        }
        if placements
            .iter()
            .any(|placement: &ModelPlacement| placement.model == model)
        {
            return Err(ConfigError::new(
                format!("{prefix}.model"),
                "model already has a placement",
            ));
        }
        placements.push(ModelPlacement {
            model,
            device,
            residency,
            idle_unload_after,
        });
    }
    Ok(placements)
}

fn valid_device(device: &str) -> bool {
    if device == "cpu" {
        return true;
    }
    device.strip_prefix("cuda:").is_some_and(|index| {
        !index.is_empty()
            && index.bytes().all(|digit| digit.is_ascii_digit())
            && index.parse::<u32>().is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn error_path(source: &str) -> String {
        parse_and_validate(source).unwrap_err().path
    }

    #[test]
    fn plan_example_parses_into_typed_settings() {
        let config = parse_and_validate(
            r#"
            [server]
            bind = "127.0.0.1:17832"
            tokens_file = "/etc/fono/tokens"
            max_upload_mb = 100
            max_audio_seconds = 300
            shutdown_grace = "10s"

            [tls]
            cert = "/etc/fono/tls/cert.pem"
            key = "/etc/fono/tls/key.pem"

            [models]
            dir = "/var/lib/fono/models"
            default = "whisper:large_turbo"
            preload = ["whisper:large_turbo"]
            max_loaded = 2

            [[models.placement]]
            model = "whisper:large_turbo"
            device = "cuda:0"
            residency = "resident"

            [[models.placement]]
            model = "giga_am:v3_e2e_rnnt_int8"
            device = "cpu"
            residency = "idle_unload"
            idle_unload_after = "30m"

            [queue]
            max_queued = 32
            max_inflight_per_token = 2
            cpu_parallel = 2

            [metrics]
            enabled = false
            "#,
        )
        .unwrap();
        assert_eq!(
            config.server.bind,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 17832)
        );
        assert_eq!(config.server.shutdown_grace, Duration::from_secs(10));
        assert_eq!(
            config.models.placement[1].idle_unload_after,
            Some(Duration::from_secs(1800))
        );
        assert_eq!(
            config.models.placement[1].residency,
            ModelResidency::IdleUnload
        );
        assert_eq!(
            config.tls.unwrap().cert,
            PathBuf::from("/etc/fono/tls/cert.pem")
        );
    }

    #[test]
    fn absent_optional_settings_use_safe_defaults() {
        let config = parse_and_validate("[server]\n").unwrap();
        assert_eq!(
            config.server.bind,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 17832)
        );
        assert_eq!(config.server.max_upload_mb, 100);
        assert_eq!(config.models.preload, ["whisper:large_turbo"]);
        assert_eq!(config.queue.cpu_parallel, 2);
        assert!(config.tls.is_none());
        assert!(!config.metrics.enabled);
    }

    #[test]
    fn unknown_sections_and_nested_keys_report_full_paths() {
        assert_eq!(error_path("[unexpected]\nvalue = 1"), "unexpected");
        assert_eq!(error_path("[server]\nunknown = 1"), "server.unknown");
        assert_eq!(
            error_path("[[models.placement]]\nmodel = 'x'\ndevice = 'cpu'\nunknown = 1"),
            "models.placement[0].unknown"
        );
    }

    #[test]
    fn invalid_typed_fields_and_limits_report_full_paths() {
        for (source, expected) in [
            ("[server]\nbind = 'localhost:17832'", "server.bind"),
            ("[server]\nmax_upload_mb = 0", "server.max_upload_mb"),
            (
                "[server]\nmax_audio_seconds = -1",
                "server.max_audio_seconds",
            ),
            ("[server]\nshutdown_grace = '0s'", "server.shutdown_grace"),
            ("[queue]\ncpu_parallel = 0", "queue.cpu_parallel"),
            ("[queue]\nmax_queued = 'many'", "queue.max_queued"),
            ("[models]\nmax_loaded = 0", "models.max_loaded"),
            ("[metrics]\nenabled = 'yes'", "metrics.enabled"),
        ] {
            assert_eq!(error_path(source), expected, "{source}");
        }
    }

    #[test]
    fn tls_requires_both_paths() {
        assert_eq!(error_path("[tls]\ncert = '/cert.pem'"), "tls.key");
        assert_eq!(error_path("[tls]\nkey = '/key.pem'"), "tls.cert");
        assert_eq!(error_path("[tls]\ncert = ''"), "tls.cert");
    }

    #[test]
    fn model_placement_and_preload_errors_identify_the_entry() {
        assert_eq!(
            error_path(
                "[[models.placement]]\nmodel = 'x'\ndevice = 'cpu'\nresidency = 'idle_unload'"
            ),
            "models.placement[0].idle_unload_after"
        );
        assert_eq!(
            error_path(
                "[[models.placement]]\nmodel = 'x'\ndevice = 'cpu'\nidle_unload_after = '30m'"
            ),
            "models.placement[0].idle_unload_after"
        );
        assert_eq!(
            error_path("[models]\npreload = ['a', 'b', 'c']"),
            "models.preload"
        );
        assert_eq!(
            error_path("[models]\npreload = ['a', 5]"),
            "models.preload[1]"
        );
        assert_eq!(
            error_path("[models]\npreload = ['a', 'a']"),
            "models.preload[1]"
        );
        assert_eq!(
            error_path("[[models.placement]]\nmodel = 'x'\ndevice = 'cuda:-1'"),
            "models.placement[0].device"
        );
    }

    #[test]
    fn parse_errors_do_not_echo_values() {
        let secret = "secret-token-do-not-print";
        let error = parse_and_validate(&format!("[server]\nbind = '{secret}'")).unwrap_err();
        assert_eq!(error.path, "server.bind");
        assert!(!error.to_string().contains(secret));
        let syntax_error = parse_and_validate(&format!("[server]\nbind = '{secret}")).unwrap_err();
        assert_eq!(syntax_error.path, "server.bind");
        assert!(!syntax_error.to_string().contains(secret));
    }

    #[test]
    fn syntax_errors_identify_only_known_fields() {
        assert_eq!(
            error_path("[queue]\nmax_queued = 'unterminated"),
            "queue.max_queued"
        );
        assert_eq!(
            error_path("[server]\nsecret_token = 'unterminated"),
            "config"
        );
    }
}
