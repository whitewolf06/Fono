//! OS-backed storage for renderer-sensitive credentials.

use crate::error::{AppError, AppResult};

const LLM_API_KEY_TARGET: &str = "Fono/llm-api-key";

#[cfg(windows)]
pub fn load_llm_api_key() -> AppResult<Option<String>> {
    use std::ptr;

    use windows::{
        core::{HRESULT, PCWSTR},
        Win32::{
            Foundation::ERROR_NOT_FOUND,
            Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC},
        },
    };

    let target = wide(LLM_API_KEY_TARGET);
    let mut credential: *mut CREDENTIALW = ptr::null_mut();
    let result = unsafe {
        CredReadW(
            PCWSTR(target.as_ptr()),
            CRED_TYPE_GENERIC,
            0,
            &mut credential,
        )
    };
    if let Err(error) = result {
        if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) {
            return Ok(None);
        }
        return Err(AppError::Config(format!(
            "cannot read LLM API key from Windows Credential Manager: {error}"
        )));
    }
    if credential.is_null() {
        return Err(AppError::Config(
            "Windows Credential Manager returned an empty credential pointer".into(),
        ));
    }

    let value = unsafe {
        let credential_ref = &*credential;
        let bytes = std::slice::from_raw_parts(
            credential_ref.CredentialBlob,
            credential_ref.CredentialBlobSize as usize,
        );
        let decoded = String::from_utf8(bytes.to_vec()).map_err(|error| {
            AppError::Config(format!("stored LLM API key is not valid UTF-8: {error}"))
        });
        CredFree(credential.cast());
        decoded?
    };
    Ok(Some(value))
}

#[cfg(windows)]
pub fn store_llm_api_key(value: &str) -> AppResult<()> {
    use windows::{
        core::PWSTR,
        Win32::Security::Credentials::{
            CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        },
    };

    if value.trim().is_empty() {
        return delete_llm_api_key();
    }
    let mut target = wide(LLM_API_KEY_TARGET);
    let mut username = wide("Fono");
    let mut blob = value.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        CredentialBlobSize: blob.len().try_into().map_err(|_| {
            AppError::Config("LLM API key is too large for Windows Credential Manager".into())
        })?,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(username.as_mut_ptr()),
        ..Default::default()
    };
    unsafe { CredWriteW(&credential, 0) }.map_err(|error| {
        AppError::Config(format!(
            "cannot store LLM API key in Windows Credential Manager: {error}"
        ))
    })
}

#[cfg(windows)]
pub fn delete_llm_api_key() -> AppResult<()> {
    use windows::{
        core::{HRESULT, PCWSTR},
        Win32::{
            Foundation::ERROR_NOT_FOUND,
            Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC},
        },
    };

    let target = wide(LLM_API_KEY_TARGET);
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0) } {
        Ok(()) => Ok(()),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
        Err(error) => Err(AppError::Config(format!(
            "cannot delete LLM API key from Windows Credential Manager: {error}"
        ))),
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(not(windows))]
pub fn load_llm_api_key() -> AppResult<Option<String>> {
    Ok(None)
}

#[cfg(not(windows))]
pub fn store_llm_api_key(_value: &str) -> AppResult<()> {
    Err(AppError::Config(
        "secure LLM credential storage is supported only on Windows".into(),
    ))
}

#[cfg(not(windows))]
pub fn delete_llm_api_key() -> AppResult<()> {
    Ok(())
}
