use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

use thiserror::Error;

use crate::domain::channel::models::ChannelId;

/// Errors that can occur during topic sharding operations
#[derive(Debug, Error)]
pub enum ShardingError {
    #[error("Number of shards must be greater than 0, got: {0}")]
    ZeroShards(u32),

    #[error("Number of shards must be a power of 2 for optimal distribution, got: {0}")]
    NotPowerOfTwo(u32),

    #[error("Topic prefix cannot be empty")]
    EmptyTopicPrefix,
}

/// Consistent hashing for Kafka topic sharding
///
/// This module implements a sharding strategy for Kafka topics to achieve
/// horizontal scalability. Instead of having one topic for all messages,
/// we shard messages across multiple topics based on channel_id.
///
/// Benefits:
/// - Parallel processing across multiple Kafka partitions
/// - Better load distribution
/// - Consumers can subscribe to specific shards
/// - Scales linearly with a number of shards
#[derive(Debug)]
pub struct TopicSharder {
    num_shards: u32,
    topic_prefix: String,
}

impl TopicSharder {
    /// Create a new topic sharder
    ///
    /// # Arguments
    /// * `num_shards` - Number of shards (topics) to distribute across (must be power of 2)
    /// * `topic_prefix` - Prefix for topic names (e.g., "chat.messages")
    ///
    /// # Errors
    /// Returns `ShardingError::ZeroShards` if num_shards is 0
    /// Returns `ShardingError::NotPowerOfTwo` if num_shards is not a power of 2
    /// Returns `ShardingError::EmptyTopicPrefix` if topic_prefix is empty
    ///
    /// # Example
    /// ```
    /// use chat_service::outbound::events::topic::TopicSharder;
    ///
    /// let sharder = TopicSharder::new(16, "chat.messages")?;
    /// // Creates topics: chat.messages.0, chat.messages.1, ..., chat.messages.15
    /// # Ok::<(), chat_service::outbound::events::topic::ShardingError>(())
    /// ```
    pub fn new(num_shards: u32, topic_prefix: &str) -> Result<Self, ShardingError> {
        if num_shards == 0 {
            return Err(ShardingError::ZeroShards(num_shards));
        }

        if !num_shards.is_power_of_two() {
            return Err(ShardingError::NotPowerOfTwo(num_shards));
        }

        if topic_prefix.is_empty() {
            return Err(ShardingError::EmptyTopicPrefix);
        }

        let topic_prefix = String::from(topic_prefix);

        Ok(Self {
            num_shards,
            topic_prefix,
        })
    }

    /// Get the shard (topic name) for a given channel_id using consistent hashing
    ///
    /// Uses the same hash function for the same channel_id, ensuring:
    /// - All messages for a channel go to the same shard
    /// - Deterministic routing across all service instances
    /// - Even distribution across shards
    pub fn get_shard_for_channel(&self, channel_id: ChannelId) -> String {
        let shard_index = self.compute_shard_index(channel_id);
        format!("{}.{}", self.topic_prefix, shard_index)
    }

    /// Compute the shard index for a channel_id
    fn compute_shard_index(&self, channel_id: ChannelId) -> u32 {
        let mut hasher = DefaultHasher::new();
        channel_id.hash(&mut hasher);
        let hash = hasher.finish();

        // Use modulo to get shard index
        // Since num_shards is power of 2, we can use bitwise AND for better performance
        (hash as u32) & (self.num_shards - 1)
    }

    /// Get all shard topic names
    ///
    /// Useful for consumers that need to subscribe to all shards
    pub fn get_all_shards(&self) -> Vec<String> {
        (0..self.num_shards)
            .map(|i| format!("{}.{}", self.topic_prefix, i))
            .collect()
    }

    /// Get the number of shards
    pub fn num_shards(&self) -> u32 {
        self.num_shards
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::domain::channel::models::ChannelId;

    #[test]
    fn test_shard_consistency() {
        let sharder = TopicSharder::new(16, "chat.messages").unwrap();
        let channel_id = ChannelId::new();

        // The same channel should always map to the same shard
        let shard1 = sharder.get_shard_for_channel(channel_id);
        let shard2 = sharder.get_shard_for_channel(channel_id);
        assert_eq!(shard1, shard2);
    }

    #[test]
    fn test_shard_distribution() {
        let sharder = TopicSharder::new(16, "chat.messages").unwrap();
        let mut shard_counts: HashMap<String, usize> = HashMap::new();

        // Generate 1000 random channel IDs and count distribution
        for _ in 0..1000 {
            let channel_id = ChannelId::new();
            let shard = sharder.get_shard_for_channel(channel_id);
            *shard_counts.entry(shard).or_insert(0) += 1;
        }

        // All shards should be used
        assert_eq!(shard_counts.len(), 16);

        // Distribution should be relatively even (within 40% of average)
        let average = 1000.0 / 16.0;
        for count in shard_counts.values() {
            let ratio = (*count as f64) / average;
            assert!(
                ratio > 0.6 && ratio < 1.4,
                "Distribution too skewed: {} vs avg {}",
                count,
                average
            );
        }
    }

    #[test]
    fn test_get_all_shards() {
        let sharder = TopicSharder::new(4, "chat.messages").unwrap();
        let shards = sharder.get_all_shards();

        assert_eq!(shards.len(), 4);
        assert_eq!(shards[0], "chat.messages.0");
        assert_eq!(shards[1], "chat.messages.1");
        assert_eq!(shards[2], "chat.messages.2");
        assert_eq!(shards[3], "chat.messages.3");
    }

    #[test]
    fn test_zero_shards_returns_error() {
        let result = TopicSharder::new(0, "chat.messages");
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardingError::ZeroShards(n) => assert_eq!(n, 0),
            _ => panic!("Expected ZeroShards error"),
        }
    }

    #[test]
    fn test_non_power_of_two_returns_error() {
        let result = TopicSharder::new(5, "chat.messages");
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardingError::NotPowerOfTwo(n) => assert_eq!(n, 5),
            _ => panic!("Expected NotPowerOfTwo error"),
        }
    }

    #[test]
    fn test_empty_topic_prefix_returns_error() {
        let result = TopicSharder::new(16, "");
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardingError::EmptyTopicPrefix => (),
            _ => panic!("Expected EmptyTopicPrefix error"),
        }
    }

    #[test]
    fn test_shard_format() {
        let sharder = TopicSharder::new(8, "chat.messages").unwrap();
        let channel_id = ChannelId::new();
        let shard = sharder.get_shard_for_channel(channel_id);

        // Should match pattern: chat.messages.N where N is 0-7
        assert!(shard.starts_with("chat.messages."));
        let index: u32 = shard
            .strip_prefix("chat.messages.")
            .unwrap()
            .parse()
            .unwrap();
        assert!(index < 8);
    }
}
