use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::instrument;

pub struct Check<'a> {
    pub data: &'a Data,
}

impl Check<'_> {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        // This is an Imap4rev1 feature. It does the same as Noop for us as we have no memory gc.
        // It also only is allowed in selected state
        if matches!(
            self.data.con_state.read().await.state,
            State::Selected(_, _)
        ) {
            lines
                .send(format!("{} OK CHECK completed", command_data.tag))
                .await?;
        } else {
            lines
                .send(format!("{} NO invalid state", command_data.tag))
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Access, Connection};
    use futures::{channel::mpsc, StreamExt};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_successfull_check() {
        let caps = Check {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                    secure: true,
                    // TODO this may be invalid actuallly
                    username: None,
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "1",
            command: Commands::Check,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("1 OK CHECK completed")));
    }

    #[tokio::test]
    async fn test_unsuccessfull_check() {
        let caps = Check {
            data: &Data {
                con_state: Arc::new(RwLock::new(Connection {
                    state: State::NotAuthenticated,
                    secure: true,
                    username: None,
                })),
            },
        };
        let cmd_data = CommandData {
            tag: "1",
            command: Commands::Check,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("1 NO invalid state")));
    }
}
