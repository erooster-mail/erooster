use futures::{channel::mpsc::SendError, Sink, SinkExt};

pub struct Quit;

impl Quit {
    pub async fn exec<S>(&self, lines: &mut S) -> color_eyre::eyre::Result<()>
    where
        S: Sink<String, Error = SendError> + std::marker::Unpin + std::marker::Send,
    {
        lines.send(String::from("221 OK")).await?;
        Ok(())
    }
}
