use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use crate::diagnostics::Diagnostics;

pub fn failure_message(
    window_loop_started: bool,
    error: &str,
    database: &str,
    log: &str,
) -> String {
    let heading = if window_loop_started {
        "TaskCascade stopped unexpectedly."
    } else {
        "TaskCascade could not start."
    };
    format!("{heading}\n\n{error}\n\nDatabase: {database}\nLog: {log}")
}

#[cfg(windows)]
pub fn show(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    // SAFETY: The UTF-16 buffers are null-terminated and live for the entire call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
pub fn show(title: &str, message: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "{title}\n{message}");
}

#[derive(Default)]
pub struct PanicReport {
    message: Mutex<Option<String>>,
    window_loop_started: AtomicBool,
}

impl PanicReport {
    pub fn install_hook(self: &Arc<Self>, diagnostics: Arc<Diagnostics>) {
        let report = Arc::clone(self);
        std::panic::set_hook(Box::new(move |info| {
            let message = info.to_string();
            diagnostics.log(&format!("desktop panic: {message}"));
            *report
                .message
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(message);
        }));
    }

    pub fn mark_window_loop_started(&self) {
        self.window_loop_started.store(true, Ordering::SeqCst);
    }

    pub fn window_loop_started(&self) -> bool {
        self.window_loop_started.load(Ordering::SeqCst)
    }

    pub fn message(&self) -> String {
        self.message
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .unwrap_or_else(|| "panic details were unavailable".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::failure_message;

    #[test]
    fn failure_message_names_paths_and_distinguishes_running_loop() {
        for (started, heading) in [(false, "could not start"), (true, "stopped unexpectedly")] {
            let message = failure_message(
                started,
                "test failure",
                "C:/scratch/tasks.sqlite",
                "C:/scratch/logs/desktop.log",
            );
            assert!(message.starts_with(&format!("TaskCascade {heading}.")));
            assert!(message.contains("test failure"));
            assert!(message.contains("Database: C:/scratch/tasks.sqlite"));
            assert!(message.contains("Log: C:/scratch/logs/desktop.log"));
        }
    }
}
