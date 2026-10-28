mod cli;
mod config;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn selected_config_path(explicit: Option<&str>, environment: Option<OsString>) -> PathBuf {
    explicit
        .map(PathBuf::from)
        .or_else(|| environment.map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/etc/fono/server.toml"))
}

fn unavailable(feature: &str) -> ExitCode {
    eprintln!("{feature} is not available in this build");
    ExitCode::from(2)
}

fn check_config(path: &Path) -> ExitCode {
    match config::load_and_validate_with_env(path, std::env::vars_os()) {
        Ok(_) => {
            println!("Configuration valid");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Configuration error: {error}");
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
        println!("api_version: 2 (planned; HTTP server unavailable)");
        println!("worker_protocol: {}", fono_stt_protocol::PROTOCOL_VERSION);
        return ExitCode::SUCCESS;
    }
    match matches.subcommand() {
        Some(("serve", _)) => unavailable("Server runtime"),
        Some(("models", _)) => unavailable("Model management"),
        Some(("token", _)) => unavailable("Token management"),
        Some(("doctor", _)) => unavailable("Server diagnostics"),
        Some(("config", matches)) => match matches.subcommand() {
            Some(("check", check)) => {
                let path = selected_config_path(
                    check.get_one::<String>("config").map(String::as_str),
                    std::env::var_os("FONO_CONFIG"),
                );
                check_config(&path)
            }
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
