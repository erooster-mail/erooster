use crate::commands::{fetch::Fetch, store::Store, CommandData, Data};
use erooster_core::backend::storage::Storage;
use futures::{Sink, SinkExt};
use std::sync::Arc;
use tracing::instrument;

use super::search::Search;

pub struct Uid<'a> {
    pub data: &'a Data,
}

impl Uid<'_> {
    #[instrument(skip(self, lines, command_data, storage))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        storage: Arc<Storage>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if command_data.arguments[0].to_lowercase() == "fetch" {
            Fetch { data: self.data }
                .exec(lines, command_data, storage, true)
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "copy" {
            // TODO implement other commands
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "move" {
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "expunge" {
            lines
                .send(format!("{} BAD Not supported", command_data.tag))
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "search" {
            Search { data: self.data }
                .exec(lines, storage, command_data, true)
                .await?;
        } else if command_data.arguments[0].to_lowercase() == "store" {
            Store { data: self.data }
                .exec(lines, storage, command_data, true)
                .await?;
        }
        Ok(())
    }
}
