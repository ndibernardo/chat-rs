#!/usr/bin/env bash
set -euo pipefail

# Rolling-restart proof for chat-rs-staging, all traffic routed through
# Ingress (no port-forward, no real DNS — curl's --resolve fakes
# chat.staging.local -> 127.0.0.1; websocat has no --resolve equivalent, so
# it uses the ws-c: overlay + --ws-c-uri instead, see ws_monitor). Registers two
# users and a channel, starts background HTTP load plus a WebSocket
# reconnect monitor, triggers a rolling restart on the three HTTP-facing
# Deployments, and asserts the run stayed clean throughout. Exit 0 is the
# Phase 2 exit criterion.

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd))"
cd "$REPO_ROOT"

CLUSTER_NAME=chat-rs
KIND_CONTEXT="kind-${CLUSTER_NAME}"
NAMESPACE=chat-rs-staging
HOST=chat.staging.local
BASE_URL="http://${HOST}"

# Unique per run so reruns never collide with a previous run's users/channel.
RUN_SUFFIX="$(date +%s)"
USER_A_USERNAME="emily-jones-${RUN_SUFFIX}"
USER_A_EMAIL="emily.jones+${RUN_SUFFIX}@example.com"
USER_A_PASSWORD="Summer-Meadow_2022!"
USER_B_USERNAME="robert-brown-${RUN_SUFFIX}"
USER_B_EMAIL="robert.brown+${RUN_SUFFIX}@example.com"
USER_B_PASSWORD="Golden-Harbor_2021!"
CHANNEL_NAME="incident-review-${RUN_SUFFIX}"

# ~10 requests/sec per loop — deliberately not login (an argon2 loop would
# skew CPU and confuse the HPA's own scaling signal).
LOAD_INTERVAL_SECONDS=0.1
# How long to keep the background load running after the rollout itself
# reports done, to catch any trailing reconnect gap.
POST_ROLLOUT_GRACE_SECONDS=15
# A close->reconnect gap at or above this fails the run.
MAX_RECONNECT_GAP_SECONDS=5

WORKDIR="$(mktemp -d)"
declare -a BACKGROUND_PIDS=()
WS_MONITOR_PID=""
CURRENT_WS_PID_FILE="$WORKDIR/ws-current.pid"
HISTORY_STATUS_LOG="$WORKDIR/history-status.log"
USER_GET_STATUS_LOG="$WORKDIR/user-get-status.log"
WS_EVENTS_LOG="$WORKDIR/ws-events.log"
: >"$HISTORY_STATUS_LOG"
: >"$USER_GET_STATUS_LOG"
: >"$WS_EVENTS_LOG"

cleanup() {
  [[ -n "$WS_MONITOR_PID" ]] && kill "$WS_MONITOR_PID" >/dev/null 2>&1 || true
  if [[ -f "$CURRENT_WS_PID_FILE" ]]; then
    kill "$(cat "$CURRENT_WS_PID_FILE")" >/dev/null 2>&1 || true
  fi
  local pid
  for pid in "${BACKGROUND_PIDS[@]:-}"; do
    kill "$pid" >/dev/null 2>&1 || true
  done
  wait >/dev/null 2>&1 || true
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

fail() {
  local step="$1"
  local detail_file="${2:-}"
  echo
  echo "FAIL: ${step}" >&2
  if [[ -n "$detail_file" && -f "$detail_file" ]]; then
    echo "--- last response/output (${detail_file}) ---" >&2
    cat "$detail_file" >&2
  fi
  exit 1
}

# Captures the response body in $3 and leaves the HTTP status in $HTTP_STATUS.
HTTP_STATUS=""
http_post() {
  local path="$1" body="$2" outfile="$3" token="${4:-}"
  local -a auth_header=()
  [[ -n "$token" ]] && auth_header=(-H "Authorization: Bearer ${token}")
  HTTP_STATUS="$(curl -sS -o "$outfile" -w '%{http_code}' -X POST \
    --resolve "${HOST}:80:127.0.0.1" "${BASE_URL}${path}" \
    -H 'Content-Type: application/json' \
    "${auth_header[@]}" \
    -d "$body")" || true
  [[ -n "$HTTP_STATUS" ]] || fail "POST ${path} (connection failed)" "$outfile"
}

http_get() {
  local path="$1" outfile="$2" token="${3:-}"
  local -a auth_header=()
  [[ -n "$token" ]] && auth_header=(-H "Authorization: Bearer ${token}")
  HTTP_STATUS="$(curl -sS -o "$outfile" -w '%{http_code}' \
    --resolve "${HOST}:80:127.0.0.1" "${BASE_URL}${path}" \
    "${auth_header[@]}")" || true
  [[ -n "$HTTP_STATUS" ]] || fail "GET ${path} (connection failed)" "$outfile"
}

# Stage 1: both migrate Jobs must already be Complete before anything else —
# a stuck migration means the schema, not the rollout, is what's broken.
assert_migrate_jobs_completed() {
  local job status
  for job in chat-migrate user-migrate; do
    status="$(kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" get "job/${job}" \
      -o jsonpath='{.status.conditions[?(@.type=="Complete")].status}' 2>/dev/null || true)"
    [[ "$status" == "True" ]] || fail "migrate Job ${job} is not Complete in namespace ${NAMESPACE}"
  done
  echo "migrate Jobs Completed: chat-migrate, user-migrate"
}

