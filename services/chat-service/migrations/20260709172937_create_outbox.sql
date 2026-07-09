-- Transactional outbox: every mutation that must publish a domain event
-- writes its outbox row in the same transaction as the aggregate write, so
-- the two can never diverge. A separate relay drains this table into Kafka.
CREATE TABLE outbox (
    event_id UUID PRIMARY KEY,
    aggregate_id UUID NOT NULL,
    topic TEXT NOT NULL,
    schema TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ
);

-- Partial index: only unpublished rows are ever scanned by the relay, and
-- that set stays small under normal operation.
CREATE INDEX outbox_unpublished_idx ON outbox (created_at) WHERE published_at IS NULL;
