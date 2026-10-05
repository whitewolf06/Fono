use crate::Config;
use std::path::PathBuf;

pub fn embedded_config() -> Config {
    Config {
        endpoint: env!("FONO_SETUP_ENDPOINT").to_owned(),
        public_key: env!("FONO_SETUP_PUBLIC_KEY").to_owned(),
        repository: "whitewolf06/fono".to_owned(),
    }
}

pub fn bootstrap_version() -> &'static str {
    env!("FONO_SETUP_VERSION")
}

pub fn default_cache_dir() -> Result<PathBuf, String> {
    dirs::cache_dir()
        .map(|directory| directory.join("FonoSetup").join("cache"))
        .ok_or_else(|| "Не удалось определить каталог временных загрузок Windows.".into())
}
