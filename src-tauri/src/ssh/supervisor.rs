use std::future::{pending, Future};
use std::sync::Arc;
use std::time::Duration;

use russh::client;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{Instant, Interval, MissedTickBehavior};

use crate::connection_log;

pub(crate) struct FixedKeepaliveSchedule {
    interval: Option<Interval>,
    ticks: u64,
}

impl FixedKeepaliveSchedule {
    pub(crate) fn new(interval_secs: u64) -> Self {
        let interval = if interval_secs == 0 {
            None
        } else {
            let period = Duration::from_secs(interval_secs);
            let mut interval = tokio::time::interval_at(Instant::now() + period, period);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
            Some(interval)
        };
        Self { interval, ticks: 0 }
    }

    pub(crate) async fn tick(&mut self) {
        match self.interval.as_mut() {
            Some(interval) => {
                interval.tick().await;
                self.ticks = self.ticks.saturating_add(1);
            }
            None => pending::<()>().await,
        }
    }

    pub(crate) fn ticks(&self) -> u64 {
        self.ticks
    }
}

pub(crate) struct KeepaliveTask {
    cancel: Option<oneshot::Sender<String>>,
    task: Option<JoinHandle<()>>,
}

impl KeepaliveTask {
    pub(crate) async fn stop(mut self, reason: impl Into<String>) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(reason.into());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for KeepaliveTask {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send("owner dropped".to_string());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

pub(crate) fn spawn_keepalive_task<H>(
    session_id: String,
    interval_secs: u64,
    primary: Arc<client::Handle<H>>,
    secondary: Option<Arc<client::Handle<H>>>,
) -> Option<KeepaliveTask>
where
    H: client::Handler + Send + Sync + 'static,
    H::Error: std::fmt::Display,
{
    spawn_task(session_id, interval_secs, move || {
        let primary = primary.clone();
        let secondary = secondary.clone();
        async move {
            primary.send_keepalive(true).await?;
            if let Some(secondary) = secondary {
                secondary.send_keepalive(true).await?;
            }
            Ok(())
        }
    })
}

pub(crate) fn spawn_locked_keepalive_task<H>(
    session_id: String,
    interval_secs: u64,
    primary: Arc<Mutex<client::Handle<H>>>,
    secondary: Option<Arc<client::Handle<H>>>,
) -> Option<KeepaliveTask>
where
    H: client::Handler + Send + Sync + 'static,
    H::Error: std::fmt::Display,
{
    spawn_task(session_id, interval_secs, move || {
        let primary = primary.clone();
        let secondary = secondary.clone();
        async move {
            primary.lock().await.send_keepalive(true).await?;
            if let Some(secondary) = secondary {
                secondary.send_keepalive(true).await?;
            }
            Ok(())
        }
    })
}

fn spawn_task<F, Fut>(session_id: String, interval_secs: u64, send: F) -> Option<KeepaliveTask>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), russh::Error>> + Send + 'static,
{
    if interval_secs == 0 {
        connection_log::append(&session_id, "fixed keepalive disabled interval_secs=0");
        return None;
    }

    let (cancel, mut cancel_rx) = oneshot::channel::<String>();
    let task_session_id = session_id.clone();
    let task = tokio::spawn(async move {
        let mut schedule = FixedKeepaliveSchedule::new(interval_secs);
        connection_log::append(
            &task_session_id,
            format!("fixed keepalive started interval_secs={}", interval_secs),
        );
        loop {
            tokio::select! {
                reason = &mut cancel_rx => {
                    connection_log::append(
                        &task_session_id,
                        format!("fixed keepalive stopped reason={}", reason.unwrap_or_else(|_| "owner dropped".to_string())),
                    );
                    break;
                }
                _ = schedule.tick() => {
                    match send().await {
                        Ok(()) => connection_log::append(
                            &task_session_id,
                            format!("fixed keepalive sent sequence={}", schedule.ticks()),
                        ),
                        Err(error) => {
                            connection_log::append(
                                &task_session_id,
                                format!("fixed keepalive failed sequence={} error={}", schedule.ticks(), error),
                            );
                            break;
                        }
                    }
                }
            }
        }
    });

    Some(KeepaliveTask {
        cancel: Some(cancel),
        task: Some(task),
    })
}
