ALTER TABLE mails
    DROP COLUMN 'uid';

ALTER TABLE mails
    DROP COLUMN 'mailbox';

DROP TABLE IF EXISTS mailbox_uid_counter;

DROP FUNCTION IF EXISTS next_uid_for_folder;

DROP FUNCTION IF EXISTS increment_uid;

DROP TRIGGER IF EXISTS base_table_insert_trigger;

DROP TRIGGER IF EXISTS base_table_update_unknown_trigger;