# Stage 2: register both users (tolerating 409 on a same-second rerun) and
# log in via /api/auth/login — the route the K6 table omits but everything
# downstream depends on.
register_user() {
  local username="$1" email="$2" password="$3"
  local body outfile
  body="$(jq -n --arg u "$username" --arg e "$email" --arg p "$password" \
    '{username: $u, email: $e, password: $p}')"
  outfile="$WORKDIR/register-${username}.json"
  http_post "/api/users" "$body" "$outfile"
  case "$HTTP_STATUS" in
    201) ;;
    409) echo "user ${username} already exists, continuing" ;;
    *) fail "register user ${username} (HTTP ${HTTP_STATUS})" "$outfile" ;;
  esac
}

LOGIN_TOKEN=""
LOGIN_USER_ID=""
login_user() {
  local username="$1" password="$2"
  local body outfile
  body="$(jq -n --arg u "$username" --arg p "$password" '{username: $u, password: $p}')"
  outfile="$WORKDIR/login-${username}.json"
  http_post "/api/auth/login" "$body" "$outfile"
  [[ "$HTTP_STATUS" == "200" ]] || fail "login user ${username} (HTTP ${HTTP_STATUS})" "$outfile"
  LOGIN_TOKEN="$(jq -r '.data.token' "$outfile")"
  LOGIN_USER_ID="$(jq -r '.data.user.id' "$outfile")"
}

register_and_login_users() {
  register_user "$USER_A_USERNAME" "$USER_A_EMAIL" "$USER_A_PASSWORD"
  register_user "$USER_B_USERNAME" "$USER_B_EMAIL" "$USER_B_PASSWORD"

  login_user "$USER_A_USERNAME" "$USER_A_PASSWORD"
  USER_A_TOKEN="$LOGIN_TOKEN"
  USER_A_ID="$LOGIN_USER_ID"

  login_user "$USER_B_USERNAME" "$USER_B_PASSWORD"
  USER_B_TOKEN="$LOGIN_TOKEN"
  USER_B_ID="$LOGIN_USER_ID"

  [[ -n "$USER_A_TOKEN" && "$USER_A_TOKEN" != "null" ]] || fail "user A JWT missing from login response"
  [[ -n "$USER_B_TOKEN" && "$USER_B_TOKEN" != "null" ]] || fail "user B JWT missing from login response"

  echo "registered + logged in via Ingress: A=${USER_A_ID} (${USER_A_USERNAME}), B=${USER_B_ID} (${USER_B_USERNAME})"
}

# Stage 3: A creates a private channel with B as a member, retrying past the
# cluster still warming up rather than a replica-lag gate (channel_members
# has no FK to the user-events replica table).
create_channel_with_member_b() {
  local body outfile attempt
  body="$(jq -n --arg n "$CHANNEL_NAME" --arg m "$USER_B_ID" \
    '{channel_type: "private", name: $n, members: [$m]}')"
  outfile="$WORKDIR/create-channel.json"

  for attempt in $(seq 1 10); do
    http_post "/api/channels" "$body" "$outfile" "$USER_A_TOKEN"
    if [[ "$HTTP_STATUS" == "201" ]]; then
      CHANNEL_ID="$(jq -r '.id' "$outfile")"
      echo "channel created: ${CHANNEL_ID} (${CHANNEL_NAME})"
      return 0
    fi
    echo "create channel attempt ${attempt}/10 got HTTP ${HTTP_STATUS}, retrying..."
    sleep 1
  done
  fail "create channel with B as member (HTTP ${HTTP_STATUS})" "$outfile"
}

