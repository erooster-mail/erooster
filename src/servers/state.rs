use std::net::IpAddr;

use crate::imap_commands::auth::AuthenticationMethod;

/// State of the connection session between us and the Client
#[derive(Debug, PartialEq, Clone)]
pub struct Connection {
    pub state: State,
    pub secure: bool,
    pub username: Option<String>,
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
