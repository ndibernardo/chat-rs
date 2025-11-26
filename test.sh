#!/usr/bin/env bash
set -e
set -eo pipefail

echo "Starting test infrastructure..."
docker-compose -f docker-compose.test.yml up -d

echo "Waiting for services to be ready..."
# Wait for postgres, kafka, and user-service to fully start
sleep 20

# Wait specifically for Cassandra to be healthy (it takes ~50-60 seconds)
echo "Waiting for Cassandra to be ready..."
timeout=90
while [ $timeout -gt 0 ]; do
  if docker exec cassandra-test cqlsh -e "describe cluster" > /dev/null 2>&1; then
    echo "Cassandra is ready!"
    break
  fi
  echo "Waiting for Cassandra... ($timeout seconds remaining)"
  sleep 5
  timeout=$((timeout - 5))
done

if [ $timeout -le 0 ]; then
  echo "Cassandra failed to become ready"
  docker logs cassandra-test
  exit 1
fi

echo "Setting up databases..."
DATABASE_URL="postgresql://postgres:postgres@localhost:5433/user" sqlx migrate run --source user-service/migrations
DATABASE_URL="postgresql://postgres:postgres@localhost:5433/chat" sqlx migrate run --source chat-service/migrations

docker exec cassandra-test cqlsh -e "CREATE KEYSPACE IF NOT EXISTS chat WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1};"
docker exec cassandra-test cqlsh -e "USE chat; CREATE TABLE IF NOT EXISTS messages_by_channel (channel_id uuid, message_id timeuuid, user_id uuid, content text, timestamp timestamp, PRIMARY KEY (channel_id, message_id)) WITH CLUSTERING ORDER BY (message_id DESC);"
docker exec cassandra-test cqlsh -e "USE chat; CREATE TABLE IF NOT EXISTS messages_by_user (user_id uuid, message_id timeuuid, channel_id uuid, content text, timestamp timestamp, PRIMARY KEY (user_id, message_id)) WITH CLUSTERING ORDER BY (message_id DESC);"

echo "Running tests..."
export SQLX_OFFLINE=true
export DATABASE_URL="postgresql://postgres:postgres@localhost:5433/postgres"
export CASSANDRA_NODES="localhost:9043"
export KAFKA__BROKERS="localhost:9093"
export USER_SERVICE_GRPC_URL="http://localhost:50052"

cargo test --all -- --test-threads=1

echo "Cleaning up..."
docker-compose -f docker-compose.test.yml down -v

echo "Done!"
