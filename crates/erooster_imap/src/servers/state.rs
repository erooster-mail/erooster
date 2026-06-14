// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::commands::{auth::AuthenticationMethod, parsers::DateTime};

/// State of the connection session between us and the Client
#[derive(Debug, Clone)]
pub struct Connection {
    pub state: State,
    pub secure: bool,
    pub username: Option<String>,
    pub active_capabilities: Vec<Capabilities>,
}

impl Connection {
    pub const fn new(secure: bool) -> Self {
        Connection {
            state: State::NotAuthenticated,
            secure,
            username: None,
            active_capabilities: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
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
}

#[derive(PartialEq, Eq, Clone)]
pub struct AppendingState {
    pub folder: String,
    pub flags: Option<Vec<String>>,
    pub datetime: Option<DateTime>,
    pub data: Option<Vec<u8>>,
    pub datalen: usize,
    pub tag: String,
    /// State to restore when APPEND completes (RFC 9051: APPEND does not change
    /// the selected mailbox).
    pub previous_state: Box<State>,
}

#[allow(clippy::missing_fields_in_debug)]
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
