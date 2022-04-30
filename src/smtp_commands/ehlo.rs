use futures::{channel::mpsc::SendError, Sink, SinkExt};

pub struct Ehlo;

impl Ehlo {
    pub async fn exec<S>(&self, hostname: String, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        lines.feed(format!("250 {}", hostname)).await?;
        lines.feed(String::from("250 AUTH LOGIN")).await?;
        lines.flush().await?;
        Ok(())
    }
}