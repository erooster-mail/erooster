// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::commands::CommandData;
use erooster_deps::{
    color_eyre,
    futures::{Sink, SinkExt},
    tracing::{self, instrument},
};

pub struct Login;

impl Login {
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
        lines
            .send(format!(
                "{} NO [PRIVACYREQUIRED] LOGIN COMMAND DISABLED FOR SECURITY. USE AUTH",
                command_data.tag
            ))
            .await?;
        Ok(())
    }
}
