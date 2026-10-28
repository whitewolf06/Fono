use clap::{Arg, ArgAction, Command};

fn config_argument() -> Arg {
    Arg::new("config")
        .long("config")
        .value_name("PATH")
        .global(true)
        .help("Server TOML path (default: FONO_CONFIG or /etc/fono/server.toml)")
}

pub fn command() -> Command {
    Command::new("fono-server")
        .about("Fono speech recognition server")
        .disable_version_flag(true)
        .arg(config_argument())
        .arg(
            Arg::new("product-version")
                .long("version")
                .short('V')
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Show product, planned API and worker protocol versions"),
        )
        .subcommand(Command::new("serve").about("Start the HTTP server"))
        .subcommand(
            Command::new("models")
                .about("Manage server speech models")
                .subcommand_required(true)
                .subcommand(Command::new("list").about("List server models"))
                .subcommand(
                    Command::new("pull")
                        .about("Download and verify a model")
                        .arg(Arg::new("id").required(true).value_name("ID")),
                )
                .subcommand(
                    Command::new("remove")
                        .about("Remove an installed model")
                        .arg(Arg::new("id").required(true).value_name("ID")),
                )
                .subcommand(Command::new("verify").about("Verify installed models")),
        )
        .subcommand(
            Command::new("token")
                .about("Manage server access tokens")
                .subcommand_required(true)
                .subcommand(
                    Command::new("create")
                        .about("Create an access token")
                        .arg(Arg::new("name").required(true).value_name("NAME"))
                        .arg(
                            Arg::new("scope")
                                .long("scope")
                                .value_name("SCOPE")
                                .value_parser(["recognize", "admin"])
                                .default_value("recognize"),
                        ),
                )
                .subcommand(Command::new("list").about("List access tokens"))
                .subcommand(
                    Command::new("revoke")
                        .about("Revoke an access token")
                        .arg(Arg::new("name").required(true).value_name("NAME")),
                ),
        )
        .subcommand(Command::new("doctor").about("Check server readiness"))
        .subcommand(
            Command::new("config")
                .about("Inspect server configuration")
                .subcommand_required(true)
                .subcommand(Command::new("check").about("Validate a TOML configuration file")),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn root_help_lists_every_command_family() {
        let help = command()
            .try_get_matches_from(["fono-server", "--help"])
            .unwrap_err();
        assert_eq!(help.kind(), ErrorKind::DisplayHelp);
        let text = help.to_string();
        for name in ["serve", "models", "token", "doctor", "config", "--version"] {
            assert!(text.contains(name), "missing {name} in help");
        }
    }

    #[test]
    fn nested_help_lists_model_and_token_actions() {
        for (group, expected) in [
            ("models", ["list", "pull", "remove", "verify"]),
            ("token", ["create", "list", "revoke", "--help"]),
        ] {
            let help = command()
                .try_get_matches_from(["fono-server", group, "--help"])
                .unwrap_err();
            assert_eq!(help.kind(), ErrorKind::DisplayHelp);
            let text = help.to_string();
            for name in expected {
                assert!(text.contains(name), "missing {name} in {group} help");
            }
        }
        let create_help = command()
            .try_get_matches_from(["fono-server", "token", "create", "--help"])
            .unwrap_err();
        assert!(create_help.to_string().contains("--scope"));
    }

    #[test]
    fn token_scope_accepts_only_supported_values() {
        assert!(command()
            .try_get_matches_from([
                "fono-server",
                "token",
                "create",
                "client",
                "--scope",
                "admin"
            ])
            .is_ok());
        assert!(command()
            .try_get_matches_from([
                "fono-server",
                "token",
                "create",
                "client",
                "--scope",
                "write"
            ])
            .is_err());
    }
}
