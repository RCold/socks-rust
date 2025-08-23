use crate::socks::error::Error;

#[repr(u8)]
pub enum Command {
    Connect = 0x01u8,
    Bind = 0x02u8,
    UdpAssociate = 0x03u8,
}

impl TryInto<Command> for u8 {
    type Error = Error;

    fn try_into(self) -> Result<Command, Self::Error> {
        if self == Command::Connect as Self {
            Ok(Command::Connect)
        } else if self == Command::Bind as Self {
            Ok(Command::Bind)
        } else if self == Command::UdpAssociate as Self {
            Ok(Command::UdpAssociate)
        } else {
            Err(Error::CommandNotSupported)
        }
    }
}
