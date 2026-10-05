use crate::{config, verify_installer, Bootstrapper, Phase};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

const HELP: &str = "FonoSetup — онлайн-установщик Fono для Windows.
Без аргументов открывает окно установки.

Диагностика без установки и открытия окон:
  --check-only
  --download-only [--cache-dir <absolute-path>]
  --verify-package <path> --signature-file <path> --version <X.Y.Z>
  --help

Проверка и загрузка используют встроенный публичный канал Fono.
--download-only проверяет подпись и сохраняет пакет, но не запускает его.";

pub fn run(arguments: Vec<String>) -> Result<Option<i32>, String> {
    if arguments.is_empty() {
        return Ok(None);
    }
    if arguments.len() == 1 && matches!(arguments[0].as_str(), "--help" | "-h" | "/?") {
        println!("{HELP}");
        return Ok(Some(0));
    }
    if arguments == ["--check-only"] {
        let bootstrap = Bootstrapper::new(config::embedded_config(), config::default_cache_dir()?)
            .map_err(|error| error.to_string())?;
        let release = bootstrap
            .resolve_manifest(&AtomicBool::new(false))
            .map_err(|error| error.to_string())?;
        println!(
            "{}",
            serde_json::json!({
                "bootstrap_version": config::bootstrap_version(),
                "available_version": release.version,
                "installer_url": release.url,
                "installer_verified": false,
            })
        );
        return Ok(Some(0));
    }
    if arguments[0] == "--download-only" {
        let cache_dir = match arguments.as_slice() {
            [_] => config::default_cache_dir()?,
            [_, option, path] if option == "--cache-dir" => PathBuf::from(path),
            _ => return Err("Используйте --download-only [--cache-dir <absolute-path>].".into()),
        };
        if !cache_dir.is_absolute() {
            return Err("Каталог загрузки должен быть абсолютным путём.".into());
        }
        let bootstrap = Bootstrapper::new(config::embedded_config(), cache_dir)
            .map_err(|error| error.to_string())?;
        let installer = bootstrap
            .run(Arc::new(AtomicBool::new(false)), |progress| {
                if progress.phase != Phase::Downloading {
                    eprintln!("{:?}", progress.phase);
                }
            })
            .map_err(|error| error.to_string())?;
        println!(
            "{}",
            serde_json::json!({
                "bootstrap_version": config::bootstrap_version(),
                "available_version": installer.version,
                "path": installer.path,
                "installer_verified": true,
                "installed": false,
            })
        );
        return Ok(Some(0));
    }
    let fields = verification_fields(&arguments)?;
    let path = PathBuf::from(&fields["--verify-package"]);
    let signature_path = PathBuf::from(&fields["--signature-file"]);
    let size = fs::metadata(&signature_path)
        .map_err(|error| error.to_string())?
        .len();
    if size == 0 || size > 8192 {
        return Err("Неверный размер файла подписи.".into());
    }
    let signature = fs::read_to_string(signature_path).map_err(|error| error.to_string())?;
    verify_installer(
        &path,
        &signature,
        &config::embedded_config().public_key,
        &fields["--version"],
    )
    .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::json!({ "version": fields["--version"], "installer_verified": true, "installed": false })
    );
    Ok(Some(0))
}

fn verification_fields(arguments: &[String]) -> Result<BTreeMap<String, String>, String> {
    let allowed = ["--verify-package", "--signature-file", "--version"];
    if arguments.len() != 6 {
        return Err("Неизвестные аргументы. Используйте --help.".into());
    }
    let mut fields = BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        if !allowed.contains(&pair[0].as_str())
            || pair[1].is_empty()
            || fields.insert(pair[0].clone(), pair[1].clone()).is_some()
        {
            return Err("Неверные или повторяющиеся параметры проверки.".into());
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_mixed_arguments_cannot_fall_through_to_installation() {
        for arguments in [
            vec!["--silent"],
            vec!["--check-only", "--install"],
            vec!["--download-only", "--cache-dir", "relative"],
            vec![
                "--verify-package",
                "file",
                "--version",
                "0.6.12",
                "--version",
                "0.6.12",
            ],
        ] {
            assert!(run(arguments.into_iter().map(str::to_owned).collect()).is_err());
        }
    }

    #[test]
    fn only_an_empty_command_line_opens_the_interactive_installer() {
        assert_eq!(run(Vec::new()).unwrap(), None);
        assert_eq!(run(vec!["--help".into()]).unwrap(), Some(0));
    }
}
