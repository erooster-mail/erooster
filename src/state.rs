use std::net::IpAddr;

use crate::commands::auth::AuthenticationMethod;

/// State of the connection session between us and the Client
#[derive(Debug, PartialEq, Clone)]
pub struct Connection {
    pub state: State,
    pub ip: IpAddr,
    pub secure: bool,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum State {
    NotAuthenticated,
    Authenticating((AuthenticationMethod, String)),
    Authenticated,
    Selected(String),
}
