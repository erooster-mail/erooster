// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{
    commands::{CommandData, Data},
    servers::state::State,
};
use erooster_core::backend::storage::{MailEntryType, MailStorage, Storage};
use erooster_deps::{
    color_eyre,
    futures::{Sink, SinkExt},
    tracing::{self, instrument},
};

pub struct Check<'a> {
    pub data: &'a Data,
}

impl Check<'_> {
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
        // This is an Imap4rev1 feature. It does the same as Noop for us as we have no memory gc.
        // It also only is allowed in selected state
        if let State::Selected(folder, _) = &self.data.con_state.state {
            let folder = folder.replace('/', ".");
            let Some(username) = self.data.con_state.username.clone() else {
                lines
                    .send(format!("{} NO invalid state", command_data.tag))
                    .await?;
                return Ok(());
            };
            let mailbox_path = storage.to_ondisk_path(folder.clone(), username.clone())?;
            let mails: Vec<MailEntryType> = storage
                .list_new(format!("{username}/{folder}"), &mailbox_path)
                .await;
            let got_new = !mails.is_empty();
            if got_new {
                let mails: Vec<MailEntryType> = storage
                    .list_all(format!("{username}/{folder}"), &mailbox_path)
                    .await;
                lines.send(format!("* {} EXISTS", mails.len())).await?;
            }

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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use crate::servers::state::{Access, Connection};
    use erooster_deps::futures::{channel::mpsc, StreamExt};
    use erooster_deps::tokio;

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_successful_check() {
        let caps = Check {
            data: &mut Data {
                con_state: Connection {
                    state: State::Selected("INBOX".to_string(), Access::ReadWrite),
                    secure: true,
                    username: Some(String::from("test")),
                    active_capabilities: vec![],
                },
            },
        };
        let cmd_data = CommandData {
            tag: "1",
            command: Commands::Check,
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
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(rx.next().await, Some(String::from("1 OK CHECK completed")));
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[tokio::test]
    async fn test_unsuccessful_check() {
        let caps = Check {
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
            tag: "1",
            command: Commands::Check,
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
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(rx.next().await, Some(String::from("1 NO invalid state")));
    }
}
