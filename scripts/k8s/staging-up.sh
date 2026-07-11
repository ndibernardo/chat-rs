#!/usr/bin/env bash
set -euo pipefail

# Brings up the chat-rs-staging namespace on the same kind cluster
# scripts/k8s/dev-up.sh brings up. Assumes that script already ran at least
# once — the kind cluster, Argo CD, and every cluster-wide operator (Strimzi,
# CNPG, Scylla Operator, cert-manager, ingress-nginx, metrics-server,
# prometheus-adapter, kube-prometheus-stack) are shared with dev and are not
# reinstalled here.
#
# Default path applies this repo's two staging Applications
# (deploy/argocd/{infra,app}-staging.yaml) and waits for Argo to sync them —
# dev and staging are deployed by separate scripts on purpose, so bringing up
# one never silently deploys the other. --direct installs via plain
# `helm upgrade` instead, from the working tree — never mix the two on the
# same namespace, they'll fight over resource ownership.

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd))"
cd "$REPO_ROOT"

CLUSTER_NAME=chat-rs
KIND_CONTEXT="kind-${CLUSTER_NAME}"
NAMESPACE=chat-rs-staging

# Must track scripts/k8s/dev-up.sh's STRIMZI_CHART_VERSION — re-verify both
# together against Artifact Hub before bumping either.
readonly STRIMZI_CHART_VERSION=1.1.0

# Stage 1: Strimzi's chart creates a RoleBinding per watched namespace, so
# chat-rs-staging must exist first (idempotent apply).
create_namespace() {
  kubectl --context "$KIND_CONTEXT" create namespace "$NAMESPACE" \
    --dry-run=client -o yaml | kubectl --context "$KIND_CONTEXT" apply -f -
}

# Stage 2: re-upgrade is a no-op if scripts/k8s/dev-up.sh already applied
# deploy/operators/strimzi.yaml's current watchNamespaces — but if this
# script runs against a cluster that was set up before chat-rs-staging was
# added there, this is what actually grants Strimzi the RoleBinding it needs.
reupgrade_strimzi() {
  helm repo add strimzi https://strimzi.io/charts/ --force-update
  helm repo update

  helm upgrade --install strimzi-kafka-operator strimzi/strimzi-kafka-operator \
    --version "$STRIMZI_CHART_VERSION" \
    --namespace strimzi --create-namespace \
    -f deploy/operators/strimzi.yaml \
    --wait --timeout 5m
}

# Stage 3: install Kafka, Postgres, and Scylla custom resources into the
# staging namespace from the same repo-authored infra chart dev uses, dev-
# shaped values.
install_infra() {
  helm upgrade --install chat-rs-infra deploy/charts/chat-rs-infra \
    --namespace "$NAMESPACE" \
    -f deploy/charts/chat-rs-infra/values-staging.yaml \
    --wait --timeout 10m
}

# Stage 4: block until every operator has actually reconciled its CRs —
# mirrors scripts/k8s/dev-up.sh's wait_for_infra.
wait_for_infra() {
  kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" wait kafka/chat-kafka \
    --for=condition=Ready --timeout=600s

  kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" wait cluster/user-db cluster/chat-db \
    --for=condition=Ready --timeout=600s

  local attempt
  for attempt in $(seq 1 60); do
    local ready
    ready="$(kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" get scyllacluster/chat-scylla \
      -o jsonpath='{.status.racks.rack1.readyMembers}' 2>/dev/null || true)"
    local wanted
    wanted="$(kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" get scyllacluster/chat-scylla \
      -o jsonpath='{.spec.datacenter.racks[0].members}')"

    if [[ -n "$ready" && "$ready" == "$wanted" ]]; then
      echo "ScyllaCluster chat-scylla: $ready/$wanted rack members ready"
      return 0
    fi

    echo "waiting for ScyllaCluster chat-scylla rack members (${ready:-0}/${wanted}), attempt $attempt/60"
    sleep 10
  done

  echo "ScyllaCluster chat-scylla did not reach ready member count in time" >&2
  exit 1
}

# Stage 5: install the chat-rs app chart into the staging namespace with its
# own keypair — a distinct release name isn't needed since Helm releases are
# themselves namespace-scoped, so "chat-rs" here never collides with dev's.
install_app() {
  helm upgrade --install chat-rs deploy/charts/chat-rs \
    --namespace "$NAMESPACE" \
    -f deploy/charts/chat-rs/values-staging.yaml \
    --set-file jwt.privateKey=keys/staging/jwt_ed25519.pem \
    --set-file jwt.publicKey=keys/staging/jwt_ed25519.pub.pem \
    --wait --timeout 10m
}

# Stage 6 (GitOps path only): apply this repo's two staging Applications and
# wait for Argo to sync + report Healthy. dev's script owns cluster-issuer
# and its own two Applications — this one only ever touches staging's.
apply_argocd_apps() {
  kubectl --context "$KIND_CONTEXT" apply \
    -f deploy/argocd/infra-staging.yaml \
    -f deploy/argocd/app-staging.yaml

  local app
  for app in chat-rs-infra-staging chat-rs-app-staging; do
    echo "waiting for Application/${app} to reach Healthy..."
    kubectl --context "$KIND_CONTEXT" -n argocd wait "application/${app}" \
      --for=jsonpath='{.status.health.status}'=Healthy --timeout=600s
  done
}

main() {
  local direct=false
  if [[ "${1:-}" == "--direct" ]]; then
    direct=true
  fi

  create_namespace
  reupgrade_strimzi

  if [[ "$direct" == "true" ]]; then
    install_infra
    wait_for_infra
    install_app
  else
    apply_argocd_apps
  fi

  echo
  echo "Done: chat-rs-staging is up on cluster '$CLUSTER_NAME'."
  echo "Routed via Ingress at chat.staging.local, or port-forward directly:"
  echo "  kubectl --context $KIND_CONTEXT -n $NAMESPACE port-forward svc/user-service 3001:3001"
  echo "  kubectl --context $KIND_CONTEXT -n $NAMESPACE port-forward svc/chat-api 3002:3002"
  echo "  kubectl --context $KIND_CONTEXT -n $NAMESPACE port-forward svc/chat-ws-gateway 3003:3002"
}

main "$@"
