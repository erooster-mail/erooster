-- SPDX-FileCopyrightText: 2023 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

ALTER TABLE mails
ADD COLUMN modseq BIGSERIAL NOT NULL;