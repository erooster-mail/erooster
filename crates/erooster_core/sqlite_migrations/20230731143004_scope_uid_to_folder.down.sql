-- SPDX-FileCopyrightText: 2023 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

DROP TABLE IF EXISTS mailbox_uid_counter;

ALTER TABLE mails
    DROP COLUMN `uid`;

ALTER TABLE mails
    DROP COLUMN `mailbox`;