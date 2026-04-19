//! Tie the spawned analyzer's lifetime to this launcher process.
//!
//! Motivation: when the editor (e.g. VS Code) terminates the launcher, the
//! child analyzer should go down too. Without OS-level coupling Linux orphans
//! the child to init and Windows leaves it fully detached.
//!
//! - Linux: `PR_SET_PDEATHSIG(SIGKILL)` installed in the child before exec.
//!   The kernel delivers `SIGKILL` to the child as soon as the launcher
//!   thread dies for any reason (including SIGKILL on the launcher itself).
//! - Windows: a Job Object with `KILL_ON_JOB_CLOSE`, to which the child is
//!   attached immediately after `spawn()` via `AssignProcessToJobObject`.
//!   When the launcher exits, the last handle to the job closes and the
//!   kernel terminates the job. NOTE: there is a brief window between
//!   `CreateProcess` returning and the `AssignProcessToJobObject` call
//!   during which a grandchild spawned by the analyzer would escape the
//!   job. `bsl-analyzer-app` does not fork any processes during its LSP
//!   init handshake, so this window is safe in practice. The atomic
//!   alternative (`PROC_THREAD_ATTRIBUTE_JOB_LIST`) currently requires
//!   a nightly-only std API.
//! - macOS / other Unix: no reliable kernel primitive; rely on stdin EOF
//!   propagating through `lsp-server` for graceful shutdown.

use std::process::{Child, Command};

use anyhow::Result;

/// Guard that, while alive, keeps the child analyzer bound to the launcher.
///
/// On Windows it owns the Job Object handle; dropping it closes the last
/// handle and the kernel terminates the job. On other platforms it is a
/// zero-sized marker.
pub struct LifecycleGuard {
    #[cfg(windows)]
    // Field exists solely so its `Drop` fires with the guard; never read.
    #[allow(dead_code)]
    job: windows_impl::JobHandle,
}

/// Configures `cmd` so that, once spawned, the child will receive a lethal
/// signal/handle-close when this launcher dies. Must be called *before*
/// `cmd.spawn()`.
#[cfg(target_os = "linux")]
pub fn configure_parent_death(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;

    // Capture the launcher's PID *before* fork; compare it against
    // `getppid()` after fork to detect a parent that died (or was re-parented
    // via a `PR_SET_CHILD_SUBREAPER` ancestor) in the fork-before-exec
    // window. `== 1` alone would miss the subreaper case.
    let expected_parent_pid = unsafe { libc::getpid() };

    // SAFETY: pre_exec only invokes async-signal-safe syscalls
    // (prctl, getppid, _exit); no heap allocation, no locks.
    unsafe {
        cmd.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            // PR_SET_PDEATHSIG arms only the *future* death notification.
            // If the launcher already died, bail out now — `_exit` ignores
            // signal dispositions that might otherwise neutralise SIGKILL
            // via `raise`.
            if libc::getppid() != expected_parent_pid {
                libc::_exit(1);
            }
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
pub fn configure_parent_death(_cmd: &mut Command) {}

/// Binds an already-spawned `child` to the launcher's lifetime and returns
/// a guard that must outlive it.
///
/// On Windows this creates a kill-on-close Job Object and assigns the child
/// to it. On other platforms it is a no-op that returns a trivial guard.
#[cfg(windows)]
pub fn adopt_child(child: &Child) -> Result<LifecycleGuard> {
    Ok(LifecycleGuard { job: windows_impl::assign_to_kill_on_close_job(child)? })
}

#[cfg(not(windows))]
pub fn adopt_child(_child: &Child) -> Result<LifecycleGuard> {
    Ok(LifecycleGuard {})
}

#[cfg(windows)]
mod windows_impl {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::ptr;

    use anyhow::{bail, Result};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct JobHandle(HANDLE);

    impl Drop for JobHandle {
        fn drop(&mut self) {
            // SAFETY: handle came from `CreateJobObjectW` and is still owned
            // by this guard; no other owner calls `CloseHandle` on it.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub fn assign_to_kill_on_close_job(child: &Child) -> Result<JobHandle> {
        // SAFETY: Win32 FFI; pointers are either null per docs or point to
        // stack-owned storage valid for the duration of each call.
        unsafe {
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            if handle.is_null() {
                bail!("CreateJobObjectW failed: {}", std::io::Error::last_os_error());
            }
            let job = JobHandle(handle);

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                bail!("SetInformationJobObject failed: {}", std::io::Error::last_os_error());
            }

            let process_handle = child.as_raw_handle() as HANDLE;
            if AssignProcessToJobObject(handle, process_handle) == 0 {
                bail!("AssignProcessToJobObject failed: {}", std::io::Error::last_os_error());
            }

            Ok(job)
        }
    }
}
