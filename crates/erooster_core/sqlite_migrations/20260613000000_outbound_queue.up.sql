-- SPDX-FileCopyrightText: 2026 MTRNord
--
-- SPDX-License-Identifier: Apache-2.0

-- status: 'pending' | 'delivering' | 'failed' | 'abandoned'
CREATE TABLE outbound_queue (
    id TEXT PRIMARY KEY,
    payload TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_retry_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_error TEXT,
    -- append-only JSON array of delivery attempts: [{at, error}]
    delivery_log TEXT NOT NULL DEFAULT '[]',
    from_addr TEXT NOT NULL DEFAULT '',
    to_addrs TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL DEFAULT ''
);

CREATE INDEX outbound_queue_ready ON outbound_queue (next_retry_at)
    WHERE status IN ('pending', 'failed') AND attempts < 10;

CREATE INDEX outbound_queue_status ON outbound_queue (status, created_at);
