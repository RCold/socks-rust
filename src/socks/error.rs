use std::{error, fmt, io};

#[derive(Debug)]
pub enum ErrorKind {
    IoError,
    VersionMismatch,
    NoAcceptableAuthMethods,
    AddressTypeNotSupported,
    CommandNotSupported,
    InvalidDomainName,
    FragmentationNotSupported,
    InvalidUdpPacketReceived,
}

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    cause: Option<io::Error>,
}

impl Error {
    pub fn new(kind: ErrorKind) -> Self {
        Self { kind, cause: None }
    }
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            ErrorKind::IoError => {
                write!(f, "{}", self.cause.as_ref().unwrap())
            }
            ErrorKind::VersionMismatch => write!(f, "version mismatch"),
            ErrorKind::NoAcceptableAuthMethods => write!(f, "no acceptable authentication methods"),
            ErrorKind::AddressTypeNotSupported => write!(f, "address type not supported"),
            ErrorKind::CommandNotSupported => write!(f, "command not supported"),
            ErrorKind::InvalidDomainName => write!(f, "invalid domain name"),
            ErrorKind::FragmentationNotSupported => write!(f, "fragmentation not supported"),
            ErrorKind::InvalidUdpPacketReceived => write!(f, "invalid udp packet received"),
        }
    }
}

impl error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self {
            kind: ErrorKind::IoError,
            cause: Some(err),
        }
    }
}
