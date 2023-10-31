// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use erooster_core::backend::storage::{MailStorage, Storage};
use erooster_deps::{
    color_eyre::{self, eyre::ContextCompat},
    futures::{Sink, SinkExt},
    tracing::{self, error, instrument},
};

pub struct Create<'a> {
    pub data: &'a Data,
}
impl Create<'_> {
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
        if matches!(self.data.con_state.state, State::Authenticated)
            || matches!(self.data.con_state.state, State::Selected(_, _))
        {
            let arguments = &command_data.arguments;
            assert!(arguments.len() == 1);
            if arguments.len() == 1 {
                let folder = arguments[0].replace('/', ".");

                let mailbox_path = storage.to_ondisk_path(
                    folder.clone(),
                    self.data
                        .con_state
                        .username
                        .clone()
                        .context("Username missing in internal State")?,
                )?;
                let folder = storage.to_ondisk_path_name(folder)?;

                match storage.create_dirs(&mailbox_path) {
                    Ok(_) => {
                        if folder.to_lowercase() == ".sent" {
                            storage.add_flag(&mailbox_path, "\\Sent").await?;
                            storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                        } else if folder.to_lowercase() == ".junk" {
                            storage.add_flag(&mailbox_path, "\\Junk").await?;
                            storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                        } else if folder.to_lowercase() == ".drafts" {
                            storage.add_flag(&mailbox_path, "\\Drafts").await?;
                            storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                        } else if folder.to_lowercase() == ".archive" {
                            storage.add_flag(&mailbox_path, "\\Archive").await?;
                            storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                        } else if folder.to_lowercase() == ".trash" {
                            storage.add_flag(&mailbox_path, "\\Trash").await?;
                            storage.add_flag(&mailbox_path, "\\Subscribed").await?;
                        }
                        lines
                            .send(format!("{} OK CREATE completed", command_data.tag))
                            .await?;
                    }
                    Err(e) => {
                        error!("Failed to create folder: {}", e);

                        lines
                            .send(format!("{} NO CREATE failure", command_data.tag))
                            .await?;
                    }
                }
            } else {
                lines
                    .send(format!(
                        "{} BAD [SERVERBUG] invalid arguments",
                        command_data.tag
                    ))
                    .await?;
            }
        } else {
            lines
                .send(format!("{} NO CREATE Not authenticated", command_data.tag))
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::{CommandData, Commands},
        servers::state::{Connection, State},
    };
    use erooster_deps::{
        futures::{channel::mpsc, StreamExt},
        tokio,
    };

    #[tokio::test]
    async fn test_create_trash() {
        let caps = Create {
            data: &mut Data {
                con_state: Connection {
                    state: State::Selected(
                        String::from("."),
                        crate::servers::state::Access::ReadWrite,
                    ),
                    secure: true,
                    username: Some(String::from("test")),
                    active_capabilities: vec![],
                },
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Create,
            arguments: &["Trash"],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 OK CREATE completed"))
        );
    }

    /// Since the state Authenticated is only required read-only selected is allowed to create rooms!
    #[tokio::test]
    async fn test_create_read_only() {
        let caps = Create {
            data: &mut Data {
                con_state: Connection {
                    state: State::Selected(
                        String::from("."),
                        crate::servers::state::Access::ReadOnly,
                    ),
                    secure: true,
                    username: Some(String::from("test")),
                    active_capabilities: vec![],
                },
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Create,
            arguments: &["Trash"],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 OK CREATE completed"))
        );
    }

    #[tokio::test]
    async fn test_create_not_logged_in() {
        let caps = Create {
            data: &mut Data {
                con_state: Connection {
                    state: State::NotAuthenticated,
                    secure: true,
                    username: None,
                    active_capabilities: vec![],
                },
            },
        };
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Create,
            arguments: &["Trash"],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let config = erooster_core::get_config(String::from("./config.yml"))
            .await
            .unwrap();
        let database = erooster_core::backend::database::get_database(&config)
            .await
            .unwrap();
        let storage = erooster_core::backend::storage::get_storage(database, config);
        let res = caps.exec(&mut tx, &storage, &cmd_data).await;
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 NO CREATE Not authenticated"))
        );
    }
}
