use crate::commands::CommandData;
use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::instrument;

pub struct Capability;

impl Capability {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S>(
        &self,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        let capabilities = get_capabilities();
        lines.feed(format!("* {}", capabilities)).await?;
        lines
            .feed(format!("{} OK CAPABILITY completed", command_data.tag))
            .await?;
        lines.flush().await?;
        Ok(())
    }
}

pub const fn get_capabilities() -> &'static str {
    "CAPABILITY AUTH=PLAIN LOGINDISABLED UTF8=ONLY ENABLE IMAP4rev2 IMAP4rev1"
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
            tag: "",
            command: Commands::Capability,
            arguments: &[],
        };
        let (mut tx, mut rx) = mpsc::unbounded();
        let res = caps.exec(&mut tx, &cmd_data).await;
        assert!(res.is_ok());
        assert_eq!(
            rx.next().await,
            Some(String::from(
                "* CAPABILITY AUTH=PLAIN LOGINDISABLED IMAP4rev2 IMAP4rev1"
            ))
        );
    }
}
