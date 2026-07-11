#!/usr/bin/env bash
set -euo pipefail

# Repo root, so the script works regardless of the caller's cwd.
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd))"
cd "$REPO_ROOT"

CLUSTER_NAME=chat-rs
KIND_CONTEXT="kind-${CLUSTER_NAME}"
KIND_CONFIG=deploy/kind/cluster.yaml
USER_SERVICE_IMAGE=chat-rs/user-service:dev
CHAT_SERVICE_IMAGE=chat-rs/chat-service:dev

# Pinned from a live `helm search repo` on 2026-07-10 (ingress-nginx added
# 2026-07-11) — bump deliberately, never from memory, and re-verify against
# Artifact Hub / the upstream repo before changing any of these.
readonly CERT_MANAGER_CHART_VERSION=v1.21.0
readonly KUBE_PROMETHEUS_STACK_CHART_VERSION=87.12.5
readonly STRIMZI_CHART_VERSION=1.1.0
readonly CLOUDNATIVE_PG_CHART_VERSION=0.29.0
readonly SCYLLA_OPERATOR_CHART_VERSION=v1.21.0
readonly EXTERNAL_SECRETS_CHART_VERSION=2.7.0
readonly INGRESS_NGINX_CHART_VERSION=4.15.1
readonly METRICS_SERVER_CHART_VERSION=3.13.1
readonly PROMETHEUS_ADAPTER_CHART_VERSION=5.3.0
readonly ARGOCD_CHART_VERSION=10.1.3
readonly HEADLAMP_CHART_VERSION=0.43.0

# GitHub remote this repo's own Applications point back at — the GitOps path
# deploys whatever's on the pushed main branch there, never the working tree.
readonly REPO_URL=https://github.com/ndibernardo/chat-rs.git

# Stage 1: cluster must exist and be reachable before anything else runs.
create_cluster() {
  if kind get clusters | grep -qx "$CLUSTER_NAME"; then
    echo "kind cluster '$CLUSTER_NAME' already exists, skipping create"
  else
    kind create cluster --name "$CLUSTER_NAME" --config "$KIND_CONFIG"
  fi
  kubectl --context "$KIND_CONTEXT" wait --for=condition=Ready nodes --all --timeout=120s
}

# Stage 2: build service images and load them straight into the kind node,
# skipping a registry round-trip entirely.
build_and_load_images() {
  docker build -f services/user-service/Dockerfile -t "$USER_SERVICE_IMAGE" .
  docker build -f services/chat-service/Dockerfile -t "$CHAT_SERVICE_IMAGE" .

  kind load docker-image "$USER_SERVICE_IMAGE" --name "$CLUSTER_NAME"
  kind load docker-image "$CHAT_SERVICE_IMAGE" --name "$CLUSTER_NAME"

  # pullPolicy: IfNotPresent means an unchanged :dev tag won't be repulled —
  # a rebuild needs `kubectl rollout restart` on the affected Deployments.
  echo "Note: after rebuilding an image with the same :dev tag, run 'kubectl rollout restart deployment/<name>' to pick it up."
}