# Stage 4a: chat-api history GET, ~10/s, authed as A.
history_load_loop() {
  while true; do
    local status
    status="$(curl -sS -o /dev/null -w '%{http_code}' \
      --resolve "${HOST}:80:127.0.0.1" \
      -H "Authorization: Bearer ${USER_A_TOKEN}" \
      "${BASE_URL}/api/channels/${CHANNEL_ID}/messages" 2>/dev/null)" || true
    echo "${status:-000}" >>"$HISTORY_STATUS_LOG"
    sleep "$LOAD_INTERVAL_SECONDS"
  done
}

# Stage 4b: a cheap authed user-service GET, ~10/s — not login, see the
# LOAD_INTERVAL_SECONDS comment above for why.
user_get_load_loop() {
  while true; do
    local status
    status="$(curl -sS -o /dev/null -w '%{http_code}' \
      --resolve "${HOST}:80:127.0.0.1" \
      -H "Authorization: Bearer ${USER_A_TOKEN}" \
      "${BASE_URL}/api/users/${USER_A_ID}" 2>/dev/null)" || true
    echo "${status:-000}" >>"$USER_GET_STATUS_LOG"
    sleep "$LOAD_INTERVAL_SECONDS"
  done
}

# Stage 4c: B holds a WebSocket open and auto-reconnects on close, logging
# CONNECTED/CLOSED epochs. Connects to the literal loopback address with an
# explicit Host header — no /etc/hosts entry, no real DNS.
ws_monitor() {
  while true; do
    local frames_file
    frames_file="$(mktemp -p "$WORKDIR" ws-frames.XXXXXX)"
    # -n/--no-close: without it, /dev/null's immediate EOF makes websocat
    # send its own WS Close frame right after connecting — every cycle would
    # self-close instantly regardless of the actual rolling restart, and the
    # reconnect-gap measurement below would be meaningless.
    #
    # ws-c:tcp:127.0.0.1:80 + --ws-c-uri: websocat's `-H "Host: ..."` only
    # *adds* a header, it doesn't replace the one websocat auto-generates
    # from the connection URL — so a plain `ws://127.0.0.1/...` request
    # carries two Host headers, nginx's vhost match picks the wrong
    # (auto-generated, IP-literal) one, and every request 404s on the
    # default backend. The ws-c: low-level connector splits the raw TCP
    # target from the logical URI used to build the request, so exactly one
    # correct Host header goes out. Confirmed live against a real Ingress —
    # a plain URL + -H override reliably 404s, this doesn't.
    websocat -n -t - "ws-c:tcp:127.0.0.1:80" \
      --ws-c-uri "ws://${HOST}/ws/channels/${CHANNEL_ID}" \
      --protocol "bearer, ${USER_B_TOKEN}" \
      </dev/null >"$frames_file" 2>>"$WORKDIR/ws-monitor.err" &
    local ws_pid=$!
    echo "$ws_pid" >"$CURRENT_WS_PID_FILE"

    local waited=0
    while (( waited < 15 )) && kill -0 "$ws_pid" 2>/dev/null; do
      if grep -q '"type":"connected"' "$frames_file" 2>/dev/null; then
        echo "CONNECTED $(date +%s)" >>"$WS_EVENTS_LOG"
        break
      fi
      sleep 1
      ((++waited))
    done

    wait "$ws_pid" 2>/dev/null || true
    echo "CLOSED $(date +%s)" >>"$WS_EVENTS_LOG"
    rm -f "$frames_file"
    sleep 0.2
  done
}

start_background_load() {
  history_load_loop &
  BACKGROUND_PIDS+=($!)
  user_get_load_loop &
  BACKGROUND_PIDS+=($!)

  ws_monitor &
  WS_MONITOR_PID=$!

  # Give the first WS connection a moment to land before the rollout starts,
  # so the drill can't spuriously "pass" on zero observed activity.
  local waited=0
  while (( waited < 15 )); do
    grep -q '^CONNECTED' "$WS_EVENTS_LOG" 2>/dev/null && break
    sleep 1
    ((++waited))
  done
  grep -q '^CONNECTED' "$WS_EVENTS_LOG" 2>/dev/null \
    || fail "WebSocket monitor never reached Connected state" "$WORKDIR/ws-monitor.err"
  echo "background load running: chat-api history GET, user-service GET, WS reconnect monitor"
}

