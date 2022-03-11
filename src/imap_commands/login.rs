use std::{io, sync::Arc};

use async_trait::async_trait;
use futures::{Sink, SinkExt};

use crate::{
    imap_commands::{Command, Data},
    config::Config,
    line_codec::LinesCodecError,
};

pub struct Login<'a> {
    pub data: &'a Data<'a>,
}

#[async_trait]
impl<S> Command<S> for Login<'_>
where
    S: Sink<String, Error = LinesCodecError> + std::marker::Unpin + std::marker::Send,
    S::Error: From<io::Error>,
{
    async fn exec(&mut self, lines: &mut S, _config: Arc<Config>) -> anyhow::Result<()> {
        lines
            .send(format!(
                "{} NO LOGIN COMMAND DISABLED FOR SECURITY. USE AUTH",
                self.data.command_data.as_ref().unwrap().tag
            ))
            .await?;
        Ok(())
    }
}
