-- SPDX-FileCopyrightText: 2023 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

CREATE TABLE IF NOT EXISTS mails (
    id BIGINT NOT NULL PRIMARY KEY UNIQUE,
    maildir_id TEXT NOT NULL
);
