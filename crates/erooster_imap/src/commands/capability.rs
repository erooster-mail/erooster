use crate::commands::CommandData;
use futures::{Sink, SinkExt};
use tracing::instrument;

pub struct Capability;

impl Capability {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S, E>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
        secure: bool,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        let capabilities = if secure {
            get_capabilities()
        } else {
            get_unencrypted_capabilities()
        };
        lines.feed(format!("* {capabilities}")).await?;
        lines
            .feed(format!("{} OK CAPABILITY completed", command_data.tag))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}

pub const fn get_capabilities() -> &'static str {
    "CAPABILITY AUTH=PLAIN LOGINDISABLED UTF8=ONLY ENABLE IMAP4rev2 IMAP4rev1 ESEARCH"
}

pub const fn get_unencrypted_capabilities() -> &'static str {
    "CAPABILITY LOGINDISABLED UTF8=ONLY ENABLE IMAP4rev2 IMAP4rev1 STARTTLS"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandData, Commands};
    use futures::{channel::mpsc, StreamExt};

    #[tokio::test]
    async fn test_get_capabilities() {
        let caps = Capability {};
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Capability,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &cmd_data, true).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "* CAPABILITY AUTH=PLAIN LOGINDISABLED UTF8=ONLY ENABLE IMAP4rev2 IMAP4rev1 ESEARCH"
            ))
        );
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 OK CAPABILITY completed"))
        );
    }

    #[tokio::test]
    async fn test_get_unencrypted_capabilities() {
        let caps = Capability {};
        let cmd_data = CommandData {
            tag: "a1",
            command: Commands::Capability,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &cmd_data, false).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "* CAPABILITY LOGINDISABLED UTF8=ONLY ENABLE IMAP4rev2 IMAP4rev1 STARTTLS"
            ))
        );
        assert_eq!(
            rx.next().await,
            Some(String::from("a1 OK CAPABILITY completed"))
        );
    }
}
