-- A direct channel loses its meaning when one of its two participants is
-- deleted, but the row (and its message history reference) is kept: the
-- surviving participant may still read history. NULL = active.
ALTER TABLE channels ADD COLUMN deactivated_at TIMESTAMPTZ;
