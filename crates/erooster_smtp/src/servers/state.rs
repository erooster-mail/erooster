// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use mail_auth::SpfOutput;

/// State of the connection session between us and the Client
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Connection {
    pub state: State,
    pub secure: bool,
    pub require_tls: bool,
    /// True when the connection arrived on a submission port (587 or 465).
    /// Submission connections must authenticate before sending MAIL FROM.
    pub is_submission: bool,
    pub receipts: Option<Vec<String>>,
    pub sender: Option<String>,
    pub ehlo: Option<String>,
    pub peer_addr: String,
    pub spf_result: Option<SpfOutput>,
    /// Client-declared message size from `MAIL FROM` SIZE= parameter (bytes).
    pub declared_size: Option<u64>,
}

impl Connection {
    pub const fn new(secure: bool, is_submission: bool, peer_addr: String) -> Self {
        Connection {
            secure,
            require_tls: false,
            is_submission,
            state: State::NotAuthenticated,
            receipts: None,
            sender: None,
            ehlo: None,
            peer_addr,
            spf_result: None,
            declared_size: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum State {
    /// Initial State
    NotAuthenticated,
    /// DATA command issued, if not None this means we were authenticated
    ReceivingData((Option<String>, Data)),
    /// Authentication in progress
    Authenticating(AuthState),
    /// Authentication done
    Authenticated(String),
}

#[derive(Clone)]
pub struct Data(pub Vec<u8>);

impl std::fmt::Debug for Data {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // No-Op
        f.debug_struct("Data").finish()
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone)]
pub enum AuthState {
    Plain,
    Username,
    Password(String),
}
