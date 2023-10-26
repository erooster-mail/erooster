use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use erooster_core::backend::storage::{MailStorage, Storage};
use erooster_deps::{
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tracing::{self, instrument},
};

pub struct Noop<'a> {
    pub data: &'a Data,
}

impl Noop<'_> {
    #[instrument(skip(self, lines, storage, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        storage: &Storage,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        // TODO: return status as suggested in https://www.rfc-editor.org/rfc/rfc9051.html#name-noop-command
        if let State::Selected(folder, _) = &self.data.con_state.state {
            let folder = folder.replace('/', ".");
            let mailbox_path = storage.to_ondisk_path(
                folder.clone(),
                self.data
                    .con_state
                    .username
                    .clone()
                    .context("Username missing in internal State")?,
            )?;

            let mails = storage.count_cur(&mailbox_path) + storage.count_new(&mailbox_path);
            lines.send(format!("* {mails} EXISTS")).await?;
        }
        lines
            .send(format!("{} OK NOOP completed", command_data.tag))
            .await?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Access, Connection, State};
    use erooster_deps::futures::{channel::mpsc, StreamExt};
    use erooster_deps::tokio;

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_not_selected() {
        let state = &mut Data {
            con_state: Connection {
                state: State::NotAuthenticated,
                secure: true,
                username: None,
                active_capabilities: vec![],
            },
        };
        let caps = Noop { data: state };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Noop,
            arguments: &[],
        };
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("a1 OK NOOP completed")));
    }

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn test_selected_empty() {
        let state = &mut Data {
            con_state: Connection {
                state: State::Selected("Meow".to_string(), Access::ReadOnly),
                secure: true,
                username: Some("MTRNord".to_string()),
                active_capabilities: vec![],
            },
        };
        let caps = Noop { data: state };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Noop,
            arguments: &[],
        };
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(rx.next().await, Some(String::from("* 0 EXISTS")));
        assert_eq!(rx.next().await, Some(String::from("a1 OK NOOP completed")));
    }

    // TODO: We should test having messages in new or current. This needs setting up the DB and the folder
}
