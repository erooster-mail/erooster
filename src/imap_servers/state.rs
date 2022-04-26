use crate::imap_commands::auth::AuthenticationMethod;
use std::sync::Arc;
use tokio::sync::RwLock;

/// State of the connection session between us and the Client
#[derive(Debug, PartialEq, Clone)]
pub struct Connection {
    pub state: State,
    pub secure: bool,
    pub username: Option<String>,
}

impl Connection {
    pub fn new(secure: bool) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Connection {
            state: State::NotAuthenticated,
            secure,
            username: None,
        }))
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum State {
    /// Initial State
    NotAuthenticated,
    /// Auth in progress
    Authenticating(AuthenticationMethod, String),
    /// Auth successful
    Authenticated,
    /// Folder selected
    Selected(String, Access),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}
