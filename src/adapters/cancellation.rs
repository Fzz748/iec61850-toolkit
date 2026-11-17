use crate::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);

pub fn requested() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

#[cfg(windows)]
unsafe extern "system" fn handler(event: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        CANCELLED.store(true, Ordering::Relaxed);
        1
    } else {
        0
    }
}

#[cfg(unix)]
extern "C" fn handler(_: i32) {
    CANCELLED.store(true, Ordering::Relaxed);
}

pub struct Cancellation {
    #[cfg(unix)]
    previous: libc::sighandler_t,
}

impl Cancellation {
    pub fn install() -> Result<Self> {
        CANCELLED.store(false, Ordering::Relaxed);
        #[cfg(windows)]
        {
            // The callback only sets a lock-free atomic; it does not touch application state.
            if unsafe {
                windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(handler), 1)
            } == 0
            {
                return Err(Error::Io(std::io::Error::last_os_error()));
            }
            Ok(Self {})
        }
        #[cfg(unix)]
        {
            let previous =
                unsafe { libc::signal(libc::SIGINT, handler as *const () as libc::sighandler_t) };
            if previous == libc::SIG_ERR {
                return Err(Error::Io(std::io::Error::last_os_error()));
            }
            Ok(Self { previous })
        }
    }
}

impl Drop for Cancellation {
    fn drop(&mut self) {
        #[cfg(windows)]
        if unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(handler), 0) }
            == 0
        {
            eprintln!(
                "cannot restore Ctrl+C handler: {}",
                std::io::Error::last_os_error()
            );
        }
        #[cfg(unix)]
        if unsafe { libc::signal(libc::SIGINT, self.previous) } == libc::SIG_ERR {
            eprintln!(
                "cannot restore SIGINT handler: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}
