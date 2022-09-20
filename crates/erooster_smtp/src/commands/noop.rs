use futures::{Sink, SinkExt};
use tracing::instrument;

pub struct Noop;

impl Noop {
    #[instrument(skip(self, lines))]
    pub async fn exec<S, E>(&self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        lines.send(String::from("250 OK")).await?;
        Ok(())
    }
}
