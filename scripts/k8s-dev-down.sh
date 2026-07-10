#!/usr/bin/env bash
set -euo pipefail

CLUSTER_NAME=chat-rs

# kind delete cluster already no-ops on a missing cluster, so this is
# idempotent without an extra existence check.
kind delete cluster --name "$CLUSTER_NAME"
