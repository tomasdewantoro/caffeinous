use std::fmt;
use std::io;
use std::process::ExitStatus;

#[derive(Debug)]
pub enum Error {
    /// IOKit returned a non-success `IOReturn`.
    IoKit { call: &'static str, code: i32 },
    /// A helper process could not be started or its files could not be read/written.
    Io(io::Error),
    /// A helper process ran and failed.
    Command {
        command: String,
        status: ExitStatus,
        stderr: String,
    },
    /// The user dismissed the administrator password prompt.
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoKit { call, code } => write!(f, "{call} failed with IOReturn {code:#010x}"),
            Self::Io(e) => e.fmt(f),
            Self::Command {
                command,
                status,
                stderr,
            } => {
                write!(f, "`{command}` failed ({status})")?;
                let stderr = stderr.trim();
                if !stderr.is_empty() {
                    write!(f, ": {stderr}")?;
                }
                Ok(())
            }
            Self::Cancelled => f.write_str("administrator authorization was cancelled"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
