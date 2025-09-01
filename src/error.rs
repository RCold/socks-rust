use std::{error, fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    VersionMismatch,
    NoAcceptableAuthMethods,
    AddressTypeNotSupported,
    CommandNotSupported,
    InvalidDomainName,
    FragmentationNotSupported,
    InvalidUdpPacketReceived,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;
        match self {
            Io(err) => err.fmt(f),
            VersionMismatch => f.write_str("version mismatch"),
            NoAcceptableAuthMethods => f.write_str("no acceptable authentication methods"),
            AddressTypeNotSupported => f.write_str("address type not supported"),
            CommandNotSupported => f.write_str("command not supported"),
            InvalidDomainName => f.write_str("invalid domain name"),
            FragmentationNotSupported => f.write_str("fragmentation not supported"),
            InvalidUdpPacketReceived => f.write_str("invalid udp packet received"),
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
