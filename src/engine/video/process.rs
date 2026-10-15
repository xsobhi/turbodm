//! Running yt-dlp: no console window, UTF-8 output, and stopping all of it. yt-dlp's Windows
//! and Linux builds are two processes (a launcher that unpacks Python, then yt-dlp itself), and
//! yt-dlp runs FFmpeg: stopping only the first would leave the others downloading. So on Windows
//! they all go in a job object, which also ends them if TurboDM itself goes; on Linux, in a
//! process group of their own.

use std::path::Path;
use std::process::Stdio;
use tokio::process::{Child, Command};

/// CREATE_NO_WINDOW: console programs started by TurboDM (which has no console) don't open one.
pub const NO_WINDOW: u32 = 0x0800_0000;

/// A command for `program`, its output piped to us.
pub fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true)
        // Python writes the console's code page to a pipe, and buffers it, unless told otherwise
        .env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1").env("PYTHONUNBUFFERED", "1");
    #[cfg(windows)]
    command.creation_flags(NO_WINDOW);
    #[cfg(unix)]
    command.process_group(0);
    command
}

/// A running yt-dlp and everything it started.
pub struct Running {
    pub child: Child,
    #[cfg(windows)]
    job: job::Job,
}

impl Running {
    pub fn spawn(mut command: Command) -> std::io::Result<Self> {
        let child = command.spawn()?;
        #[cfg(windows)]
        let job = {
            let job = job::Job::new().unwrap_or_default();
            if let Some(handle) = child.raw_handle() {
                job.add(handle);
            }
            job
        };
        Ok(Running { child, #[cfg(windows)] job })
    }

    /// Stop it all and wait for it (what it downloaded stays, for --continue).
    pub async fn stop(&mut self) {
        #[cfg(windows)]
        self.job.terminate();
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            unsafe extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            // SAFETY: a signal to the process group we started (its id is the leader's); SIGTERM
            // lets yt-dlp's launcher pass it on and Python end cleanly
            unsafe { kill(-(pid as i32), 15) };
            let ended = tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await;
            if ended.is_err() {
                unsafe { kill(-(pid as i32), 9) }; // SIGKILL: it didn't listen
            }
        }
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

#[cfg(windows)]
mod job {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::*;

    /// A job object whose processes end when it's closed (or TurboDM ends).
    #[derive(Default)]
    pub struct Job(isize);

    impl Job {
        pub fn new() -> Option<Self> {
            // SAFETY: a new unnamed job, set to kill its processes when its last handle closes
            unsafe {
                let job = CreateJobObjectW(None, windows::core::PCWSTR::null()).ok()?;
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let _ = SetInformationJobObject(job, JobObjectExtendedLimitInformation, (&raw const limits).cast(),
                                                std::mem::size_of_val(&limits) as u32);
                Some(Job(job.0 as isize))
            }
        }

        /// Put a process in it; what it starts from then on is in it too. (The launcher starts
        /// yt-dlp after unpacking it, long after this.)
        pub fn add(&self, process: std::os::windows::io::RawHandle) {
            if self.0 != 0 {
                // SAFETY: the child's own process handle, still open (tokio holds it)
                unsafe { let _ = AssignProcessToJobObject(self.handle(), HANDLE(process)); }
            }
        }

        pub fn terminate(&self) {
            if self.0 != 0 {
                // SAFETY: our job handle; ends every process in it
                unsafe { let _ = TerminateJobObject(self.handle(), 1); }
            }
        }

        fn handle(&self) -> HANDLE {
            HANDLE(self.0 as *mut _)
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            if self.0 != 0 {
                // SAFETY: closes our handle (ending what's left in the job)
                unsafe { let _ = CloseHandle(self.handle()); }
            }
        }
    }
}
