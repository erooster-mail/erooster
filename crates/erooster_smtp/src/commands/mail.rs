use crate::commands::{parsers::localpart_arguments, CommandData, Data};
use color_eyre::eyre::{bail, ContextCompat};
use futures::{Sink, SinkExt};
use mail_auth::{Resolver, SpfResult};
use tracing::{error, instrument};

pub struct Mail<'a> {
    pub data: &'a Data,
}

impl Mail<'_> {
    #[instrument(skip(self, lines, command_data))]
    pub async fn exec<S, E>(
        &self,
        hostname: &str,
        lines: &mut S,
        command_data: &CommandData<'_>,
    ) -> color_eyre::eyre::Result<()>
    where
        E: std::error::Error + std::marker::Sync + std::marker::Send + 'static,
        S: Sink<String, Error = E> + std::marker::Unpin + std::marker::Send,
    {
        if command_data.arguments.is_empty() {
            bail!("Failed to parse localpart arguments (no arguments)");
        }

        match localpart_arguments(command_data.arguments[0]).map(|(_, senders)| senders) {
            Ok(args) => {
                // Create a resolver using Quad9 DNS
                let resolver = Resolver::new_quad9_tls()?;
                let mut write_lock = self.data.con_state.write().await;
                for sender in &args {
                    // Verify MAIL-FROM identity
                    let result = resolver
                        .verify_spf_sender(
                            write_lock.peer_addr.parse()?,
                            write_lock.ehlo.as_ref().context("Missing ehlo")?,
                            hostname,
                            sender,
                        )
                        .await;

                    // TODO: Possibly we shouldnt fail them all? Unsure
                    if result.result() == SpfResult::Fail {
                        lines
                            .feed(String::from("550 5.7.1 SPF MAIL FROM check failed:"))
                            .await?;
                        // TODO: find a better url
                        lines
                            .feed(String::from("550 5.7.1 The domain example.com explains:"))
                            .await?;
                        lines
                            .feed(String::from(
                                "550 5.7.1 Please see http://www.example.com/mailpolicy.html",
                            ))
                            .await?;
                        lines.flush().await?;
                        return Ok(());
                    }
                }
                let senders: Vec<_> = args.iter().map(ToString::to_string).collect();
                {
                    write_lock.sender = Some(senders[0].clone());
                };
                lines
                    .send(format!(
                        "250 2.1.0 Originator {} OK",
                        command_data.arguments[0]
                    ))
                    .await?;
            }
            Err(e) => {
                error!("Failed to parse localpart arguments: {:?}", e);
                bail!("Failed to parse localpart arguments");
            }
        }

        Ok(())
    }
}