# Stage 3: install every operator from its pinned upstream chart. Order
# matters — cert-manager first because the Scylla Operator's webhooks depend
# on cert-manager-issued certs, kube-prometheus-stack early so its
# ServiceMonitor/PodMonitor CRDs exist before later charts want to use them.
install_operators() {
  helm repo add jetstack https://charts.jetstack.io --force-update
  helm repo add prometheus-community https://prometheus-community.github.io/helm-charts --force-update
  helm repo add strimzi https://strimzi.io/charts/ --force-update
  helm repo add cnpg https://cloudnative-pg.github.io/charts --force-update
  helm repo add scylla https://storage.googleapis.com/scylla-operator-charts/stable --force-update
  helm repo add external-secrets https://charts.external-secrets.io --force-update
  helm repo add ingress-nginx https://kubernetes.github.io/ingress-nginx --force-update
  helm repo add metrics-server https://kubernetes-sigs.github.io/metrics-server/ --force-update
  helm repo add argo https://argoproj.github.io/argo-helm --force-update
  helm repo add headlamp https://kubernetes-sigs.github.io/headlamp/ --force-update
  helm repo update

  helm upgrade --install cert-manager jetstack/cert-manager \
    --version "$CERT_MANAGER_CHART_VERSION" \
    --namespace cert-manager --create-namespace \
    -f deploy/operators/cert-manager.yaml \
    --wait --timeout 5m

  helm upgrade --install kube-prometheus-stack prometheus-community/kube-prometheus-stack \
    --version "$KUBE_PROMETHEUS_STACK_CHART_VERSION" \
    --namespace monitoring --create-namespace \
    -f deploy/operators/kube-prometheus-stack.yaml \
    --wait --timeout 5m

  helm upgrade --install metrics-server metrics-server/metrics-server \
    --version "$METRICS_SERVER_CHART_VERSION" \
    --namespace kube-system \
    -f deploy/operators/metrics-server.yaml \
    --wait --timeout 5m

  # Reads kube-prometheus-stack's Prometheus Service — must land after it.
  helm upgrade --install prometheus-adapter prometheus-community/prometheus-adapter \
    --version "$PROMETHEUS_ADAPTER_CHART_VERSION" \
    --namespace monitoring \
    -f deploy/operators/prometheus-adapter.yaml \
    --wait --timeout 5m

  # Strimzi's chart creates RoleBindings inside every watched namespace, so
  # both chat-rs and chat-rs-staging must exist before the operator installs
  # (idempotent apply) — watchNamespaces lists chat-rs-staging even though
  # scripts/k8s/staging-up.sh, not this script, deploys anything into it.
  kubectl --context "$KIND_CONTEXT" create namespace chat-rs \
    --dry-run=client -o yaml | kubectl --context "$KIND_CONTEXT" apply -f -
  kubectl --context "$KIND_CONTEXT" create namespace chat-rs-staging \
    --dry-run=client -o yaml | kubectl --context "$KIND_CONTEXT" apply -f -

  helm upgrade --install strimzi-kafka-operator strimzi/strimzi-kafka-operator \
    --version "$STRIMZI_CHART_VERSION" \
    --namespace strimzi --create-namespace \
    -f deploy/operators/strimzi.yaml \
    --wait --timeout 5m

  helm upgrade --install cloudnative-pg cnpg/cloudnative-pg \
    --version "$CLOUDNATIVE_PG_CHART_VERSION" \
    --namespace cnpg-system --create-namespace \
    -f deploy/operators/cloudnative-pg.yaml \
    --wait --timeout 5m

  helm upgrade --install scylla-operator scylla/scylla-operator \
    --version "$SCYLLA_OPERATOR_CHART_VERSION" \
    --namespace scylla-operator --create-namespace \
    -f deploy/operators/scylla-operator.yaml \
    --wait --timeout 5m

  helm upgrade --install external-secrets external-secrets/external-secrets \
    --version "$EXTERNAL_SECRETS_CHART_VERSION" \
    --namespace external-secrets --create-namespace \
    -f deploy/operators/external-secrets.yaml \
    --wait --timeout 5m

  helm upgrade --install ingress-nginx ingress-nginx/ingress-nginx \
    --version "$INGRESS_NGINX_CHART_VERSION" \
    --namespace ingress-nginx --create-namespace \
    -f deploy/operators/ingress-nginx.yaml \
    --wait --timeout 5m

  helm upgrade --install argocd argo/argo-cd \
    --version "$ARGOCD_CHART_VERSION" \
    --namespace argocd --create-namespace \
    -f deploy/operators/argo-cd.yaml \
    --wait --timeout 5m

  helm upgrade --install headlamp headlamp/headlamp \
    --version "$HEADLAMP_CHART_VERSION" \
    --namespace headlamp --create-namespace \
    -f deploy/operators/headlamp.yaml \
    --wait --timeout 5m
}

# Stage 4: `--wait` above only waits for each release's own resources, not
# for CRDs to be fully established or for the Scylla webhook to actually
# serve — closing that race here keeps it out of every later stage.
wait_for_operator_crds() {
  kubectl --context "$KIND_CONTEXT" wait --for condition=established --timeout=120s \
    crd/kafkas.kafka.strimzi.io \
    crd/kafkanodepools.kafka.strimzi.io \
    crd/kafkatopics.kafka.strimzi.io \
    crd/clusters.postgresql.cnpg.io \
    crd/scyllaclusters.scylla.scylladb.com \
    crd/servicemonitors.monitoring.coreos.com \
    crd/podmonitors.monitoring.coreos.com \
    crd/applications.argoproj.io

  # ScyllaCluster admission webhook must be serving before any ScyllaCluster
  # CR is applied, or the apply fails against an unready webhook endpoint.
  kubectl --context "$KIND_CONTEXT" -n scylla-operator rollout status deploy --timeout=300s

  # ingress-nginx's admission webhook must be serving before any Ingress
  # object applies, or the apply is rejected.
  kubectl --context "$KIND_CONTEXT" -n ingress-nginx rollout status deploy --timeout=300s
}

# Stage 5: install Kafka, Postgres, and Scylla custom resources from the
# repo-authored infra chart. `--wait` only covers the chart's own Kubernetes
# objects (the CRs themselves); it returns as soon as they're created, long
# before the operators finish reconciling Kafka/CNPG/Scylla into a Ready
# state — that's what the next stage polls for.
install_infra() {
  helm upgrade --install chat-rs-infra deploy/charts/chat-rs-infra \
    --namespace chat-rs \
    -f deploy/charts/chat-rs-infra/values-dev.yaml \
    --wait --timeout 10m
}

