-- SPDX-FileCopyrightText: 2023 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0
ALTER TABLE mails RENAME TO mails_temp;

CREATE TABLE IF NOT EXISTS mails (
    id INTEGER NOT NULL PRIMARY KEY UNIQUE,
    maildir_id TEXT NOT NULL,
    `uid` INTEGER NOT NULL DEFAULT 0,
    `mailbox` TEXT DEFAULT 'unknown' NOT NULL,
    modseq BIGINT NOT NULL
);

INSERT INTO mails SELECT * FROM mails_temp;

DROP TABLE mails_temp;