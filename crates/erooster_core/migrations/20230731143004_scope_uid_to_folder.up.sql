ALTER TABLE mails
  ADD uid BIGINT NOT NULL DEFAULT 0;
  ADD mailbox VARCHAR NOT NULL DEFAULT 'unknown';

ALTER TABLE mails DROP CONSTRAINT mails_pkey;

ALTER TABLE mails ADD PRIMARY KEY (mailbox, uid);

create table IF NOT EXISTS mailbox_uid_counter 
(
  uid BIGINT primary key, 
  mailbox VARCHAR not null
);

create function IF NOT EXISTS next_uid_for_folder(m_mailbox VARCHAR) 
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

create function IF NOT EXISTS increment_uid()
  returns trigger
as
$$
begin
  new.uid := next_uid_for_folder(new.mailbox);
  return new;
end;
$$
language plpgsql;

create trigger IF NOT EXISTS base_table_insert_trigger
  before insert on mails
  for each row
  execute procedure increment_uid();


create trigger IF NOT EXISTS base_table_update_unknown_trigger
  before update on mails
  for each row
  when (old.mailbox = 'unknown' AND new.mailbox != 'unknown')
  execute procedure increment_uid();
