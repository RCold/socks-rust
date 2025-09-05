use std::{error, fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    VersionMismatch,
    InvalidAddressType,
    InvalidCommand,
    InvalidDomainName,
    FragmentationNotSupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;
        match self {
            Io(err) => err.fmt(f),
            VersionMismatch => f.write_str("version mismatch"),
            InvalidAddressType => f.write_str("invalid address type"),
            InvalidCommand => f.write_str("invalid command"),
            InvalidDomainName => f.write_str("invalid domain name"),
            FragmentationNotSupported => f.write_str("fragmentation not supported"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}
