use erooster_deps::{
    color_eyre,
    futures::{Sink, SinkExt},
    tracing::{self, instrument},
};

pub struct Rset;

impl Rset {
    #[instrument(skip(self, lines))]
    pub async fn exec<S, E>(&self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        lines.feed(String::from("250 OK")).await?;
        lines.flush().await?;
        Ok(())
    }
}
