use crate::commands::{auth::AuthenticationMethod, parsers::DateTime};
use std::sync::Arc;
use tokio::sync::RwLock;

/// State of the connection session between us and the Client
#[derive(Debug)]
pub struct Connection {
    pub state: State,
    pub secure: bool,
    pub username: Option<String>,
    pub active_capabilities: Vec<Capabilities>,
}

impl Connection {
    pub fn new(secure: bool) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Connection {
            state: State::NotAuthenticated,
            secure,
            username: None,
            active_capabilities: vec![],
        }))
    }
}

#[derive(Debug, Clone)]
pub enum Capabilities {
    UTF8,
    Other(String),
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
    /// An Email is getting appended to the folder
    /// Fields are left to right:
    /// - folder
    /// - flags
    /// - datetime
    Appending(AppendingState),
    /// We received all data for the email but there may be misc data coming
    GotAppendData,
}

#[derive(PartialEq, Eq, Clone)]
pub struct AppendingState {
    pub folder: String,
    pub flags: Option<Vec<String>>,
    pub datetime: Option<DateTime>,
    pub data: Option<Vec<u8>>,
    pub datalen: usize,
    pub tag: String,
}

impl std::fmt::Debug for AppendingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppendingState")
            .field("folder", &self.folder)
            .field("flags", &self.flags)
            .field("datetime", &self.datetime)
            .field("datalen", &self.datalen)
            .field("tag", &self.tag)
            .finish()
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}
