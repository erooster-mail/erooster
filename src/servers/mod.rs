use std::net::IpAddr;

use crate::commands::auth::AuthenticationMethod;

pub mod encrypted;
pub mod unencrypted;

#[derive(Debug, PartialEq, Clone)]
pub struct ConnectionState {
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
