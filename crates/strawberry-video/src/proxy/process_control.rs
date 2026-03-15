//! # Proxy Process Control
//!
//! OS-level suspend/resume of FFmpeg proxy subprocesses.
//!
//! ## Purpose
//!
//! When the user presses Play, proxy FFmpeg processes are suspended at the OS
//! level (frozen: zero CPU, zero I/O). When playback stops, they resume at
//! IDLE priority. This eliminates I/O contention between proxy generation and
//! the playback decode pipeline.
//!
//! ## Platform Implementation
//!
//! - **Windows**: Uses `NtSuspendProcess`/`NtResumeProcess` via raw FFI from
//!   `ntdll.dll`. No external dependencies needed.
//! - **Unix**: Uses `SIGSTOP`/`SIGCONT` via `libc::kill`.

use super::ProxyManager;

impl ProxyManager {
    /// Suspend all active proxy FFmpeg subprocesses.
    ///
    /// Called by `VideoAssetManager::start_playback()` to eliminate I/O
    /// contention between proxy generation and playback decode. The FFmpeg
    /// processes are frozen at the OS level — zero CPU/IO usage.
    ///
    /// Safe to call when no proxies are running (no-op).
    pub fn suspend_proxy_processes(&self) {
        let pids: Vec<u32> = match self.active_proxy_pids.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };

        if pids.is_empty() {
            return;
        }

        tracing::info!(
            "Suspending {} proxy FFmpeg process(es) for playback",
            pids.len()
        );

        for pid in &pids {
            Self::suspend_process(*pid);
        }
    }

    /// Resume all suspended proxy FFmpeg subprocesses.
    ///
    /// Called by `VideoAssetManager::stop_playback()` to let proxy generation
    /// continue after playback stops.
    ///
    /// Safe to call when no proxies are suspended (no-op).
    pub fn resume_proxy_processes(&self) {
        let pids: Vec<u32> = match self.active_proxy_pids.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };

        if pids.is_empty() {
            return;
        }

        tracing::info!(
            "Resuming {} proxy FFmpeg process(es) after playback",
            pids.len()
        );

        for pid in &pids {
            Self::resume_process(*pid);
        }
    }

    /// Suspend a single process by PID (Windows — raw FFI, no external deps)
    #[cfg(windows)]
    fn suspend_process(pid: u32) {
        type HANDLE = *mut std::ffi::c_void;
        const PROCESS_SUSPEND_RESUME: u32 = 0x0800;

        extern "system" {
            fn OpenProcess(desired_access: u32, inherit_handle: i32, pid: u32) -> HANDLE;
            fn CloseHandle(handle: HANDLE) -> i32;
            fn GetModuleHandleA(name: *const u8) -> HANDLE;
            fn GetProcAddress(
                module: HANDLE,
                name: *const u8,
            ) -> Option<unsafe extern "system" fn() -> isize>;
        }

        unsafe {
            let handle: HANDLE = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
            if handle.is_null() {
                tracing::warn!("Failed to open process {} for suspend", pid);
                return;
            }

            let ntdll: HANDLE = GetModuleHandleA(b"ntdll.dll\0".as_ptr());
            if !ntdll.is_null() {
                if let Some(func) = GetProcAddress(ntdll, b"NtSuspendProcess\0".as_ptr()) {
                    let nt_suspend: extern "system" fn(HANDLE) -> i32 = std::mem::transmute(func);
                    let status: i32 = nt_suspend(handle);
                    if status == 0 {
                        tracing::debug!("Suspended proxy process {}", pid);
                    } else {
                        tracing::warn!("NtSuspendProcess failed for PID {}: 0x{:X}", pid, status);
                    }
                }
            }

            CloseHandle(handle);
        }
    }

    /// Resume a single process by PID (Windows — raw FFI, no external deps)
    #[cfg(windows)]
    fn resume_process(pid: u32) {
        type HANDLE = *mut std::ffi::c_void;
        const PROCESS_SUSPEND_RESUME: u32 = 0x0800;

        extern "system" {
            fn OpenProcess(desired_access: u32, inherit_handle: i32, pid: u32) -> HANDLE;
            fn CloseHandle(handle: HANDLE) -> i32;
            fn GetModuleHandleA(name: *const u8) -> HANDLE;
            fn GetProcAddress(
                module: HANDLE,
                name: *const u8,
            ) -> Option<unsafe extern "system" fn() -> isize>;
        }

        unsafe {
            let handle: HANDLE = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
            if handle.is_null() {
                tracing::warn!("Failed to open process {} for resume", pid);
                return;
            }

            let ntdll: HANDLE = GetModuleHandleA(b"ntdll.dll\0".as_ptr());
            if !ntdll.is_null() {
                if let Some(func) = GetProcAddress(ntdll, b"NtResumeProcess\0".as_ptr()) {
                    let nt_resume: extern "system" fn(HANDLE) -> i32 = std::mem::transmute(func);
                    let status: i32 = nt_resume(handle);
                    if status == 0 {
                        tracing::debug!("Resumed proxy process {}", pid);
                    } else {
                        tracing::warn!("NtResumeProcess failed for PID {}: 0x{:X}", pid, status);
                    }
                }
            }

            CloseHandle(handle);
        }
    }

    /// Suspend a single process by PID — Unix (SIGSTOP)
    #[cfg(not(windows))]
    fn suspend_process(pid: u32) {
        unsafe {
            libc::kill(pid as i32, libc::SIGSTOP);
        }
        tracing::debug!("Sent SIGSTOP to proxy process {}", pid);
    }

    /// Resume a single process by PID — Unix (SIGCONT)
    #[cfg(not(windows))]
    fn resume_process(pid: u32) {
        unsafe {
            libc::kill(pid as i32, libc::SIGCONT);
        }
        tracing::debug!("Sent SIGCONT to proxy process {}", pid);
    }
}
