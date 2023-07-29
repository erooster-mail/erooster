use crate::commands::{CommandData, Data};
use color_eyre::eyre::ContextCompat;
use erooster_core::backend::storage::{MailEntry, MailStorage, Storage};
use futures::{Sink, SinkExt};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{debug, instrument};

pub struct Search<'a> {
    pub data: &'a Data,
}

impl Search<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        storage: Arc<Storage>,
        command_data: &CommandData<'_>,
        uid: bool,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        debug!("Search command_data: {:#?}", command_data);
        lines
            .send(format!("{} BAD Not supported", command_data.tag))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}
