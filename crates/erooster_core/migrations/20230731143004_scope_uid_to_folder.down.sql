DROP TRIGGER IF EXISTS base_table_update_unknown_trigger ON mails;

DROP TRIGGER IF EXISTS base_table_insert_trigger ON mails;

DROP FUNCTION IF EXISTS increment_uid;

DROP FUNCTION IF EXISTS next_uid_for_folder;

DROP TABLE IF EXISTS mailbox_uid_counter;

ALTER TABLE mails
    DROP COLUMN "uid";

ALTER TABLE mails
    DROP COLUMN "mailbox";