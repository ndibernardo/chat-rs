#!/usr/bin/env bash
set -euo pipefail

# One-environment-at-a-time release valve: tears down only the chat-rs-staging
# namespace and its two Helm releases, leaving the kind cluster, dev, and
# every cluster-wide operator untouched.

CLUSTER_NAME=chat-rs
KIND_CONTEXT="kind-${CLUSTER_NAME}"
NAMESPACE=chat-rs-staging

helm uninstall chat-rs --namespace "$NAMESPACE" --kube-context "$KIND_CONTEXT" --wait \
  || echo "release 'chat-rs' not found in ${NAMESPACE}, skipping"

helm uninstall chat-rs-infra --namespace "$NAMESPACE" --kube-context "$KIND_CONTEXT" --wait \
  || echo "release 'chat-rs-infra' not found in ${NAMESPACE}, skipping"

# Deletes the CNPG/Scylla PVCs, StatefulSets, and Jobs the two releases
# above left behind — Helm uninstall doesn't touch dynamically-provisioned
# storage, and a bare namespace delete is the simplest way to catch it all.
kubectl --context "$KIND_CONTEXT" delete namespace "$NAMESPACE" --ignore-not-found --wait --timeout=120s
