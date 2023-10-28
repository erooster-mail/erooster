-- SPDX-FileCopyrightText: 2023 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

ALTER TABLE mails
ALTER modseq SET DATA TYPE BIGINT;
UPDATE mails
SET modseq = 1;