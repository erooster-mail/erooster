use crate::{
    commands::{parsers::localpart_arguments, CommandData, Data},
    servers::state::State,
};
use color_eyre::eyre::bail;
use erooster_core::backend::database::{Database, DB};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::{info, instrument};

pub struct Rcpt<'a> {
    pub data: &'a Data,
}

impl Rcpt<'_> {
    #[instrument(skip(self, lines, database, command_data))]
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
        if command_data.arguments.is_empty() {
            bail!("Failed to parse rcpt arguments");
        }
        let receipts: Vec<String> = localpart_arguments(command_data.arguments[0])
            .map(|(_, receipts)| receipts)
            .expect("Failed to parse localpart arguments")
            .iter()
            .map(ToString::to_string)
            .collect();

        {
            let mut write_lock = self.data.con_state.write().await;
            if matches!(&write_lock.state, State::NotAuthenticated) {
                cfg_if::cfg_if! {
                    if #[cfg(not(feature = "honeypot"))] {
                        for receipt in &receipts {
                            if !database.user_exists(&receipt.to_lowercase()).await {
                                lines.send(String::from("550 No such user here")).await?;
                                return Ok(());
                            }
                        }
                    }
                }
            }

            write_lock.receipts = Some(receipts);
        };

        lines.send(String::from("250 OK")).await?;
        Ok(())
    }
}
