//! Binary output is pulled only as fast as xterm consumes it. SSH readers stop at
//! the high water mark, while their input, resize and transfer controls stay live.
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Notify;

const HIGH_WATER: usize = 2 * 1024 * 1024;
const LOW_WATER: usize = 512 * 1024;
const READ_SIZE: usize = 128 * 1024;

#[derive(Default)]
struct Buffer {
    chunks: VecDeque<Vec<u8>>,
    offset: usize,
    bytes: usize,
    paused: bool,
}

impl Buffer {
    fn push(&mut self, data: Vec<u8>) -> bool {
        let was_empty = self.bytes == 0;
        self.bytes += data.len();
        self.paused |= self.bytes >= HIGH_WATER;
        self.chunks.push_back(data);
        was_empty
    }

    fn take(&mut self) -> Vec<u8> {
        let mut output = Vec::with_capacity(self.bytes.min(READ_SIZE));
        while output.len() < READ_SIZE {
            let Some(chunk) = self.chunks.front() else {
                break;
            };
            let amount = (chunk.len() - self.offset).min(READ_SIZE - output.len());
            output.extend_from_slice(&chunk[self.offset..self.offset + amount]);
            self.offset += amount;
            self.bytes -= amount;
            if self.offset == chunk.len() {
                self.chunks.pop_front();
                self.offset = 0;
            }
        }
        if self.bytes <= LOW_WATER {
            self.paused = false;
        }
        output
    }
}

#[derive(Default)]
struct Pipe {
    buffer: Mutex<Buffer>,
    drained: Notify,
}

#[derive(Default)]
pub struct TerminalOutputState(Mutex<HashMap<String, Arc<Pipe>>>);

fn pipe(app: &AppHandle, id: &str) -> Option<Arc<Pipe>> {
    app.state::<TerminalOutputState>()
        .0
        .lock()
        .unwrap()
        .get(id)
        .cloned()
}

pub fn emit(app: &AppHandle, id: &str, data: Vec<u8>) {
    if data.is_empty() {
        return;
    }
    if let Some(pipe) = pipe(app, id) {
        if pipe.buffer.lock().unwrap().push(data) {
            let _ = app.emit(&format!("terminal-output-ready-{id}"), ());
        }
    } else {
        // Serial and terminals not yet mounted retain the existing event path.
        let _ = app.emit(&format!("ssh-data-{id}"), data);
    }
}

pub fn can_read(app: &AppHandle, id: &str) -> bool {
    pipe(app, id).is_none_or(|p| !p.buffer.lock().unwrap().paused)
}

pub async fn wait_for_capacity(app: &AppHandle, id: &str) {
    let Some(p) = pipe(app, id) else { return };
    loop {
        let notified = p.drained.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !p.buffer.lock().unwrap().paused {
            return;
        }
        notified.await;
    }
}

#[tauri::command]
pub fn attach_terminal_output(session_id: String, state: State<TerminalOutputState>) {
    state.0.lock().unwrap().entry(session_id).or_default();
}

#[tauri::command]
pub fn read_terminal_output(
    session_id: String,
    state: State<TerminalOutputState>,
) -> tauri::ipc::Response {
    let p = state.0.lock().unwrap().get(&session_id).cloned();
    let bytes = p.map_or_else(Vec::new, |p| {
        let bytes = p.buffer.lock().unwrap().take();
        p.drained.notify_waiters();
        bytes
    });
    tauri::ipc::Response::new(bytes)
}

#[tauri::command]
pub fn detach_terminal_output(session_id: String, state: State<TerminalOutputState>) {
    if let Some(p) = state.0.lock().unwrap().remove(&session_id) {
        // Wake a paused reader even if the UI was closed while full.
        *p.buffer.lock().unwrap() = Buffer::default();
        p.drained.notify_waiters();
    }
}
