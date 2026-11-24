use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    /// IOKit returned a non-success `IOReturn`.
    IoKit { call: &'static str, code: i32 },
    /// A file or helper process could not be read, written or started.
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoKit { call, code } => write!(f, "{call} failed with IOReturn {code:#010x}"),
            Self::Io(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::IoKit { .. } => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
