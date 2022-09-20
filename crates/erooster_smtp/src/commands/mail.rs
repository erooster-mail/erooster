use crate::commands::{parsers::localpart_arguments, CommandData, Data};
use color_eyre::eyre::bail;
use futures::{Sink, SinkExt};
use tracing::{error, instrument};

pub struct Mail<'a> {
    pub data: &'a Data,
}

impl Mail<'_> {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if command_data.arguments.is_empty() {
            bail!("Failed to parse localpart arguments (no arguments)");
        }

        match localpart_arguments(command_data.arguments[0]).map(|(_, senders)| senders) {
            Ok(args) => {
                let senders: Vec<_> = args.iter().map(ToString::to_string).collect();
                {
                    let mut write_lock = self.data.con_state.write().await;
                    write_lock.sender = Some(senders[0].clone());
                };
                lines.send(String::from("250 OK")).await?;
            }
            Err(e) => {
                error!("Failed to parse localpart arguments: {:?}", e);
                bail!("Failed to parse localpart arguments");
            }
        }

        Ok(())
    }
}
