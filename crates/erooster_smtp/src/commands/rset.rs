use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::instrument;

pub struct Rset;

impl Rset {
    #[instrument(skip(self, lines))]
    pub async fn exec<S>(&self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        lines.feed(String::from("250 OK")).await?;
        lines.flush().await?;
        Ok(())
    }
}
