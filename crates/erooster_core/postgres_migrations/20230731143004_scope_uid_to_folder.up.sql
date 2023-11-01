-- SPDX-FileCopyrightText: 2023 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

ALTER TABLE mails
  ADD COLUMN IF NOT EXISTS "uid" BIGINT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS "mailbox" VARCHAR DEFAULT 'unknown' NOT NULL;

create table IF NOT EXISTS mailbox_uid_counter 
(
  "uid" BIGINT primary key, 
  "mailbox" VARCHAR not null
);

CREATE OR REPLACE function next_uid_for_folder(m_mailbox VARCHAR) 
  returns integer
as
$$
   insert into mailbox_uid_counter (uid, mailbox) 
   values (1, m_mailbox)
   on conflict (mailbox) 
   do update 
      set uid = mailbox_uid_counter.uid + 1
   returning uid;
$$
language sql
volatile;

CREATE OR REPLACE function increment_uid()
  returns trigger
as
$$
begin
  new.uid := next_uid_for_folder(new.mailbox);
  return new;
end;
$$
language plpgsql;

CREATE OR REPLACE trigger base_table_insert_trigger
  before insert on mails
  for each row
  execute procedure increment_uid();


CREATE OR REPLACE trigger base_table_update_unknown_trigger
  before update on mails
  for each row
  when (old.mailbox = 'unknown' AND new.mailbox != 'unknown')
  execute procedure increment_uid();
