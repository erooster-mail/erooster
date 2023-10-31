-- SPDX-FileCopyrightText: 2023 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

ALTER TABLE mails
  ADD `uid` BIGINT NOT NULL DEFAULT 0;

ALTER TABLE mails
  ADD `mailbox` TEXT DEFAULT 'unknown' NOT NULL;

create table IF NOT EXISTS mailbox_uid_counter 
(
  `uid` BIGINT primary key, 
  `mailbox` TEXT not null
);
