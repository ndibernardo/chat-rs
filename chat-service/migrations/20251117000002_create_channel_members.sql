-- Single membership relation for every channel type. Private channels store
-- their members here; direct channels store their two participants here too
-- (written in the same transaction that creates the channel), so every read
-- path over membership ("who has access to channel X?") has one rail.
CREATE TABLE IF NOT EXISTS channel_members (
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL,
    PRIMARY KEY (channel_id, user_id)
);

CREATE INDEX idx_channel_members_user ON channel_members(user_id);

-- Pure invariant table for direct channels: the canonical natural key
-- (ordered participant pair), functioning as a materialized unique index that
-- Postgres cannot express declaratively over channel_members rows (there is
-- no "UNIQUE over the set of members per channel"). Written once at creation
-- and never read for membership — consulted only implicitly, via the unique
-- violation on insert, to answer "does a DM between A and B already exist?".
--
-- CHECK (user_id_low < user_id_high) does two jobs at once: canonical
-- ordering (so [a,b] and [b,a] collide on the UNIQUE constraint) and a
-- schema-level ban on self-DMs (a strict inequality can't hold for a == a)
-- — the relational twin of the domain's Channel::new_direct check.
--
-- The pair is deliberately redundant with the two rows in channel_members:
-- both are written once, in the same transaction, and a DM's pair never
-- changes afterward, so no drift is possible.
CREATE TABLE IF NOT EXISTS direct_channel_keys (
    channel_id   UUID PRIMARY KEY REFERENCES channels(id) ON DELETE CASCADE,
    user_id_low  UUID NOT NULL,
    user_id_high UUID NOT NULL,
    CONSTRAINT direct_channel_keys_ordered_pair CHECK (user_id_low < user_id_high),
    CONSTRAINT direct_channel_keys_unique_pair  UNIQUE (user_id_low, user_id_high)
);
