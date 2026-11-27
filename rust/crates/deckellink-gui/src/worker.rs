//! Background tasks: all port I/O runs in a worker thread. Workers never touch the UI;
//! they send events and the final outcome over a channel, the UI polls it every frame.

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use deckellink_core::appdata::StoredReceive;
use deckellink_core::clock::CancelToken;
use deckellink_core::diagnostics::{FormatGuess, LoopbackResult};
use deckellink_core::error::UserFacingError;
use deckellink_core::parameters::archive::StoredParameterSet;
use deckellink_core::transfer::receiver::ReceivedProgram;
use deckellink_core::transfer::{Event, EventSink, ReceiveResult, SendResult};

/// Kind of a running task (one at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Send,
    DripFeed,
    Receive,
    ParamRead,
    ParamWrite,
    Loopback,
    FormatDetect,
}

impl TaskKind {
    pub fn label(self) -> &'static str {
        match self {
            TaskKind::Send => "Senden",
            TaskKind::DripFeed => "Nachladebetrieb",
            TaskKind::Receive => "Empfangen",
            TaskKind::ParamRead => "Parameter auslesen",
            TaskKind::ParamWrite => "Parameter schreiben",
            TaskKind::Loopback => "Loopback-Test",
            TaskKind::FormatDetect => "Datenformat mitlesen",
        }
    }
}

/// Final result of a task.
#[derive(Debug)]
pub enum Outcome {
    Sent(SendResult),
    Received {
        result: ReceiveResult,
        program: Option<ReceivedProgram>,
        stored: Result<Option<StoredReceive>, String>,
    },
    ParamsRead {
        result: ReceiveResult,
        program: Option<ReceivedProgram>,
        stored: Result<Option<StoredParameterSet>, String>,
    },
    ParamsWritten(SendResult),
    Loopback(LoopbackResult),
    Format(Result<(usize, FormatGuess), UserFacingError>),
}

#[derive(Debug)]
pub enum WorkerMsg {
    Event(Event),
    Done(Box<Outcome>),
}

/// Forwards events to the UI thread and wakes it up.
#[derive(Debug)]
pub struct ChannelSink {
    tx: Sender<WorkerMsg>,
    ctx: egui::Context,
}

impl EventSink for ChannelSink {
    fn emit(&mut self, event: Event) {
        // A closed channel only means the UI is gone; the task still ends normally.
        let _ = self.tx.send(WorkerMsg::Event(event));
        self.ctx.request_repaint();
    }
}

/// A running background task.
#[derive(Debug)]
pub struct RunningTask {
    pub kind: TaskKind,
    pub token: CancelToken,
    rx: Receiver<WorkerMsg>,
    handle: Option<JoinHandle<()>>,
}

impl RunningTask {
    /// Start `job` in a new thread. A panic inside the job is reported as a failed task, never crashes the UI.
    pub fn spawn<F>(kind: TaskKind, ctx: &egui::Context, job: F) -> Result<Self, String>
    where
        F: FnOnce(&CancelToken, &mut dyn EventSink) -> Outcome + Send + 'static,
    {
        let (tx, rx) = channel();
        let token = CancelToken::new();
        let worker_token = token.clone();
        let mut sink = ChannelSink { tx: tx.clone(), ctx: ctx.clone() };
        let wake = ctx.clone();
        let handle = std::thread::Builder::new()
            .name(format!("deckellink-{}", kind.label()))
            .spawn(move || {
                let outcome = job(&worker_token, &mut sink);
                let _ = tx.send(WorkerMsg::Done(Box::new(outcome)));
                wake.request_repaint();
            })
            .map_err(|e| format!("Hintergrundvorgang konnte nicht gestartet werden: {e}"))?;
        Ok(Self { kind, token, rx, handle: Some(handle) })
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Drain all pending messages. `Err` if the worker ended without a result (internal error).
    pub fn poll(&mut self) -> (Vec<Event>, Option<Result<Outcome, String>>) {
        let mut events = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(WorkerMsg::Event(event)) => events.push(event),
                Ok(WorkerMsg::Done(outcome)) => {
                    self.join();
                    return (events, Some(Ok(*outcome)));
                }
                Err(TryRecvError::Empty) => return (events, None),
                Err(TryRecvError::Disconnected) => {
                    self.join();
                    return (
                        events,
                        Some(Err(format!(
                            "{} wurde unerwartet beendet (interner Fehler). Bitte Protokoll prüfen und Vorgang wiederholen.",
                            self.kind.label()
                        ))),
                    );
                }
            }
        }
    }

    fn join(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RunningTask {
    fn drop(&mut self) {
        // Closing the application cancels the transfer (the CNC is never left waiting silently).
        self.token.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckellink_core::transfer::SendState;

    #[test]
    fn panicking_job_is_reported_not_propagated() {
        let ctx = egui::Context::default();
        #[allow(clippy::panic)]
        let mut task = RunningTask::spawn(TaskKind::Send, &ctx, |_, _| panic!("boom")).unwrap();
        let mut outcome = None;
        for _ in 0..200 {
            let (_, done) = task.poll();
            if done.is_some() {
                outcome = done;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(matches!(outcome, Some(Err(message)) if message.contains("unerwartet")));
    }

    #[test]
    fn events_then_outcome() {
        let ctx = egui::Context::default();
        let mut task = RunningTask::spawn(TaskKind::Receive, &ctx, |token, sink| {
            sink.emit(Event::SendState(SendState::Sending));
            assert!(!token.is_cancelled());
            Outcome::ParamsWritten(crate::worker::tests::dummy_result())
        })
        .unwrap();
        let mut events = Vec::new();
        for _ in 0..200 {
            let (mut new, done) = task.poll();
            events.append(&mut new);
            if done.is_some() {
                assert!(matches!(done, Some(Ok(Outcome::ParamsWritten(_)))));
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(events.len(), 1);
    }

    pub(crate) fn dummy_result() -> SendResult {
        SendResult {
            state: SendState::Completed,
            bytes_total: 0,
            bytes_sent: 0,
            started_at: chrono::Local::now().fixed_offset(),
            duration_s: 0.0,
            sha256: String::new(),
            profile_id: String::new(),
            port: String::new(),
            simulated: true,
            error: None,
            port_left_open: false,
        }
    }
}
