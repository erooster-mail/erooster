-- SPDX-FileCopyrightText: 2026 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

CREATE TYPE outbound_queue_status AS ENUM ('pending', 'delivering', 'failed', 'abandoned');

CREATE TABLE outbound_queue (
    id UUID PRIMARY KEY,
    payload JSONB NOT NULL,
    status outbound_queue_status NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_error TEXT,
    -- append-only log of every delivery attempt: [{at, error}]
    delivery_log JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- human-readable summary for postmaster inspection
    from_addr TEXT NOT NULL DEFAULT '',
    to_addrs TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL DEFAULT ''
);

-- worker poll index: only pending/failed entries that are due
CREATE INDEX outbound_queue_ready ON outbound_queue (next_retry_at)
    WHERE status IN ('pending', 'failed') AND attempts < 10;

-- postmaster / admin queries
CREATE INDEX outbound_queue_status ON outbound_queue (status, created_at);
