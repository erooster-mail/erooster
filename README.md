<!--
SPDX-FileCopyrightText: 2023 MTRNord

SPDX-License-Identifier: Apache-2.0
-->

# Erooster

[![codecov](https://codecov.io/gh/MTRNord/erooster/branch/main/graph/badge.svg?token=ieNQlSkDTF)](https://codecov.io/gh/MTRNord/erooster)
[![Contributor Covenant](https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg)](code_of_conduct.md)

A mail suite written in rust meant to be easy to use.

## Getting started

Currently, the setup is quite rough.

You need some certificates for your server (PEM format) and a Postgres database as well as dkim keys.
The easiest way to get them is to use opendkim like this:

```bash
opendkim-genkey \
    --domain=<hostname> \
    --subdomains
```

you should save the file in the folder at `mail.dkim_key_path`.
You also should add the TXT dns record that is in the txt file to your domain.

To get started you need a `config.yml` like this, it can either be in /etc/erooster or the working dir:

```yaml
tls:
  key_path: "./certs/key.pem"
  cert_path: "./certs/cert.pem"
mail:
  maildir_folders: "./maildir"
  hostname: "localhost"
  displayname: Erooster
  dkim_key_path: "/etc/erooster/keys/default.private"
  dkim_key_selector: "default"
database:
  postgres_url: ""
listen_ips:
  - "[::1]"
  - "127.0.0.1"
webserver:
  port: 80
  tls: false
sentry: false
rspamd:
  address: http://localhost:11333
```

The maildir_folders defines where the emails and folders can be found at. This is close to the maildir format postfix uses. (We use other files to keep track of the state of it)

After that, you can just do `cargo run --release` to run it. The server is reachable via the usual IMAP ports. STARTTLS is only supported for SMTP.

### Setting up users

To set up users, you can use the `eroosterctl` command.
It will talk to the database. So make sure your config file is set up.

To register a user, you simply run `eroosterctl register` and follow the questions.
The password is saved as an argon2 hash inside the database.

To change a password, there is the `change-password` subcommand.
You need to provide the old password and the new one.
It is planned that admins can also change this using a pre-encrypted password instead.
In the future, this is going to be replaced by an integrated web interface users can directly use.

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

- Implementing every single piece of optional spec
- MySQL/MariaDB support
- Support for IMAP LOGIN command (It is per rev2 spec)
- Support for POP3
- Support for Exchange (this is subject to change)

## Error Reporting

Erooster by default does not auto report any panics or errors.
It provides however a GitHub reporting link on panics.

## Comparisons

As I made a Reddit post, some comparisons were made in the commands.
You can check them out at <https://www.reddit.com/r/rust/comments/uyxxrg/comment/ia7qwcg/?utm_source=share&utm_medium=web2x&context=3>

## Support

Due to personal constraints, I currently do not prove enterprise support for this. Please open issues instead. I will try to reply as soon as I can, but I cannot guarantee a specific time frame.

## Contact

To contact the erooster team you can find us at <https://matrix.to/#/#erooster:midnightthoughts.space> or if an email is absolutely needed please write to [support@nordgedanken.dev](mailto:support@nordgedanken.dev). As written in the Support section, there is no enterprise support at this time. So please don't ask for it. It will just fill up the mailbox. :)

## Note on SQLITE

Note that this isnt officially supported outside of running tests. Some migrations rely on sql functions which sqlite does NOT support.

Running sqlite in prod means no support whatsoever.
