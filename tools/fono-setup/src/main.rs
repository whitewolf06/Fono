#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod install;
#[cfg(windows)]
mod ui;

fn main() {
    let arguments = std::env::args().skip(1).collect();
    match fono_setup::cli::run(arguments) {
        Ok(Some(code)) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
        Ok(None) => {}
    }

    #[cfg(windows)]
    if let Err(error) = ui::run() {
        ui::show_startup_error(&error.to_string());
        std::process::exit(1);
    }

    #[cfg(not(windows))]
    {
        eprintln!("Установщик Fono работает только в Windows.");
        std::process::exit(1);
    }
}
