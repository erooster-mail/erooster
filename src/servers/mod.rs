use std::{borrow::Cow, net::IpAddr};

use crate::commands::auth::AuthenticationMethod;

pub mod encrypted;
pub mod unencrypted;

#[derive(Debug, PartialEq)]
pub struct ConnectionState<'a> {
    pub state: State<'a>,
    pub ip: IpAddr,
    pub secure: bool,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum State<'a> {
    NotAuthenticated,
    Authenticating((AuthenticationMethod, Cow<'a, str>)),
    Authenticated,
    Selected(Cow<'a, str>),
}
