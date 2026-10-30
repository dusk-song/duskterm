use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Interrupt only pre-commit/data I/O. File promotion and rollback must run to completion.
/// The shared cancellation flag also serves native file-drag consumers, so keep that
/// contract and poll it while an otherwise stalled I/O future is pending.
pub(super) async fn transfer_step<T>(
    cancel: &Arc<AtomicBool>,
    future: impl Future<Output = T>,
) -> Result<T, String> {
    let cancelled = async {
        loop {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::select! {
        biased;
        _ = cancelled => Err("Transfer cancelled".to_string()),
        result = future => Ok(result),
    }
}

/// Report long waits without logging file paths, payloads or connection configuration.
pub(super) async fn traced_transfer_step<T>(
    cancel: &Arc<AtomicBool>,
    future: impl Future<Output = T>,
    session_id: &str,
    stage: &'static str,
) -> Result<T, String> {
    observe_transfer_step(cancel, future, Duration::from_secs(5), |elapsed| {
        crate::connection_log::append(
            session_id,
            format!(
                "sftp upload waiting stage={stage} elapsed_ms={}",
                elapsed.as_millis()
            ),
        );
    })
    .await
}

async fn observe_transfer_step<T>(
    cancel: &Arc<AtomicBool>,
    future: impl Future<Output = T>,
    period: Duration,
    mut report: impl FnMut(Duration),
) -> Result<T, String> {
    let operation = transfer_step(cancel, future);
    tokio::pin!(operation);
    let started = tokio::time::Instant::now();
    let mut ticks = tokio::time::interval_at(started + period, period);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            result = &mut operation => return result,
            _ = ticks.tick() => report(started.elapsed()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn stalled_io_reports_without_restarting_and_still_cancels() {
        let cancel = Arc::new(AtomicBool::new(false));
        let trigger = cancel.clone();
        let mut reports = 0;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            trigger.store(true, Ordering::Relaxed);
        });
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            observe_transfer_step(
                &cancel,
                std::future::pending::<()>(),
                Duration::from_millis(10),
                |_| reports += 1,
            ),
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap_err(), "Transfer cancelled");
        assert!(reports > 0);
    }

    #[tokio::test]
    async fn cancellation_interrupts_stalled_reads_and_writes() {
        for writing in [false, true] {
            let cancel = Arc::new(AtomicBool::new(false));
            let trigger = cancel.clone();
            let (_peer, mut stream) = tokio::io::duplex(1);
            let operation = async {
                if writing {
                    stream.write_all(&[1; 128]).await.map(|_| ())
                } else {
                    stream.read(&mut [0; 1]).await.map(|_| ())
                }
            };
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                trigger.store(true, Ordering::Relaxed);
            });
            let result =
                tokio::time::timeout(Duration::from_secs(1), transfer_step(&cancel, operation))
                    .await
                    .expect("cancellation must not wait for the network timeout");
            assert_eq!(result.unwrap_err(), "Transfer cancelled");
        }
    }

    #[tokio::test]
    async fn cancellation_before_start_does_not_poll_the_operation() {
        let cancel = Arc::new(AtomicBool::new(true));
        let result = transfer_step(&cancel, async { panic!("cancelled work was polled") }).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn completed_io_and_errors_are_preserved() {
        let cancel = Arc::new(AtomicBool::new(false));
        assert_eq!(
            transfer_step(&cancel, async { Ok::<_, &str>(7) })
                .await
                .unwrap(),
            Ok(7)
        );
        assert_eq!(
            transfer_step(&cancel, async { Err::<u8, _>("disk error") })
                .await
                .unwrap(),
            Err("disk error")
        );
    }
}
