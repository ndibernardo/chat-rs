CREATE TABLE IF NOT EXISTS channel_members (
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL,
    PRIMARY KEY (channel_id, user_id)
);

CREATE TABLE IF NOT EXISTS channel_participants (
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL,
    PRIMARY KEY (channel_id, user_id)
);

CREATE INDEX idx_channel_members_user      ON channel_members(user_id);
CREATE INDEX idx_channel_participants_user ON channel_participants(user_id);