# Stage 5: roll the three HTTP-facing Deployments and wait for each rollout
# to finish before moving on.
trigger_rolling_restart() {
  kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" rollout restart \
    deployment/user-service deployment/chat-api deployment/chat-ws-gateway

  local deploy
  for deploy in user-service chat-api chat-ws-gateway; do
    kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" rollout status \
      "deployment/${deploy}" --timeout=180s
  done
  echo "rolling restart complete on user-service, chat-api, chat-ws-gateway"
}

stop_background_load() {
  echo "letting background load run ${POST_ROLLOUT_GRACE_SECONDS}s past rollout completion..."
  sleep "$POST_ROLLOUT_GRACE_SECONDS"

  kill "$WS_MONITOR_PID" >/dev/null 2>&1 || true
  WS_MONITOR_PID=""
  if [[ -f "$CURRENT_WS_PID_FILE" ]]; then
    kill "$(cat "$CURRENT_WS_PID_FILE")" >/dev/null 2>&1 || true
  fi

  local pid
  for pid in "${BACKGROUND_PIDS[@]:-}"; do
    kill "$pid" >/dev/null 2>&1 || true
  done
  BACKGROUND_PIDS=()
  wait >/dev/null 2>&1 || true
}

# Stage 6a: zero non-2xx across the whole run — a `000` (curl's own
# not-a-status-code) counts as a dropped connection, not a pass.
assert_no_non_2xx() {
  local log="$1" name="$2"
  local total bad
  total="$(wc -l <"$log" | tr -d ' ')"
  bad="$(grep -cvE '^2[0-9][0-9]$' "$log" || true)"
  if [[ "$total" -eq 0 ]]; then
    fail "${name}: no requests were recorded at all" "$log"
  fi
  if [[ "$bad" -gt 0 ]]; then
    echo "--- ${name}: non-2xx status breakdown ---" >&2
    grep -vE '^2[0-9][0-9]$' "$log" | sort | uniq -c >&2
    fail "${name}: ${bad}/${total} requests were non-2xx (including dropped connections)"
  fi
  echo "${name}: ${total}/${total} requests returned 2xx"
}

# Stage 6b: at least one observed close, and every close is followed by a
# reconnect within MAX_RECONNECT_GAP_SECONDS.
assert_ws_reconnects() {
  local closes
  closes="$(grep -c '^CLOSED' "$WS_EVENTS_LOG" || true)"
  if [[ "$closes" -lt 1 ]]; then
    fail "no WebSocket close observed during the rolling restart" "$WS_EVENTS_LOG"
  fi
  echo "observed ${closes} WebSocket close(s) during the run"

  local prev_event="" prev_epoch="" event epoch gap
  while read -r event epoch; do
    if [[ "$event" == "CLOSED" ]]; then
      prev_event="CLOSED"
      prev_epoch="$epoch"
    elif [[ "$event" == "CONNECTED" && "$prev_event" == "CLOSED" ]]; then
      gap=$(( epoch - prev_epoch ))
      if (( gap >= MAX_RECONNECT_GAP_SECONDS )); then
        fail "reconnect gap ${gap}s (>= ${MAX_RECONNECT_GAP_SECONDS}s) after close at epoch ${prev_epoch}" "$WS_EVENTS_LOG"
      fi
      prev_event=""
    fi
  done <"$WS_EVENTS_LOG"
  echo "every close -> reconnect gap stayed under ${MAX_RECONNECT_GAP_SECONDS}s"
}

main() {
  assert_migrate_jobs_completed
  register_and_login_users
  create_channel_with_member_b
  start_background_load
  trigger_rolling_restart
  stop_background_load

  assert_no_non_2xx "$HISTORY_STATUS_LOG" "chat-api channel history GET"
  assert_no_non_2xx "$USER_GET_STATUS_LOG" "user-service authed GET"
  assert_ws_reconnects

  echo
  echo "PASS: rolling restart survived with zero non-2xx HTTP responses and"
  echo "every WebSocket close reconnected within ${MAX_RECONNECT_GAP_SECONDS}s"
  echo "  user A: ${USER_A_ID} (${USER_A_USERNAME})"
  echo "  user B: ${USER_B_ID} (${USER_B_USERNAME})"
  echo "  channel: ${CHANNEL_ID} (${CHANNEL_NAME})"
}

main "$@"
