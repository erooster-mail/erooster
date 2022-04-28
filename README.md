# Erooster

A mail suite written in rust meant to be easy to use.

## Getting started

Currently the setup is quite rough.

You need some certificates for your server (pem format) and it currently has no user managment yet.

To get started you need a `config.yml` like this:

```
tls:
  cert_path: "./certs"
mail:
  maildir_folders: "./maildir"
```

The cert_path defines where the key.pem and cert.pem can be found. The names currently are hard coded but meant to get adjustable in the future.

The maildir_folders defines where the emails and forlders can be found at. This is close to the maildir format postfix uses. (We use other files to keep track of the state of it)

After that you can just do `cargo run --release` to run it. The server is reachable via the usual IMAP ports. STARTTLS is currently not supported.

## Features

- Imap4rev2 compatible
- Maildir support
- TLS by default
- Single binary
- Low Resource usage
- Postgres first

## Non Goal

- Implementing every single peace of optional spec
- MySQL/Mariadb support
- Support for imap LOGIN command (It is per rev2 spec)
