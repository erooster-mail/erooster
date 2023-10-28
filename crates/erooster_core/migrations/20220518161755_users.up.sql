-- SPDX-FileCopyrightText: 2023 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

CREATE TABLE IF NOT EXISTS users (
    username TEXT NOT NULL PRIMARY KEY UNIQUE,
    hash TEXT
);
