use std::sync::Arc;

use tokio::sync::RwLock;

/// State of the connection session between us and the Client
#[derive(Debug)]
pub struct Connection {
    pub state: State,
    pub secure: bool,
    pub data: Option<String>,
    pub receipts: Option<Vec<String>>,
    pub sender: Option<String>,
    pub ehlo: Option<String>,
    pub peer_addr: String,
}

impl Connection {
    pub fn new(secure: bool, peer_addr: String) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Connection {
            secure,
            state: State::NotAuthenticated,
            data: None,
            receipts: None,
            sender: None,
            ehlo: None,
            peer_addr,
        }))
    }
}

#[derive(Debug, Clone)]
pub enum State {
    /// Initial State
    NotAuthenticated,
    /// DATA command issued, if not None this means we were authenticated
    ReceivingData(Option<String>),
    /// Authentication in progress
    Authenticating(AuthState),
    /// Authentication done
    Authenticated(String),
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone)]
pub enum AuthState {
    Plain,
    Username,
    Password(String),
}
