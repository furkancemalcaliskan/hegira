CREATE TABLE outbox_messages (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    payload JSONB NOT NULL,
    idempotency_key TEXT,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
    available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    locked_at TIMESTAMPTZ,
    lock_owner TEXT,
    processed_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX outbox_messages_idempotency_uq
    ON outbox_messages (name, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX outbox_messages_pending_idx
    ON outbox_messages (available_at, created_at)
    WHERE processed_at IS NULL;

CREATE TABLE inbox_messages (
    consumer TEXT NOT NULL,
    message_id UUID NOT NULL REFERENCES outbox_messages(id) ON DELETE CASCADE,
    processed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (consumer, message_id)
);
