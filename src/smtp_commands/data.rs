use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::info;

use crate::{smtp_commands::Data, smtp_servers::state::State};

#[allow(clippy::module_name_repetitions)]
pub struct DataCommand<'a> {
    pub data: &'a Data,
}

impl DataCommand<'_> {
    pub async fn exec<S>(&self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        info!("Waiting for incoming data");
        {
            self.data.con_state.write().await.state = State::ReceivingData;
        };
        lines
            .send(String::from("354 Start mail input; end with <CRLF>.<CRLF>"))
            .await?;
        Ok(())
    }

    pub async fn receive<S>(&self, lines: &mut S, line: &str) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        {
            let mut write_lock = self.data.con_state.write().await;
            if let Some(data) = &write_lock.data {
                write_lock.data = Some(format!("{}\r\n{}", data, line));
            } else {
                write_lock.data = Some(line.to_string());
            }
            if line == "." {
                info!("Data: {:#?}", write_lock.data);
                write_lock.state = State::NotAuthenticated;
            }
        };
        if line == "." {
            lines.send(String::from("250 OK")).await?;
        }
        Ok(())
    }
}
