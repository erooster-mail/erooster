use crate::{
    commands::{parsers::localpart_arguments, CommandData, Data},
    servers::state::State,
};
use color_eyre::eyre::bail;
use erooster_core::backend::database::{Database, DB};
use futures::{Sink, SinkExt};
use tracing::{info, instrument};

pub struct Rcpt<'a> {
    pub data: &'a mut Data,
}

impl Rcpt<'_> {
    #[instrument(skip(self, lines, database, command_data))]
    pub async fn exec<S, E>(
        &mut self,
        lines: &mut S,
        database: &DB,
        hostname: &str,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
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
            if matches!(&self.data.con_state.state, State::NotAuthenticated) {
                for receipt in &receipts {
                    if !receipt.contains(hostname) {
                        lines
                            .feed(String::from(
                                "551-5.7.1 Forwarding to remote hosts disabled",
                            ))
                            .await?;
                        lines
                            .feed(String::from(
                                "551 5.7.1 Select another host to act as your forwarder",
                            ))
                            .await?;
                        lines.flush().await?;
                        return Ok(());
                    }
                    if !database.user_exists(&receipt.to_lowercase()).await {
                        lines
                            .send(format!("550 5.1.1 Mailbox \"{receipt}\" does not exist"))
                            .await?;
                        return Ok(());
                    }
                }
            }

            self.data.con_state.receipts = Some(receipts);
        };

        lines
            .send(format!(
                "250 2.1.5 Recipient {} OK",
                command_data.arguments[0]
            ))
            .await?;
        Ok(())
    }
}
