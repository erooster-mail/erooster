use futures::{channel::mpsc::SendError, Sink, SinkExt};
use tracing::instrument;

pub struct Noop;

impl Noop {
    #[instrument(skip(self, lines))]
    pub async fn exec<S>(&self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        lines.send(String::from("250 OK")).await?;
        Ok(())
    }
}
