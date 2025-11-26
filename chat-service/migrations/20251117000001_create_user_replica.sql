-- Create user_replica table for local read model
-- This table stores denormalized user data from user-service events
-- Updated via Kafka consumer listening to user-events topic

CREATE TABLE IF NOT EXISTS user_replica (
    id UUID PRIMARY KEY,
    username VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,

    -- Metadata for tracking
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for fast lookups by username
CREATE INDEX idx_user_replica_username ON user_replica(username);
