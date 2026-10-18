//! Read-only metrics and a kill-on-close job containing this test and its children.
use serde::Serialize;
use std::ffi::c_void;

type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[repr(C)]
#[derive(Default)]
struct ProcessMemory {
    size: u32,
    page_faults: u32,
    peak_working_set: usize,
    working_set: usize,
    peak_paged_pool: usize,
    paged_pool: usize,
    peak_nonpaged_pool: usize,
    nonpaged_pool: usize,
    pagefile: usize,
    peak_pagefile: usize,
    private_bytes: usize,
}
#[repr(C)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    id: u32,
    owner: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
}
#[repr(C)]
#[derive(Default)]
struct BasicJobLimits {
    process_time: i64,
    job_time: i64,
    flags: u32,
    minimum_working_set: usize,
    maximum_working_set: usize,
    active_processes: u32,
    affinity: usize,
    priority: u32,
    scheduling_class: u32,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedJobLimits {
    basic: BasicJobLimits,
    io_counters: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetProcessHandleCount(process: Handle, count: *mut u32) -> i32;
    fn GetProcessTimes(
        process: Handle,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> Handle;
    fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, data: *const c_void, size: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, timeout: u32) -> u32;
    fn ExitProcess(code: u32) -> !;
    fn GetLastError() -> u32;
}
#[link(name = "psapi")]
extern "system" {
    fn GetProcessMemoryInfo(process: Handle, counters: *mut ProcessMemory, size: u32) -> i32;
}
struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // Every handle here was returned to this harness by a Win32 API.
        unsafe { CloseHandle(self.0) };
    }
}
fn last_error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}

/// This is a dedicated test executable invocation with exactly one selected test.
/// Its job handle intentionally lives until process exit. Windows then kills any
/// remaining descendant even if the runner is interrupted or the test panics.
pub(super) fn protect_descendants() -> Result<(), String> {
    unsafe {
        let job = OwnedHandle(CreateJobObjectW(std::ptr::null(), std::ptr::null()));
        if job.0.is_null() {
            return Err(last_error("create private soak job"));
        }
        let limits = ExtendedJobLimits {
            basic: BasicJobLimits {
                flags: 0x2000, // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                ..Default::default()
            },
            ..Default::default()
        };
        if SetInformationJobObject(
            job.0,
            9, // JobObjectExtendedLimitInformation
            &limits as *const _ as *const c_void,
            std::mem::size_of_val(&limits) as u32,
        ) == 0
        {
            return Err(last_error("set soak kill-on-close job"));
        }
        if AssignProcessToJobObject(job.0, GetCurrentProcess()) == 0 {
            return Err(last_error("attach dedicated soak test process to job"));
        }
        std::mem::forget(job);
    }
    Ok(())
}

/// A kernel handle observes the exact controlling runner even if Node is killed
/// without receiving a signal. This dedicated driver then exits; its private
/// kill-on-close job also kills its own worker descendants. The independent
/// native deadline bounds the driver if the external timer cannot run.
pub(super) fn watch_runner(runner_pid: u32, timeout_seconds: u64) -> Result<(), String> {
    let handle = unsafe { OpenProcess(0x0010_0000, 0, runner_pid) }; // SYNCHRONIZE
    if handle.is_null() {
        return Err(last_error("open controlling runner lifetime handle"));
    }
    let raw = handle as usize;
    let spawned = std::thread::Builder::new()
        .name("fono-soak-runner-watchdog".into())
        .spawn(move || {
            let parent = OwnedHandle(raw as Handle);
            let result = unsafe { WaitForSingleObject(parent.0, (timeout_seconds * 1000) as u32) };
            let code = if result == 0 { 3 } else { 4 };
            unsafe { ExitProcess(code) };
        });
    if spawned.is_err() {
        unsafe { CloseHandle(handle) };
        return Err("could not start controlling-runner watchdog".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Resources {
    pub pid: u32,
    pub handles: u32,
    pub threads: u32,
    pub private_bytes: usize,
    pub cpu_seconds: f64,
}

fn process(pid: u32) -> Result<OwnedHandle, String> {
    let handle = unsafe { OpenProcess(0x0400 | 0x0010, 0, pid) };
    if handle.is_null() {
        Err(last_error("open owned process for metrics"))
    } else {
        Ok(OwnedHandle(handle))
    }
}
pub(super) fn cpu_seconds(pid: u32) -> Result<f64, String> {
    let process = process(pid)?;
    cpu_time(process.0)
}
fn cpu_time(process: Handle) -> Result<f64, String> {
    let (mut creation, mut exit, mut kernel, mut user) = (
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
    );
    if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(last_error("read owned process CPU time"));
    }
    let ticks = |time: FileTime| (u64::from(time.high) << 32) | u64::from(time.low);
    Ok((ticks(kernel) + ticks(user)) as f64 / 10_000_000.0)
}
pub(super) fn resources(pid: u32) -> Result<Resources, String> {
    let process = process(pid)?;
    let mut handles = 0;
    let mut memory = ProcessMemory {
        size: std::mem::size_of::<ProcessMemory>() as u32,
        ..Default::default()
    };
    let memory_size = memory.size;
    if unsafe { GetProcessHandleCount(process.0, &mut handles) } == 0
        || unsafe { GetProcessMemoryInfo(process.0, &mut memory, memory_size) } == 0
    {
        return Err(last_error("read owned process resources"));
    }
    let threads = unsafe {
        let snapshot = OwnedHandle(CreateToolhelp32Snapshot(4, 0)); // TH32CS_SNAPTHREAD
        if snapshot.0 == -1_isize as Handle {
            return Err(last_error("snapshot threads for numeric count"));
        }
        let mut entry: ThreadEntry = std::mem::zeroed();
        entry.size = std::mem::size_of::<ThreadEntry>() as u32;
        let mut present = Thread32First(snapshot.0, &mut entry);
        if present == 0 && GetLastError() != 18 {
            // ERROR_NO_MORE_FILES
            return Err(last_error("read first thread snapshot entry"));
        }
        let mut count = 0;
        while present != 0 {
            count += u32::from(entry.owner == pid);
            present = Thread32Next(snapshot.0, &mut entry);
            if present == 0 && GetLastError() != 18 {
                return Err(last_error("read next thread snapshot entry"));
            }
        }
        if count == 0 {
            return Err("live owned process had no measurable threads".into());
        }
        count
    };
    Ok(Resources {
        pid,
        handles,
        threads,
        private_bytes: memory.private_bytes,
        cpu_seconds: cpu_time(process.0)?,
    })
}
