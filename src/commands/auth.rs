use async_trait::async_trait;
use futures::{Sink, SinkExt};
use simdutf8::compat::from_utf8;
use std::{borrow::Cow, io};
use tracing::{debug, error};

use crate::{
    commands::{Command, Data},
    line_codec::LinesCodecError,
    servers::State,
};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AuthenticationMethod {
    Plain,
}

pub struct Plain<'a, 'b> {
    pub data: Data<'a, 'b>,
    pub auth_data: Cow<'a, str>,
}

#[async_trait]
impl<S> Command<S> for Plain<'_, '_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &'async_trait mut S) -> anyhow::Result<()> {
        debug!("auth_data: {}", self.auth_data);
        let bytes = base64::decode(self.auth_data.as_bytes());
        match bytes {
            Ok(bytes) => {
                let auth_data_vec: Vec<&str> = bytes
                    .split(|x| *x == b'\0')
                    .filter_map(|string| {
                        let new_string = from_utf8(string);
                        if let Ok(new_string) = new_string {
                            return Some(new_string);
                        }
                        None
                    })
                    .collect();
                if auth_data_vec.len() == 3 {
                    // TODO remove with actual implementation.
                    // This exists so the compiler knows that collect is justified here
                    let _guard = auth_data_vec.get(0);

                    // TODO check against DB
                    {
                        self.data.con_state.state = State::Authenticated;
                    };
                    let secure = { self.data.con_state.secure };
                    if secure {
                        lines
                            .send(format!("{} OK Success (tls protection)", self.data.tag))
                            .await?;
                    } else {
                        lines
                            .send(format!("{} OK Success (unprotected)", self.data.tag))
                            .await?;
                    }
                } else {
                    {
                        self.data.con_state.state = State::NotAuthenticated;
                    };
                    lines
                        .send(format!("{} BAD Invalid arguments", self.data.tag))
                        .await?;
                }
            }
            Err(e) => {
                {
                    self.data.con_state.state = State::NotAuthenticated;
                };
                error!("Error logging in: {}", e);
                lines
                    .send(format!("{} BAD Invalid arguments", self.data.tag))
                    .await?;
                return Ok(());
            }
        }

        Ok(())
    }
}
