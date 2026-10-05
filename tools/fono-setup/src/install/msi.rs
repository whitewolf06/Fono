//! Only products related to Fono's existing WiX UpgradeCode are inspected.
//! MSI queries are read-only: they never repair or uninstall a product.

use semver::Version;
use windows::{
    core::{w, PCWSTR, PWSTR},
    Win32::{
        Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
        System::ApplicationInstallationAndServicing::{
            MsiEnumRelatedProductsW, MsiGetProductInfoW, INSTALLPROPERTY_PRODUCTNAME,
            INSTALLPROPERTY_VERSIONSTRING,
        },
    },
};

use super::installed::METADATA_ERROR;

// Existing Tauri WiX packages use this identity. The bootstrapper build asserts
// the unchanged com.fono.desktop identifier; identity migration is explicit.
const UPGRADE_CODE: PCWSTR = w!("{BDEF1EEE-F9F0-5E97-BEB6-3ACBE9CF4DBA}");

pub(super) fn installed_versions() -> Result<Vec<Version>, String> {
    let mut versions = Vec::new();
    for index in 0..128 {
        let mut product = [0u16; 39];
        let result =
            unsafe { MsiEnumRelatedProductsW(UPGRADE_CODE, 0, index, PWSTR(product.as_mut_ptr())) };
        if result == ERROR_NO_MORE_ITEMS.0 {
            return Ok(versions);
        }
        if result != ERROR_SUCCESS.0 {
            return Err(METADATA_ERROR.to_owned());
        }
        let product = PCWSTR(product.as_ptr());
        let name = unsafe { product_property(product, INSTALLPROPERTY_PRODUCTNAME)? };
        if !name.trim().eq_ignore_ascii_case("Fono") {
            return Err(METADATA_ERROR.to_owned());
        }
        let version = unsafe { product_property(product, INSTALLPROPERTY_VERSIONSTRING)? };
        versions.push(Version::parse(version.trim()).map_err(|_| METADATA_ERROR.to_owned())?);
    }
    Err(METADATA_ERROR.to_owned())
}

unsafe fn product_property(product: PCWSTR, attribute: PCWSTR) -> Result<String, String> {
    let mut buffer = [0u16; 256];
    let mut length = (buffer.len() - 1) as u32;
    let result = MsiGetProductInfoW(
        product,
        attribute,
        PWSTR(buffer.as_mut_ptr()),
        Some(&mut length),
    );
    if result != ERROR_SUCCESS.0 || length as usize >= buffer.len() {
        return Err(METADATA_ERROR.to_owned());
    }
    String::from_utf16(&buffer[..length as usize]).map_err(|_| METADATA_ERROR.to_owned())
}
