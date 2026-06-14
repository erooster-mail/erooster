-- SPDX-FileCopyrightText: 2026 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

-- Stores the outcome of DKIM verification for each inbound message.
-- Values: 'pass' | 'fail' | 'neutral' | 'temp_error' | 'perm_error' | 'none'
-- NULL means the message was not received via SMTP (e.g. IMAP APPEND).
ALTER TABLE mails ADD COLUMN dkim_status TEXT;
