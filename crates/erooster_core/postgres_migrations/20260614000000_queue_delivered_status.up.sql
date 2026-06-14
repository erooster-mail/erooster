-- SPDX-FileCopyrightText: 2026 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

ALTER TYPE outbound_queue_status ADD VALUE IF NOT EXISTS 'delivered';
