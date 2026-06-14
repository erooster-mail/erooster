<!--
SPDX-FileCopyrightText: 2023 MTRNord

SPDX-License-Identifier: Apache-2.0
-->

# Erooster

[![codecov](https://codecov.io/gh/MTRNord/erooster/branch/main/graph/badge.svg?token=ieNQlSkDTF)](https://codecov.io/gh/MTRNord/erooster)
[![Contributor Covenant](https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg)](code_of_conduct.md)

A mail suite written in Rust, meant to be easy to use.

## Features

**IMAP (ports 143 / 993)**
- IMAP4rev2 (RFC 9051) and IMAP4rev1 (RFC 3501) compatible
- TLS on port 993, STARTTLS on port 143
- Extensions: `IDLE`, `NAMESPACE`, `UNSELECT`, `MOVE`, `ESEARCH`, `ENABLE`, `UTF8=ONLY`
- `AUTH=PLAIN` over TLS

**SMTP (port 25)**
- STARTTLS
- Extensions: `PIPELINING`, `SIZE`, `8BITMIME`, `AUTH LOGIN PLAIN` (over TLS), `REQUIRETLS`, `VRFY`
- DKIM signing on outbound messages (RSA PKCS#1 and PKCS#8)
- DKIM and DMARC verification on inbound messages
- SPF verification
- Optional [Rspamd](https://rspamd.com/) integration for spam filtering

**General**
- Maildir storage
- PostgreSQL (default) or SQLite backend
- Single binary, stable Rust
- Autoconfig endpoint (`/mail/config-v1.1.xml`) for automatic client setup (Thunderbird etc.)
- Optional [Sentry](https://sentry.io/) error reporting

## Non-goals

- MySQL / MariaDB support
- POP3
- Exchange / ActiveSync
- Every optional extension from every RFC

## Getting started

### Requirements

- TLS certificates in PEM format (e.g. from Let's Encrypt)
- DKIM key pair
- PostgreSQL database (or SQLite for testing)

### Generate DKIM keys

```bash
opendkim-genkey \
    --domain=<your-hostname> \
    --subdomains
```

Save the `.private` file at the path you set in `mail.dkim_key_path` and add the generated TXT record to your DNS.

### Configuration

Create `config.yml` in `/etc/erooster/` or the working directory:

```yaml
tls:
  key_path: "./certs/key.pem"
  cert_path: "./certs/cert.pem"
mail:
  maildir_folders: "./maildir"
  hostname: "example.com"
  displayname: "Erooster"
  dkim_key_path: "/etc/erooster/keys/default.private"
  dkim_key_selector: "default"
database:
  url: "postgres://user:password@localhost/erooster"
listen_ips:
  - "0.0.0.0"
  - "[::]"
webserver:
  port: 80
  tls: false
# Optional — remove if not using Rspamd
rspamd:
  address: http://localhost:11333
```

`maildir_folders` is the root directory where per-user mail is stored in Maildir format.

### Run

```bash
cargo run --release
```

Or with SQLite (testing only):

```bash
cargo run --release --no-default-features --features sqlite
```

### Manage users

Use the bundled `eroosterctl` binary. It reads the same `config.yml`.

```bash
# Create a new user
eroosterctl register

# Change a user's password
eroosterctl change-password
```

Passwords are stored as Argon2 hashes.

## SQLite

SQLite is supported as a compile-time feature (`--features sqlite`) and is useful for local development and tests. It is **not recommended for production** — use PostgreSQL instead.

## Support

Due to personal constraints, there is no enterprise support for Erooster. Please open GitHub issues instead. Responses are best-effort with no guaranteed time frame.

## Error reporting

Erooster does not report errors automatically. A GitHub link is printed on panics. You can optionally configure a Sentry DSN via environment variable for self-hosted error reporting.
