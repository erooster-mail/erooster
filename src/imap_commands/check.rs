use crate::{
    config::Config,
    imap_commands::{Command, Data},
    servers::state::State,
};
use async_trait::async_trait;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use std::sync::Arc;

pub struct Check<'a> {
    pub data: &'a Data,
}

#[async_trait]
impl<S> Command<S> for Check<'_>
where
    S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
{
    async fn exec(&mut self, lines: &mut S, _config: Arc<Config>) -> anyhow::Result<()> {
        // This is an Imap4rev1 feature. It does the same as Noop for us as we have no memory gc.
        // It also only is allowed in selected state
        if matches!(self.data.con_state.read().await.state, State::Selected(_)) {
            lines
                .send(format!(
                    "{} OK CHECK completed",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        } else {
            lines
                .send(format!(
                    "{} NO invalid state",
                    self.data.command_data.as_ref().unwrap().tag
                ))
                .await?;
        }
        Ok(())
    }
}
