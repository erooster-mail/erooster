use crate::commands::{parsers::search_arguments, CommandData, Data};
use erooster_core::backend::storage::Storage;
use futures::{Sink, SinkExt};
use nom::{error::convert_error, Finish};
use std::sync::Arc;
use tracing::{debug, error, instrument};

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
        is_uid: bool,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let offset = usize::from(is_uid);
        let arguments = &command_data.arguments[offset..];
        let search_args: String = arguments.join(" ");
        let search_args = search_args.as_str();
        debug!("Search arguments: {:?}", search_args);

        match search_arguments(search_args).finish() {
            Ok((_, args)) => {
                debug!(
                    "Resulting parsed arguments of the search query: {:#?}",
                    args
                );
            }
            Err(e) => {
                error!(
                    "Failed to parse search arguments: {}",
                    convert_error(search_args, e)
                );
                lines
                    .send(format!("{} BAD Unable to parse", command_data.tag))
                    .await?;
            }
        }

        lines
            .send(format!("{} BAD Not supported", command_data.tag))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}
