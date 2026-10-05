//! Reads only Fono's uninstall entry; never reads application settings or data.

use semver::Version;
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::*,
    },
};

const UNINSTALL_KEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Fono");
pub(super) const METADATA_ERROR: &str = "Не удалось определить установленную версию Fono. Установка остановлена, чтобы не заменить новую версию более старой. Проверьте Fono в списке установленных приложений Windows.";

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

pub(super) fn require_compatible(candidate: &str) -> Result<(), String> {
    let candidate = Version::parse(candidate).map_err(|_| METADATA_ERROR.to_owned())?;
    let mut versions = Vec::new();
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            if let Some(version) = unsafe { installed_version(hive, view)? } {
                versions.push(version);
            }
        }
    }
    versions.extend(super::msi::installed_versions()?);
    require_not_older(&candidate, versions.iter())
}

unsafe fn installed_version(hive: HKEY, view: REG_SAM_FLAGS) -> Result<Option<Version>, String> {
    let mut key = HKEY::default();
    let result = RegOpenKeyExW(hive, UNINSTALL_KEY, 0, KEY_READ | view, &mut key);
    if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS {
        return Err(METADATA_ERROR.to_owned());
    }
    let key = RegistryKey(key);
    let name = string_value(key.0, w!("DisplayName"))?.ok_or(METADATA_ERROR)?;
    if !name.trim().eq_ignore_ascii_case("Fono") {
        return Err(METADATA_ERROR.to_owned());
    }
    // Authors changed in older Fono builds. The exact product key and name
    // identify the application; Publisher is not a version or trust signal.
    let version = string_value(key.0, w!("DisplayVersion"))?.ok_or(METADATA_ERROR)?;
    Version::parse(version.trim())
        .map(Some)
        .map_err(|_| METADATA_ERROR.to_owned())
}

unsafe fn string_value(key: HKEY, field: PCWSTR) -> Result<Option<String>, String> {
    let mut buffer = [0u16; 1024];
    let mut bytes = (buffer.len() * 2) as u32;
    let result = RegGetValueW(
        key,
        None,
        field,
        RRF_RT_REG_SZ,
        None,
        Some(buffer.as_mut_ptr().cast()),
        Some(&mut bytes),
    );
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS || bytes as usize > buffer.len() * 2 || !bytes.is_multiple_of(2) {
        return Err(METADATA_ERROR.to_owned());
    }
    let length = buffer[..bytes as usize / 2]
        .iter()
        .position(|value| *value == 0)
        .ok_or(METADATA_ERROR)?;
    String::from_utf16(&buffer[..length])
        .map(Some)
        .map_err(|_| METADATA_ERROR.to_owned())
}

fn require_not_older<'a>(
    candidate: &Version,
    versions: impl Iterator<Item = &'a Version>,
) -> Result<(), String> {
    if let Some(installed) = versions.max().filter(|installed| *installed > candidate) {
        return Err(format!("Уже установлена Fono {installed}. В канале обновлений доступна более старая версия {candidate}; установка остановлена. Используйте обновления внутри Fono или дождитесь нового релиза."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_registry_hive_can_block_a_downgrade() {
        let versions = [
            Version::parse("0.1.1").unwrap(),
            Version::parse("0.6.14").unwrap(),
        ];
        assert!(require_not_older(&Version::parse("0.6.12").unwrap(), versions.iter()).is_err());
        assert!(require_not_older(&Version::parse("0.6.14").unwrap(), versions.iter()).is_ok());
        assert!(require_not_older(&Version::parse("0.7.0").unwrap(), versions.iter()).is_ok());
    }

    #[test]
    fn a_new_install_does_not_require_a_registry_entry() {
        assert!(require_not_older(&Version::parse("0.6.12").unwrap(), [].iter()).is_ok());
    }
}
