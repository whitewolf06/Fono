//! Persistent payload-directory protection. Share handles protect the signed
//! files, while this ACL prevents unsigned child creation between validation
//! and Windows' pre-main DLL loading. Admin/System recovery remains possible.
use super::{PackageError, Result};
use std::path::Path;

#[cfg(windows)]
mod windows_seal {
    use super::*;
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    use windows::{
        core::{w, PCWSTR, PWSTR},
        Win32::{
            Foundation::{CloseHandle, LocalFree, BOOL, HANDLE, HLOCAL},
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW,
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
                    SDDL_REVISION_1, SE_FILE_OBJECT,
                },
                EqualSid, GetSecurityDescriptorOwner, GetTokenInformation, SetFileSecurityW,
                TokenUser, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
                PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY,
                TOKEN_USER,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    // OWNER RIGHTS disables the owner's implicit WRITE_DAC grant. Ordinary
    // ownership alone cannot restore a permissive ACL. The owner receives
    // list/read/execute + READ_CONTROL + SYNCHRONIZE + DELETE, with no child
    // creation, data write or WRITE_DAC. DELETE permits cleanup after handles
    // close even when the parent grants Modify rather than DELETE_CHILD.
    // During use the retained directory handle explicitly denies delete.
    const SDDL: PCWSTR = w!("D:P(A;;0x001300A9;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)");

    struct Allocation(*mut c_void);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(HLOCAL(self.0));
            }
        }
    }
    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn native(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn expected() -> Result<Allocation> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                SDDL,
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|_| PackageError::Storage)?;
        Ok(Allocation(descriptor.0))
    }

    fn canonical(descriptor: PSECURITY_DESCRIPTOR) -> Result<String> {
        let mut text = PWSTR::null();
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
        }
        .map_err(|_| PackageError::Storage)?;
        let allocation = Allocation(text.0.cast());
        let value = unsafe { text.to_string() }.map_err(|_| PackageError::Storage)?;
        drop(allocation);
        Ok(value)
    }

    fn descriptor(path: &Path) -> Result<Allocation> {
        let path = native(path);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let status = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(path.as_ptr()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                &mut descriptor,
            )
        };
        if status.0 != 0 {
            return Err(PackageError::Storage);
        }
        Ok(Allocation(descriptor.0))
    }

    fn owner_is_current_user(descriptor: PSECURITY_DESCRIPTOR) -> Result<()> {
        let mut owner = PSID::default();
        let mut defaulted = BOOL::default();
        unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) }
            .map_err(|_| PackageError::Storage)?;
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .map_err(|_| PackageError::Storage)?;
        let token = Token(token);
        let mut needed = 0;
        let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) };
        if needed == 0 || needed > 64 * 1024 {
            return Err(PackageError::Storage);
        }
        // Aligned storage is required for TOKEN_USER and its SID pointer.
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
        }
        .map_err(|_| PackageError::Storage)?;
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        if unsafe { EqualSid(owner, user.User.Sid) }.is_err() {
            return Err(PackageError::Storage);
        }
        Ok(())
    }

    pub(super) fn seal(path: &Path) -> Result<()> {
        super::super::storage::reject_links(path)?;
        let current = descriptor(path)?;
        owner_is_current_user(PSECURITY_DESCRIPTOR(current.0))?;
        let expected = expected()?;
        let path = native(path);
        if !unsafe {
            SetFileSecurityW(
                PCWSTR(path.as_ptr()),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR(expected.0),
            )
        }
        .as_bool()
        {
            return Err(PackageError::Storage);
        }
        Ok(())
    }

    pub(super) fn verify(path: &Path) -> Result<()> {
        super::super::storage::reject_links(path)?;
        let current = descriptor(path)?;
        owner_is_current_user(PSECURITY_DESCRIPTOR(current.0))?;
        let expected = expected()?;
        if canonical(PSECURITY_DESCRIPTOR(current.0))?
            != canonical(PSECURITY_DESCRIPTOR(expected.0))?
        {
            return Err(PackageError::Storage);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn owner_can_reset_acl(path: &Path) -> Result<bool> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                w!("D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)"),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|_| PackageError::Storage)?;
        let allocation = Allocation(descriptor.0);
        let path = native(path);
        let success = unsafe {
            SetFileSecurityW(
                PCWSTR(path.as_ptr()),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR(allocation.0),
            )
        }
        .as_bool();
        Ok(success)
    }

    #[cfg(test)]
    pub(super) fn parent_modify(path: &Path) -> Result<()> {
        use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .map_err(|_| PackageError::Storage)?;
        let token = Token(token);
        let mut needed = 0;
        let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) };
        if needed == 0 || needed > 64 * 1024 {
            return Err(PackageError::Storage);
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
        }
        .map_err(|_| PackageError::Storage)?;
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut sid = PWSTR::null();
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) }
            .map_err(|_| PackageError::Storage)?;
        let sid_allocation = Allocation(sid.0.cast());
        let sid = unsafe { sid.to_string() }.map_err(|_| PackageError::Storage)?;
        drop(sid_allocation);
        // Ordinary inherited Modify (0x1301bf), specifically without the
        // FILE_DELETE_CHILD (0x40) bit. No OWNER RIGHTS ACE until payload seal,
        // so creation-time ownership can still apply its final protected ACL.
        let sddl: Vec<u16> =
            format!("D:P(A;OICI;0x001301bf;;;{sid})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")
                .encode_utf16()
                .chain(Some(0))
                .collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|_| PackageError::Storage)?;
        let allocation = Allocation(descriptor.0);
        let path = native(path);
        if !unsafe {
            SetFileSecurityW(
                PCWSTR(path.as_ptr()),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR(allocation.0),
            )
        }
        .as_bool()
        {
            return Err(PackageError::Storage);
        }
        Ok(())
    }
}

pub(super) fn seal(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows_seal::seal(path)?;
        windows_seal::verify(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(PackageError::Configuration)
    }
}

pub(super) fn verify(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows_seal::verify(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(PackageError::Configuration)
    }
}

#[cfg(all(test, windows))]
pub(super) fn owner_can_reset_acl(path: &Path) -> Result<bool> {
    windows_seal::owner_can_reset_acl(path)
}

#[cfg(all(test, windows))]
pub(super) fn parent_modify(path: &Path) -> Result<()> {
    windows_seal::parent_modify(path)
}
