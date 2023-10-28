-- SPDX-FileCopyrightText: 2023 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

DROP FUNCTION mq_checkpoint;
DROP FUNCTION mq_keep_alive;
DROP FUNCTION mq_delete;
DROP FUNCTION mq_commit;
DROP FUNCTION mq_insert;
DROP FUNCTION mq_poll;
DROP FUNCTION mq_active_channels;
DROP FUNCTION mq_latest_message;
DROP TABLE mq_payloads;
DROP TABLE mq_msgs;
DROP FUNCTION mq_uuid_exists;
DROP TYPE mq_new_t;