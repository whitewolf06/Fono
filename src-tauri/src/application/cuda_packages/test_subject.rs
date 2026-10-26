//! Exercise owner ACLs as an ordinary user even on elevated Windows CI runners.
//! Only this test thread is restricted; no accounts or host security policies change.
use std::{ffi::c_void, marker::PhantomData, os::windows::ffi::OsStrExt, path::Path, rc::Rc};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, LocalFree, BOOL, ERROR_NO_TOKEN, HANDLE, HLOCAL},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            CheckTokenMembership, CreateRestrictedToken, CreateWellKnownSid, GetTokenInformation,
            ImpersonateLoggedOnUser, RevertToSelf, SetTokenInformation, TokenOwner, TokenUser,
            WinBuiltinAdministratorsSid, DISABLE_MAX_PRIVILEGE, PSECURITY_DESCRIPTOR, PSID,
            SECURITY_ATTRIBUTES, SID_AND_ATTRIBUTES, TOKEN_ADJUST_DEFAULT, TOKEN_DUPLICATE,
            TOKEN_IMPERSONATE, TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER,
        },
        Storage::FileSystem::CreateDirectoryW,
        System::Threading::{
            GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken, SetThreadToken,
        },
    },
};

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0).expect("close test token") };
    }
}

struct Allocation(*mut c_void);
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(HLOCAL(self.0));
        }
    }
}

pub(super) struct Subject {
    previous: Option<Token>,
    // Impersonation belongs to the creating thread and must never move to another.
    _thread: PhantomData<Rc<()>>,
}

impl Subject {
    pub(super) fn create(path: &Path) -> Self {
        let mut process = HANDLE::default();
        let access = TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE | TOKEN_ADJUST_DEFAULT;
        // A nested fixture must duplicate the effective thread token. Opening
        // the elevated process token from the restricted subject is forbidden.
        let nested =
            match unsafe { OpenThreadToken(GetCurrentThread(), access, true, &mut process) } {
                Ok(()) => true,
                Err(error) if error.code() == ERROR_NO_TOKEN.to_hresult() => {
                    unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut process) }
                        .expect("open current process token for the ACL test subject");
                    false
                }
                Err(error) => panic!("open effective ACL test subject: {error}"),
            };
        let process = Token(process);
        let mut needed = 0;
        let _ = unsafe { GetTokenInformation(process.0, TokenUser, None, 0, &mut needed) };
        assert!(needed > 0 && needed < 64 * 1024);
        let mut user_buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        unsafe {
            GetTokenInformation(
                process.0,
                TokenUser,
                Some(user_buffer.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
            .expect("read current test user SID");
        }
        let user = unsafe { &*user_buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut admin_buffer = [0usize; 16];
        let admin = PSID(admin_buffer.as_mut_ptr().cast());
        let mut admin_size = size_of_val(&admin_buffer) as u32;
        unsafe {
            CreateWellKnownSid(WinBuiltinAdministratorsSid, None, admin, &mut admin_size)
                .expect("create Administrators SID");
        }
        let disabled = [SID_AND_ATTRIBUTES {
            Sid: admin,
            Attributes: 0,
        }];
        let mut restricted = HANDLE::default();
        unsafe {
            CreateRestrictedToken(
                process.0,
                DISABLE_MAX_PRIVILEGE,
                Some(&disabled),
                None,
                None,
                &mut restricted,
            )
            .expect("restrict the test subject's administrative privileges");
        }
        let restricted = Token(restricted);
        let owner = TOKEN_OWNER {
            Owner: user.User.Sid,
        };
        unsafe {
            SetTokenInformation(
                restricted.0,
                TokenOwner,
                (&owner as *const TOKEN_OWNER).cast(),
                size_of::<TOKEN_OWNER>() as u32,
            )
            .expect("new fixture objects must belong to the individual test user");
        }

        // A runner's temp root may grant access only through Administrators.
        // Give the individual user access to this new UUID directory explicitly.
        let mut sid = PWSTR::null();
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid).expect("format test user SID") };
        let sid_allocation = Allocation(sid.0.cast());
        let sid = unsafe { sid.to_string().expect("read test user SID") };
        let sddl: Vec<u16> =
            format!("O:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")
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
            .expect("create private fixture directory security descriptor");
        }
        let descriptor_allocation = Allocation(descriptor.0);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: BOOL(0),
        };
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            CreateDirectoryW(PCWSTR(path.as_ptr()), Some(&attributes))
                .expect("create user-owned private ACL fixture directory");
        }
        drop(descriptor_allocation);
        drop(sid_allocation);

        let previous = nested.then_some(process);
        unsafe {
            ImpersonateLoggedOnUser(restricted.0).expect("enter ordinary-user ACL test subject")
        };
        let subject = Self {
            previous,
            _thread: PhantomData,
        };
        let mut administrator = BOOL(0);
        unsafe {
            CheckTokenMembership(None, admin, &mut administrator)
                .expect("check effective test subject privileges");
        }
        assert!(
            !administrator.as_bool(),
            "ACL tests must not bypass owner protection as Administrators"
        );
        subject
    }
}

impl Drop for Subject {
    fn drop(&mut self) {
        // Nested Temporary fixtures restore the prior restricted subject.
        let result = unsafe {
            match &self.previous {
                Some(previous) => SetThreadToken(None, previous.0),
                None => RevertToSelf(),
            }
        };
        result.expect("restore test thread security context");
    }
}
