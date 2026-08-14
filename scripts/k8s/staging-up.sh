#!/usr/bin/env bash
set -euo pipefail

# Brings up the chat-staging namespace on the same kind cluster
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

CLUSTER_NAME=chat
KIND_CONTEXT="kind-${CLUSTER_NAME}"
NAMESPACE=chat-staging
STAGING_JWT_PRIVATE_KEY_FILE="${STAGING_JWT_PRIVATE_KEY_FILE:-}"
STAGING_JWT_PUBLIC_KEY_FILE="${STAGING_JWT_PUBLIC_KEY_FILE:-}"

# Must track scripts/k8s/dev-up.sh's STRIMZI_CHART_VERSION — re-verify both
# together against Artifact Hub before bumping either.
readonly STRIMZI_CHART_VERSION=1.1.0

prepare_staging_jwt_files() {
  if [[ -z "$STAGING_JWT_PRIVATE_KEY_FILE" || -z "$STAGING_JWT_PUBLIC_KEY_FILE" ]]; then
    echo "STAGING_JWT_PRIVATE_KEY_FILE and STAGING_JWT_PUBLIC_KEY_FILE are required." >&2
    echo "Point both variables at the rotated Ed25519 keypair stored outside this repository." >&2
    exit 1
  fi

  STAGING_JWT_PRIVATE_KEY_FILE="$(realpath --canonicalize-existing "$STAGING_JWT_PRIVATE_KEY_FILE")"
  STAGING_JWT_PUBLIC_KEY_FILE="$(realpath --canonicalize-existing "$STAGING_JWT_PUBLIC_KEY_FILE")"

  if [[ "$STAGING_JWT_PRIVATE_KEY_FILE" == "$REPO_ROOT/"* ||
        "$STAGING_JWT_PUBLIC_KEY_FILE" == "$REPO_ROOT/"* ]]; then
    echo "Staging JWT key files must live outside the repository: $REPO_ROOT" >&2
    exit 1
  fi

  if ! openssl pkey -in "$STAGING_JWT_PRIVATE_KEY_FILE" -noout -check >/dev/null 2>&1; then
    echo "STAGING_JWT_PRIVATE_KEY_FILE is not a valid private key." >&2
    exit 1
  fi

  if ! openssl pkey -in "$STAGING_JWT_PRIVATE_KEY_FILE" -pubout 2>/dev/null |
       cmp -s - "$STAGING_JWT_PUBLIC_KEY_FILE"; then
    echo "The staging JWT public key does not match the private key." >&2
    exit 1
  fi
}

provision_staging_jwt() {
  kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" create secret generic jwt-signing-key \
    --from-file=jwt_ed25519.pem="$STAGING_JWT_PRIVATE_KEY_FILE" \
    --dry-run=client -o yaml | kubectl --context "$KIND_CONTEXT" apply \
      --server-side --field-manager=staging-key-provisioner --force-conflicts -f -

  kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" create configmap jwt-public-key \
    --from-file=jwt_ed25519.pub.pem="$STAGING_JWT_PUBLIC_KEY_FILE" \
    --dry-run=client -o yaml | kubectl --context "$KIND_CONTEXT" apply \
      --server-side --field-manager=staging-key-provisioner --force-conflicts -f -

  local resource
  for resource in secret/jwt-signing-key configmap/jwt-public-key; do
    kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" annotate "$resource" \
      argocd.argoproj.io/tracking-id- \
      meta.helm.sh/release-name- \
      meta.helm.sh/release-namespace- \
      kubectl.kubernetes.io/last-applied-configuration- \
      --overwrite >/dev/null 2>&1 || true
    kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" label "$resource" \
      app.kubernetes.io/instance- \
      app.kubernetes.io/managed-by- \
      --overwrite >/dev/null 2>&1 || true
  done
}

# Stage 1: Strimzi's chart creates a RoleBinding per watched namespace, so
# chat-staging must exist first (idempotent apply).
create_namespace() {
  kubectl --context "$KIND_CONTEXT" create namespace "$NAMESPACE" \
    --dry-run=client -o yaml | kubectl --context "$KIND_CONTEXT" apply -f -
}

# Stage 2: re-upgrade is a no-op if scripts/k8s/dev-up.sh already applied
# deploy/operators/strimzi.yaml's current watchNamespaces — but if this
# script runs against a cluster that was set up before chat-staging was
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
  helm upgrade --install chat-infra deploy/charts/chat-infra \
    --namespace "$NAMESPACE" \
    -f deploy/charts/chat-infra/values-staging.yaml \
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

# Stage 5: install the chat app chart with externally provisioned JWT resources.
install_app() {
  if helm --namespace "$NAMESPACE" status chat >/dev/null 2>&1 &&
     helm --namespace "$NAMESPACE" get manifest chat |
       grep -q '# Source: chat/templates/jwt/secret.yaml'; then
    helm upgrade chat deploy/charts/chat \
      --namespace "$NAMESPACE" \
      -f deploy/charts/chat/values-staging.yaml \
      --no-hooks
  fi

  provision_staging_jwt

  helm upgrade --install chat deploy/charts/chat \
    --namespace "$NAMESPACE" \
    -f deploy/charts/chat/values-staging.yaml \
    --wait --timeout 10m
}

# Stage 6 (GitOps path only): apply this repo's two staging Applications and
# wait for Argo to sync + report Healthy. dev's script owns cluster-issuer
# and its own two Applications — this one only ever touches staging's.
apply_argocd_apps() {
  provision_staging_jwt

  kubectl --context "$KIND_CONTEXT" apply \
    -f deploy/argocd/infra-staging.yaml \
    -f deploy/argocd/app-staging.yaml

  local app
  for app in chat-infra-staging chat-app-staging; do
    echo "waiting for Application/${app} to reach Healthy..."
    kubectl --context "$KIND_CONTEXT" -n argocd wait "application/${app}" \
      --for=jsonpath='{.status.health.status}'=Healthy --timeout=600s
  done

  provision_staging_jwt
}

main() {
  local direct=false
  if [[ "${1:-}" == "--direct" ]]; then
    direct=true
  fi

  prepare_staging_jwt_files
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
  echo "Done: chat-staging is up on cluster '$CLUSTER_NAME'."
  echo "Routed via Ingress at chat.staging.local, or port-forward directly:"
  echo "  kubectl --context $KIND_CONTEXT -n $NAMESPACE port-forward svc/user-service 3001:3001"
  echo "  kubectl --context $KIND_CONTEXT -n $NAMESPACE port-forward svc/chat-api 3002:3002"
  echo "  kubectl --context $KIND_CONTEXT -n $NAMESPACE port-forward svc/chat-ws-gateway 3003:3002"
}

main "$@"
