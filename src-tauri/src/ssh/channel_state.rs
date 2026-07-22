#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TerminalCause {
    RemoteEof,
    RemoteClose,
    ExitStatus(u32),
    ExitSignal(String),
    StreamEnded,
    ApplicationClosed,
    TransportError(String),
}

impl TerminalCause {
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::RemoteEof => "remote sent EOF".to_string(),
            Self::RemoteClose => "remote sent channel close".to_string(),
            Self::ExitStatus(status) => format!("remote sent exit status {}", status),
            Self::ExitSignal(signal) => format!("remote sent exit signal {}", signal),
            Self::StreamEnded => "SSH channel message stream ended".to_string(),
            Self::ApplicationClosed => "closed by application".to_string(),
            Self::TransportError(error) => format!("SSH channel transport error: {}", error),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ChannelLifecycle {
    terminal: Option<TerminalCause>,
}

impl ChannelLifecycle {
    pub(crate) fn can_write(&self) -> bool {
        self.terminal.is_none()
    }

    pub(crate) fn terminate(&mut self, cause: TerminalCause) -> bool {
        if self.terminal.is_some() {
            false
        } else {
            self.terminal = Some(cause);
            true
        }
    }

    pub(crate) fn cause(&self) -> Option<&TerminalCause> {
        self.terminal.as_ref()
    }
}
