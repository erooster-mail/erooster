DROP FUNCTION IF EXISTS next_uid_for_folder;

CREATE OR REPLACE function next_uid_for_folder(m_mailbox VARCHAR) 
  returns BIGINT
as
$$
   insert into mailbox_uid_counter ("uid", mailbox) 
   values (1, m_mailbox)
   on conflict (mailbox)
   do update 
      set "uid" = mailbox_uid_counter.uid + 1
   returning "uid";
$$
language sql
volatile;

ALTER TABLE mailbox_uid_counter DROP CONSTRAINT mailbox_uid_counter_pkey;
ALTER TABLE mailbox_uid_counter ADD PRIMARY KEY (mailbox);


ALTER TABLE mails DROP CONSTRAINT mails_pkey;
ALTER TABLE mails ADD PRIMARY KEY (mailbox, maildir_id);