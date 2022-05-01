use std::sync::Arc;

use tokio::sync::RwLock;

/// State of the connection session between us and the Client
#[derive(Debug, PartialEq, Clone)]
pub struct Connection {
    pub state: State,
    pub secure: bool,
    pub data: Option<String>,
    pub receivers: Option<Vec<String>>,
}

impl Connection {
    pub fn new(secure: bool) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Connection {
            secure,
            state: State::NotAuthenticated,
            data: None,
            receivers: None,
        }))
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum State {
    /// Initial State
    NotAuthenticated,
    /// DATA command issued
    ReceivingData,
    /// Authentication in progress
    Authenticating(AuthState),
    /// Authentication done
    Authenticated(String),
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AuthState {
    Username,
    Password(String),
}
