use std::{thread, time::Duration};

pub(crate) fn wait() -> Result<bool, String> {
    wait_with(
        luciddesk_explorer::desktop_component_released,
        Duration::from_millis(250),
    )
}

fn wait_with(
    mut probe: impl FnMut() -> Result<bool, String>,
    interval: Duration,
) -> Result<bool, String> {
    let mut status = probe();
    for _ in 1..40 {
        if status == Ok(true) {
            break;
        }
        thread::sleep(interval);
        status = probe();
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_busy_and_transient_inspection_failures() {
        let mut statuses = [Ok(false), Err("Explorer restarting".into()), Ok(true)].into_iter();
        assert_eq!(
            wait_with(|| statuses.next().unwrap(), Duration::ZERO),
            Ok(true)
        );
        assert!(statuses.next().is_none());

        for status in [Ok(false), Err("Could not inspect Explorer".into())] {
            let mut calls = 0;
            assert_eq!(
                wait_with(
                    || {
                        calls += 1;
                        status.clone()
                    },
                    Duration::ZERO
                ),
                status
            );
            assert_eq!(calls, 40);
        }
    }
}
