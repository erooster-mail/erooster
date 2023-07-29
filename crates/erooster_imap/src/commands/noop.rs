use std::sync::Arc;

use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use color_eyre::eyre::ContextCompat;
use erooster_core::backend::storage::{MailEntryType, MailStorage, Storage};
use futures::{Sink, SinkExt};
use tracing::instrument;

pub struct Noop<'a> {
    pub data: &'a Data,
}

impl Noop<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        // TODO: return status as suggested in https://www.rfc-editor.org/rfc/rfc9051.html#name-noop-command
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

            let mails = storage.count_cur(&mailbox_path) + storage.count_new(&mailbox_path);
            lines.send(format!("* {mails} EXISTS")).await?;
        }
        lines
            .send(format!("{} OK NOOP completed", command_data.tag))
            .await?;
        Ok(())
    }
}
