#[derive (Debug)]
enum Error {
    QueryStateError,
    String(&'static str),
    Io(io::Error),
    AddrParseError(net::AddrParseError)
}
use std::fmt;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::QueryStateError => write!(f, "QueryStateError"),
            Error::Io(ref err) => write!(f, "{:?}", err),
            Error::AddrParseError(ref e) => write!(f, "{:?}", e),
            Error::String(ref s) => write!(f, "{}", s)
        }
    }
}

impl From<io::Error> for Error {
    fn from(io: io::Error) -> Error {
        Error::Io(io)
    }
}

impl From<net::AddrParseError> for Error {
    fn from(a: net::AddrParseError) -> Error {
        Error::AddrParseError(a)
    }
}

impl From<&'static str> for Error {
    fn from(s: &'static str) -> Error {
        Error::String(s)
    }
}
