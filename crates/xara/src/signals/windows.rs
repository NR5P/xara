use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use windows_sys::Win32::Foundation::{BOOL, FALSE, TRUE};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};

struct Trap {
    stop: Arc<AtomicBool>,
    signaled: Arc<AtomicBool>,
    before_exit: fn(),
}

static TRAP: OnceLock<Trap> = OnceLock::new();

/// How long a console close, logoff or shutdown waits for xara to stop before Windows ends it.
const CLOSE_GRACE: Duration = Duration::from_secs(4);

/// Runs on a thread Windows starts for each console event.
unsafe extern "system" fn handler(event: u32) -> BOOL {
    let Some(t) = TRAP.get() else { return FALSE };
    if t.signaled.swap(true, Ordering::SeqCst) {
        (t.before_exit)();
        std::process::exit(130);
    }
    t.stop.store(true, Ordering::SeqCst);
    if event != CTRL_C_EVENT && event != CTRL_BREAK_EVENT {
        // Close, logoff and shutdown end the process when this returns: give xara time to
        // release PTT and close its ports first.
        std::thread::sleep(CLOSE_GRACE);
    }
    TRUE
}

/// Windows has no Unix signals; console control events (Ctrl-C, Ctrl-Break, closing the
/// window, logoff, shutdown) take their place, whatever `sigs` names.
pub fn trap(_sigs: &[libc::c_int], stop: Arc<AtomicBool>, before_exit: fn()) -> io::Result<Arc<AtomicBool>> {
    let signaled = Arc::new(AtomicBool::new(false));
    TRAP.set(Trap { stop, signaled: signaled.clone(), before_exit })
        .map_err(|_| io::Error::other("signals are already trapped"))?;
    // SAFETY: handler is a valid console control handler for the life of the process.
    if unsafe { SetConsoleCtrlHandler(Some(handler), TRUE) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(signaled)
}
