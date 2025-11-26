-- Add migration script here
CREATE TABLE IF NOT EXISTS channels (
    id UUID PRIMARY KEY,
    name VARCHAR(100),
    description TEXT,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    channel_type VARCHAR(20) NOT NULL DEFAULT 'public',
    CONSTRAINT channels_name_key UNIQUE (name)
);

CREATE INDEX idx_channels_name ON channels(name);
CREATE INDEX idx_channels_created_by ON channels(created_by);
CREATE INDEX idx_channels_type ON channels(channel_type);
