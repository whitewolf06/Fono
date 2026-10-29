mod cli;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use clap::ArgMatches;
use fono_server::{config, models, runtime, tokens};
use fono_service::rest_api::TokenScope;
use fono_stt::models::download::{DownloadEvent, DownloadPhase};

fn selected_config_path(explicit: Option<&str>, environment: Option<OsString>) -> PathBuf {
    explicit
        .map(PathBuf::from)
        .or_else(|| environment.map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/etc/fono/server.toml"))
}

fn load_config(path: &Path) -> Result<config::ServerConfig, ExitCode> {
    match config::load_and_validate_with_env(path, std::env::vars_os()) {
        Ok(config) => match models::validate_configuration(&config) {
            Ok(()) => Ok(config),
            Err(error) => {
                eprintln!("Configuration error: {error}");
                Err(ExitCode::FAILURE)
            }
        },
        Err(error) => {
            eprintln!("Configuration error: {error}");
            Err(ExitCode::FAILURE)
        }
    }
}

fn check_config(path: &Path) -> ExitCode {
    match load_config(path) {
        Ok(_) => {
            println!("Configuration valid");
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

fn serve(path: &Path) -> ExitCode {
    let config = match load_config(path) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Server runtime error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(runtime::serve(config));
    // A timed-out spawn_blocking task must not make dropping Tokio wait past
    // the server's shutdown deadline. Successful shutdown joins its work first.
    runtime.shutdown_background();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Server error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_models(path: &Path, matches: &ArgMatches) -> ExitCode {
    let config = match load_config(path) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let result = match matches.subcommand() {
        Some(("list", _)) => models::list(&config).map(|rows| {
            for row in rows {
                println!("{}\t{}\t{}", row.id, row.state.as_str(), row.bytes);
            }
            ExitCode::SUCCESS
        }),
        Some(("verify", _)) => models::verify(&config).map(|rows| {
            let required: std::collections::HashSet<_> =
                std::iter::once(config.models.default.as_str())
                    .chain(config.models.preload.iter().map(String::as_str))
                    .collect();
            let mut failed = false;
            let mut shown = false;
            for row in rows {
                let needed = required.contains(row.id.as_str());
                if row.state != models::DiskState::Missing || needed {
                    if let Some(detail) = row.detail {
                        println!("{}\t{}\t{detail}", row.id, row.state.as_str());
                    } else {
                        println!("{}\t{}", row.id, row.state.as_str());
                    }
                    shown = true;
                }
                if row.state == models::DiskState::Invalid
                    || (needed && row.state != models::DiskState::Verified)
                {
                    failed = true;
                }
            }
            if !shown {
                println!("No installed models");
            }
            if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }),
        Some(("pull", arguments)) => {
            let id = arguments
                .get_one::<String>("id")
                .expect("clap requires a model ID");
            let last = Mutex::new((None::<DownloadPhase>, 0_u64));
            let progress = |event: DownloadEvent| {
                let mut last = last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if last.0 != Some(event.phase)
                    || event.downloaded_bytes.saturating_sub(last.1) >= 16 * 1024 * 1024
                {
                    if let Some(total) = event.total_bytes {
                        eprintln!(
                            "{}: {:?} {}/{} bytes",
                            event.download_id, event.phase, event.downloaded_bytes, total
                        );
                    } else {
                        eprintln!(
                            "{}: {:?} {} bytes",
                            event.download_id, event.phase, event.downloaded_bytes
                        );
                    }
                    *last = (Some(event.phase), event.downloaded_bytes);
                }
            };
            models::pull(&config, id, &progress).map(|()| {
                println!("Installed {id}");
                ExitCode::SUCCESS
            })
        }
        Some(("remove", arguments)) => {
            let id = arguments
                .get_one::<String>("id")
                .expect("clap requires a model ID");
            models::remove(&config, id).map(|()| {
                println!("Removed {id}");
                ExitCode::SUCCESS
            })
        }
        _ => return ExitCode::from(2),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Model error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_tokens(path: &Path, matches: &ArgMatches) -> ExitCode {
    let config = match load_config(path) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let file = &config.server.tokens_file;
    let result = match matches.subcommand() {
        Some(("create", arguments)) => {
            let name = arguments
                .get_one::<String>("name")
                .expect("clap requires a client name");
            let scope = match arguments.get_one::<String>("scope").map(String::as_str) {
                Some("admin") => TokenScope::Admin,
                _ => TokenScope::Recognize,
            };
            tokens::create(file, name, scope).map(|secret| {
                // The token is returned exactly once, after the verified file update.
                println!("{secret}");
                ExitCode::SUCCESS
            })
        }
        Some(("list", _)) => tokens::list(file).map(|entries| {
            for entry in entries {
                let scope = match entry.scope {
                    TokenScope::Recognize => "recognize",
                    TokenScope::Admin => "admin",
                };
                println!("{}\t{scope}\t{}", entry.name, entry.created_at);
            }
            ExitCode::SUCCESS
        }),
        Some(("revoke", arguments)) => {
            let name = arguments
                .get_one::<String>("name")
                .expect("clap requires a client name");
            tokens::revoke(file, name).map(|()| {
                println!("Revoked {name}");
                ExitCode::SUCCESS
            })
        }
        _ => return ExitCode::from(2),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Token error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let matches = match cli::command().try_get_matches() {
        Ok(matches) => matches,
        Err(error) => error.exit(),
    };
    if matches.get_flag("product-version") {
        println!("fono-server {}", env!("FONO_PRODUCT_VERSION"));
        println!("api_version: {}", runtime::SERVER_API_VERSION);
        println!("worker_protocol: {}", fono_stt_protocol::PROTOCOL_VERSION);
        return ExitCode::SUCCESS;
    }
    let config_path = selected_config_path(
        matches.get_one::<String>("config").map(String::as_str),
        std::env::var_os("FONO_CONFIG"),
    );
    match matches.subcommand() {
        Some(("serve", _)) => serve(&config_path),
        Some(("models", arguments)) => run_models(&config_path, arguments),
        Some(("token", arguments)) => run_tokens(&config_path, arguments),
        Some(("doctor", _)) => match load_config(&config_path) {
            Ok(config) => {
                let report = fono_server::doctor::inspect(&config);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .expect("diagnostic report is serializable")
                );
                if report.ready {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(code) => code,
        },
        Some(("config", matches)) => match matches.subcommand() {
            Some(("check", _)) => check_config(&config_path),
            _ => ExitCode::from(2),
        },
        _ => {
            let mut command = cli::command();
            let _ = command.print_help();
            eprintln!();
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_config_path_wins_over_environment_and_default() {
        assert_eq!(
            selected_config_path(
                Some("selected.toml"),
                Some(OsString::from("environment.toml"))
            ),
            PathBuf::from("selected.toml")
        );
        assert_eq!(
            selected_config_path(None, Some(OsString::from("environment.toml"))),
            PathBuf::from("environment.toml")
        );
        assert_eq!(
            selected_config_path(None, None),
            PathBuf::from("/etc/fono/server.toml")
        );
    }
}
