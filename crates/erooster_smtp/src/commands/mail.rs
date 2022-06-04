use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::{info, instrument};

use crate::commands::{parsers::localpart_arguments, CommandData, Data};

pub struct Mail<'a> {
    pub data: &'a Data,
}

impl Mail<'_> {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        info!("{:#?}", command_data.arguments);
        assert!(command_data.arguments.len() == 1);
        let senders: Vec<String> = localpart_arguments(command_data.arguments[0])
            .map(|(_, senders)| senders)
            .expect("Failed to parse localpart arguments")
            .iter()
            .map(ToString::to_string)
            .collect();

        {
            let mut write_lock = self.data.con_state.write().await;
            write_lock.sender = Some(senders[0].clone());
        };
        lines.send(String::from("250 OK")).await?;
        Ok(())
    }
}
