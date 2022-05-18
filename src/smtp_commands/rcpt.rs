use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::info;

use crate::{
    database::{Database, DB},
    smtp_commands::{parsers::localpart_arguments, CommandData, Data}, smtp_servers::state::State,
};

pub struct Rcpt<'a> {
    pub data: &'a Data,
}

impl Rcpt<'_> {
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        database: DB,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        info!("{:#?}", command_data.arguments);
        assert!(command_data.arguments.len() == 1);
        let receipts: Vec<String> = localpart_arguments(command_data.arguments[0])
            .map(|(_, receipts)| receipts)
            .expect("Failed to parse localpart arguments")
            .iter()
            .map(ToString::to_string)
            .collect();

        {
            let mut write_lock = self.data.con_state.write().await;
            if matches!(&write_lock.state, State::NotAuthenticated) {
                for receipt in &receipts {
                    if !database.user_exists(receipt).await {
                        lines.send(String::from("550 No such user here")).await?;
                        return Ok(());
                    }
                }
            }

            write_lock.receipts = Some(receipts);
        };

        lines.send(String::from("250 OK")).await?;
        Ok(())
    }
}
