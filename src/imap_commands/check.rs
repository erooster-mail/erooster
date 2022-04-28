use crate::{
    imap_commands::{CommandData, Data},
    imap_servers::state::State,
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};

pub struct Check<'a> {
    pub data: &'a Data,
}

impl Check<'_> {
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        // This is an Imap4rev1 feature. It does the same as Noop for us as we have no memory gc.
        // It also only is allowed in selected state
        if matches!(
            self.data.con_state.read().await.state,
            State::Selected(_, _)
        ) {
            lines
                .send(format!("{} OK CHECK completed", command_data.tag))
                .await?;
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }
}
