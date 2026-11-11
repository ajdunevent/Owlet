use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Transport failure: DNS, TLS, refused connection, timeout. Owlet treats it as offline.
    Network(String),
    /// The token was rejected or revoked, or the password was wrong.
    Unauthorized,
    /// The server refused the request, such as deleting the default list.
    Refused(String),
    NotFound,
    Http {
        status: u32,
        message: String,
    },
    /// Response that does not match the 0.7.10 API shape.
    Protocol(String),
    InvalidUrl(String),
    /// The address answered, but not as a KitchenOwl server.
    NotKitchenOwl,
    ServerTooOld {
        build: u32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::Unauthorized => write!(f, "not authorized"),
            Self::Refused(msg) => write!(f, "refused: {msg}"),
            Self::NotFound => write!(f, "not found"),
            Self::Http { status, message } => write!(f, "HTTP {status}: {message}"),
            Self::Protocol(msg) => write!(f, "unexpected response: {msg}"),
            Self::InvalidUrl(url) => write!(f, "invalid address: {url}"),
            Self::NotKitchenOwl => write!(f, "not a KitchenOwl server"),
            Self::ServerTooOld { build } => write!(f, "KitchenOwl build {build} is too old"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn is_network(&self) -> bool {
        matches!(self, Self::Network(_))
    }
}

impl From<glib::Error> for Error {
    fn from(err: glib::Error) -> Self {
        Self::Network(err.message().to_owned())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Protocol(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