# Stage 6: block until every operator has actually reconciled its CRs,
# rather than trusting install_infra's `--wait`.
wait_for_infra() {
  kubectl --context "$KIND_CONTEXT" -n chat-rs wait kafka/chat-kafka \
    --for=condition=Ready --timeout=600s

  kubectl --context "$KIND_CONTEXT" -n chat-rs wait cluster/user-db cluster/chat-db \
    --for=condition=Ready --timeout=600s

  # ScyllaCluster has no single Ready condition that's stable across operator
  # versions, so poll the rack's member count directly instead.
  local attempt
  for attempt in $(seq 1 60); do
    local ready
    ready="$(kubectl --context "$KIND_CONTEXT" -n chat-rs get scyllacluster/chat-scylla \
      -o jsonpath='{.status.racks.rack1.readyMembers}' 2>/dev/null || true)"
    local wanted
    wanted="$(kubectl --context "$KIND_CONTEXT" -n chat-rs get scyllacluster/chat-scylla \
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

# Stage 7: install the chat-rs app chart — user-service and chat-service
# workloads together in one release. --set-file keeps the committed dev
# keypair as the single source of truth instead of copy-pasting PEM content
# into a values file; --wait also blocks on the pre-install migrate hooks,
# so this call doesn't return until both schemas are current.
install_app() {
  helm upgrade --install chat-rs deploy/charts/chat-rs \
    --namespace chat-rs \
    -f deploy/charts/chat-rs/values-dev.yaml \
    --set-file jwt.privateKey=keys/dev/jwt_ed25519.pem \
    --set-file jwt.publicKey=keys/dev/jwt_ed25519.pub.pem \
    --wait --timeout 10m
}

# Stage 8: GitOps path (the default). Applies the five Applications defined
# in deploy/argocd/ and waits for each to reconcile. A repo Secret is only
# needed for a private repo — GITHUB_TOKEN is optional on purpose.
install_argocd_apps() {
  if [[ -n "${GITHUB_TOKEN:-}" ]]; then
    kubectl --context "$KIND_CONTEXT" -n argocd create secret generic chat-rs-repo \
      --from-literal=type=git \
      --from-literal=url="$REPO_URL" \
      --from-literal=username=git \
      --from-literal=password="$GITHUB_TOKEN" \
      --dry-run=client -o yaml | kubectl --context "$KIND_CONTEXT" apply -f -
    kubectl --context "$KIND_CONTEXT" -n argocd label secret chat-rs-repo \
      argocd.argoproj.io/secret-type=repository --overwrite
  else
    echo "GITHUB_TOKEN not set — skipping repo Secret (only needed for a private repo)"
  fi

  # Dev-owned Applications only — cluster-issuer is cluster-wide and dev is
  # the natural first environment to own applying it, but staging's own two
  # Applications belong to scripts/k8s/staging-up.sh, not this script. Apply
  # both together would make "just bring up dev" silently deploy staging too.
  kubectl --context "$KIND_CONTEXT" apply \
    -f deploy/argocd/cluster-issuer.yaml \
    -f deploy/argocd/infra-dev.yaml \
    -f deploy/argocd/app-dev.yaml

  local app
  for app in chat-rs-cluster-issuer chat-rs-infra-dev chat-rs-app-dev; do
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

  create_cluster
  build_and_load_images
  install_operators
  wait_for_operator_crds

  if [[ "$direct" == "true" ]]; then
    # Escape hatch for testing unpushed changes — Argo only ever deploys
    # what's on the pushed main branch. Never mix this with the GitOps path
    # on the same namespace; the two will fight over resource ownership.
    #
    # Cluster-scoped, plain manifest — can't live in a chart that installs
    # into both chat-rs and chat-rs-staging under two Helm releases. Only
    # applied directly here: under the GitOps path below, the
    # chat-rs-cluster-issuer Application owns this object instead — applying
    # it both ways means Argo can never actually take ownership (it'll sit
    # permanently OutOfSync, unable to self-heal since that's off on purpose).
    kubectl --context "$KIND_CONTEXT" apply -f deploy/cluster/self-signed-clusterissuer.yaml
    install_infra
    wait_for_infra
    install_app
  else
    install_argocd_apps
  fi

  echo
  echo "Done: kind cluster '$CLUSTER_NAME' is up, images loaded, operators installed,"
  echo "and Kafka/Postgres/Scylla infra is Ready."
  echo "user-service and chat-service are deployed. Port-forward them with:"
  echo "  kubectl --context $KIND_CONTEXT -n chat-rs port-forward svc/user-service 3001:3001"
  echo "  kubectl --context $KIND_CONTEXT -n chat-rs port-forward svc/chat-api 3002:3002"
  echo "  kubectl --context $KIND_CONTEXT -n chat-rs port-forward svc/chat-ws-gateway 3003:3002"
  if [[ "$direct" != "true" ]]; then
    echo "Deployed via Argo CD from the pushed main branch. Port-forward the Argo UI with:"
    echo "  kubectl --context $KIND_CONTEXT -n argocd port-forward svc/argocd-server 8080:80"
  fi
  echo "Cluster dashboard (Headlamp): port-forward and grab a login token with"
  echo "  kubectl --context $KIND_CONTEXT -n headlamp port-forward svc/headlamp 4466:80"
  echo "  kubectl --context $KIND_CONTEXT -n headlamp create token headlamp"
}

main "$@"
