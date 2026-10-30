//! Read-only readiness probe. COM objects never leave the worker STA.
use super::wake::Wake;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetParent, GetWindowThreadProcessId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Desktop {
    view: isize,
    process: u32,
}

impl Desktop {
    fn current() -> Result<Self, String> {
        let view = luciddesk_explorer::desktop_view()?;
        let mut process = 0;
        unsafe { GetWindowThreadProcessId(view as _, &mut process); }
        if process == 0 { return Err("Desktop window is no longer available".into()); }
        Ok(Self { view, process })
    }

    pub fn validate(self) -> Result<(), String> {
        if Self::current()? != self {
            return Err("Desktop changed during readiness detection; retrying".into());
        }
        Ok(())
    }
}

pub(super) struct Probe(Receiver<Result<Desktop, String>>);

impl Probe {
    #[cfg(test)]
    pub fn pending() -> (Self, mpsc::Sender<Result<Desktop, String>>) {
        let (sender, receiver) = mpsc::channel();
        (Self(receiver), sender)
    }

    pub fn start(wake: Wake) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new().name("desktop-ready".into()).spawn(move || {
            // Wake on success, failure and unwind, after publishing/disconnecting.
            let _notify = wake.on_drop();
            let sender = sender;
            let started = std::time::Instant::now();
            let result = detect();
            luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Info,
                "desktop.readiness", "outcome={} elapsed_ms={}",
                if result.is_ok() { "ready" } else { "waiting" }, started.elapsed().as_millis());
            let _ = sender.send(result);
        }).map_err(|error| error.to_string())?;
        Ok(Self(receiver))
    }

    pub fn poll(&self) -> Option<Result<Desktop, String>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("Desktop readiness worker disconnected".into())),
        }
    }
}

fn detect() -> Result<Desktop, String> {
    let desktop = Desktop::current()?;
    let _apartment = luciddesk_shell::ShellApartment::initialize_sta().map_err(|e| e.to_string())?;
    // Reads view metrics and a consistent pair of item-ID snapshots, without
    // resolving file metadata, loading icons or attaching the desktop hook.
    let revision = luciddesk_shell::NativeDesktopReader::default().revision()?;
    desktop.validate()?;
    if unsafe { GetParent(desktop.view as _) } as isize != revision.view_window() {
        return Err("Shell interface and desktop window do not match; retrying".into());
    }
    Ok(desktop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_probe_is_nonblocking_and_preserves_result() {
        let (sender, receiver) = mpsc::channel();
        let probe = Probe(receiver);
        assert!(probe.poll().is_none());
        let desktop = Desktop { view: 42, process: 7 };
        sender.send(Ok(desktop)).unwrap();
        assert_eq!(probe.poll(), Some(Ok(desktop)));
    }

    #[test]
    fn probe_reports_failure_and_worker_disconnect() {
        let (sender, receiver) = mpsc::channel();
        let probe = Probe(receiver);
        sender.send(Err("Shell not ready".into())).unwrap();
        assert_eq!(probe.poll(), Some(Err("Shell not ready".into())));
        drop(sender);
        assert!(probe.poll().unwrap().is_err());
    }

    #[test]
    fn stale_desktop_cannot_pass_validation() {
        assert!(Desktop { view: 0, process: 0 }.validate().is_err());
    }

    #[test]
    #[ignore = "requires the interactive Explorer desktop"]
    fn real_desktop_interfaces_are_ready() {
        detect().unwrap().validate().unwrap();
    }
}
