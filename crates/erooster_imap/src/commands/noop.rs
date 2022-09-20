use std::sync::Arc;

use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use color_eyre::eyre::ContextCompat;
use erooster_core::backend::storage::{MailEntryType, MailStorage, Storage};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::instrument;

pub struct Noop<'a> {
    pub data: &'a Data,
}

impl Noop<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        // TODO return status as suggested in https://www.rfc-editor.org/rfc/rfc9051.html#name-noop-command
        if let State::Selected(folder, _) = &self.data.con_state.read().await.state {
            let folder = folder.replace('/', ".");
            let mailbox_path = storage.to_ondisk_path(
                folder.clone(),
                self.data
                    .con_state
                    .read()
                    .await
                    .username
                    .clone()
                    .context("Username missing in internal State")?,
            )?;
            let mails: Vec<MailEntryType> = storage.list_new(&mailbox_path).await;
            let got_new = !mails.is_empty();
            if got_new {
                let mails: Vec<MailEntryType> = storage.list_all(&mailbox_path).await;
                lines.send(format!("* {} EXISTS", mails.len())).await?;
            }
        }
        lines
            .send(format!("{} OK NOOP completed", command_data.tag))
            .await?;
        Ok(())
    }
}
