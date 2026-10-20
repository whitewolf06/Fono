use std::ffi::OsStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Start,
    Stop,
    Toggle,
    Cancel,
    Show,
}

const USAGE: &str = "Укажите одну команду: --start, --stop, --toggle, --cancel или --show.";

pub(super) fn parse<I, T>(arguments: I) -> Result<Command, &'static str>
where
    I: IntoIterator<Item = T>,
    T: AsRef<OsStr>,
{
    let mut arguments = arguments.into_iter();
    let Some(argument) = arguments.next() else {
        return Ok(Command::Show);
    };
    let command =
        match argument.as_ref().to_str() {
            Some("--start") => Command::Start,
            Some("--stop") => Command::Stop,
            Some("--toggle") => Command::Toggle,
            Some("--cancel") => Command::Cancel,
            Some("--show") => Command::Show,
            _ => return Err(
                "Неизвестная команда Fono. Допустимы --start, --stop, --toggle, --cancel и --show.",
            ),
        };
    if arguments.next().is_some() {
        return Err(USAGE);
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_launch_and_exactly_one_known_flag_are_supported() {
        assert_eq!(parse::<_, &str>([]).unwrap(), Command::Show);
        for (flag, command) in [
            ("--start", Command::Start),
            ("--stop", Command::Stop),
            ("--toggle", Command::Toggle),
            ("--cancel", Command::Cancel),
            ("--show", Command::Show),
        ] {
            assert_eq!(parse([flag]).unwrap(), command);
        }
    }

    #[test]
    fn unknown_empty_values_and_multiple_commands_fail_without_echoing_input() {
        for arguments in [
            vec!["--private-secret-value"],
            vec![""],
            vec!["file.wav"],
            vec!["--start=1"],
            vec!["--START"],
            vec!["--start", "--stop"],
            vec!["--show", "--show"],
        ] {
            let error = parse(arguments).unwrap_err();
            assert!(!error.contains("private-secret-value"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn non_unicode_argument_is_rejected_before_plugin_uses_env_args() {
        use std::os::windows::ffi::OsStringExt;
        let bad = std::ffi::OsString::from_wide(&[0xD800]);
        assert!(parse([bad]).is_err());
    }
}
