# Erooster

[![codecov](https://codecov.io/gh/MTRNord/erooster/branch/main/graph/badge.svg?token=ieNQlSkDTF)](https://codecov.io/gh/MTRNord/erooster)

A mail suite written in rust meant to be easy to use.

## Getting started

Currently the setup is quite rough.

You need some certificates for your server (pem format) and a postgres database.

To get started you need a `config.yml` like this it can either be in /etc/erooster or the working dir:

```yaml
tls:
  key_path: "./certs/key.pem"
  cert_path: "./certs/cert.pem"
mail:
  maildir_folders: "./maildir"
  hostname: "localhost"
database:
  postgres_url: ""

```
The maildir_folders defines where the emails and forlders can be found at. This is close to the maildir format postfix uses. (We use other files to keep track of the state of it)

After that you can just do `cargo run --release` to run it. The server is reachable via the usual IMAP ports. STARTTLS is currently not supported.

### Setting up users

To set up users you can use the `eroosterctl` command.
It will talk to the database. So make sure your config file is set up.

To register a user you simply run `eroosterctl register` and follow the questions.
The password is safed as an argon2 hash inside the database.

To change a password there is the `change-password` subcommand.
You need to provide the old password and the new one.
It is planned that admins can also change this using a preencrypted password instead.
In the future this is going to be replaced by an integrated web interface users can directly use.

_Note: The status subcommand at this time doesn't actually check the server status._


## Features

- Imap4rev2 compatible
- Maildir support
- TLS by default
- Single binary
- Low Resource usage
- Postgres first
- Integrated SMTP server

## Non Goal

- Implementing every single peace of optional spec
- MySQL/Mariadb support
- Support for imap LOGIN command (It is per rev2 spec)
- Support for POP3
- Support for Exchange

## Comparisons

As I made a reddit post some comparisons were made in the commands.
You can check them out at https://www.reddit.com/r/rust/comments/uyxxrg/comment/ia7qwcg/?utm_source=share&utm_medium=web2x&context=3