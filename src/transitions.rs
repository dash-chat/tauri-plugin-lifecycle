//! Runs the handlers one at a time, in the order the OS reports the
//! transitions. A transition that arrives while a handler is still running
//! drops it, stopping it at its current `.await`, before the next one starts.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

use tokio::sync::mpsc;

pub(crate) type Handler = Pin<Box<dyn Future<Output = ()> + Send>>;

static TRANSITIONS: OnceLock<mpsc::UnboundedSender<Handler>> = OnceLock::new();

pub(crate) fn start() {
    let (tx, rx) = mpsc::unbounded_channel();
    if TRANSITIONS.set(tx).is_ok() {
        tauri::async_runtime::spawn(run_in_order(rx));
    }
}

/// Queue `handler` behind every transition reported before it.
pub(crate) fn push(handler: Handler) {
    if let Some(transitions) = TRANSITIONS.get() {
        let _ = transitions.send(handler);
    }
}

async fn run_in_order(mut transitions: mpsc::UnboundedReceiver<Handler>) {
    let mut running: Option<Handler> = None;
    loop {
        let next = match running.as_mut() {
            Some(handler) => tokio::select! {
                biased;
                next = transitions.recv() => next,
                () = handler => {
                    running = None;
                    continue;
                }
            },
            None => transitions.recv().await,
        };
        match next {
            Some(next) => running = Some(next),
            None => {
                if let Some(handler) = running.take() {
                    handler.await;
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;

    fn record(log: &Arc<Mutex<Vec<&'static str>>>, entry: &'static str, wait: Duration) -> Handler {
        let log = log.clone();
        Box::pin(async move {
            log.lock().unwrap().push(entry);
            tokio::time::sleep(wait).await;
            log.lock().unwrap().push("finished");
        })
    }

    #[tokio::test]
    async fn runs_handlers_in_the_order_they_were_pushed() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::unbounded_channel();
        let running = tokio::spawn(run_in_order(rx));

        tx.send(record(&log, "pause", Duration::ZERO)).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        tx.send(record(&log, "resume", Duration::ZERO)).unwrap();
        drop(tx);
        running.await.unwrap();

        assert_eq!(
            *log.lock().unwrap(),
            ["pause", "finished", "resume", "finished"]
        );
    }

    #[tokio::test]
    async fn a_new_transition_drops_the_handler_still_running() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::unbounded_channel();
        let running = tokio::spawn(run_in_order(rx));

        tx.send(record(&log, "pause", Duration::from_secs(60)))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        tx.send(record(&log, "resume", Duration::ZERO)).unwrap();
        drop(tx);
        running.await.unwrap();

        assert_eq!(*log.lock().unwrap(), ["pause", "resume", "finished"]);
    }
}
