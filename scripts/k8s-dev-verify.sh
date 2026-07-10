#!/usr/bin/env bash
set -euo pipefail

# End-to-end smoke test against the local kind cluster: register two users,
# create a channel between them, send a message over the WebSocket gateway,
# and confirm it both fans out live and lands in history. Exit 0 means the
# full path — user-service -> Kafka user-events -> chat-worker replica,
# chat-ws-gateway -> Kafka messages -> chat-worker persister -> Scylla,
# surfaced back through chat-api — works in-cluster.

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd))"
cd "$REPO_ROOT"

CLUSTER_NAME=chat-rs
KIND_CONTEXT="kind-${CLUSTER_NAME}"
NAMESPACE=chat-rs

USER_SERVICE_LOCAL_PORT=18080
CHAT_API_LOCAL_PORT=18081
CHAT_WS_GATEWAY_LOCAL_PORT=18082

USER_SERVICE_URL="http://127.0.0.1:${USER_SERVICE_LOCAL_PORT}"
CHAT_API_URL="http://127.0.0.1:${CHAT_API_LOCAL_PORT}"
CHAT_WS_GATEWAY_URL="ws://127.0.0.1:${CHAT_WS_GATEWAY_LOCAL_PORT}"

# Unique per run so reruns never collide with a previous run's users/channel.
RUN_SUFFIX="$(date +%s)"
USER_A_USERNAME="miles-davis-${RUN_SUFFIX}"
USER_A_EMAIL="miles.davis+${RUN_SUFFIX}@example.com"
USER_A_PASSWORD="K1nd-0f-Blue_1959!"
USER_B_USERNAME="john-coltrane-${RUN_SUFFIX}"
USER_B_EMAIL="john.coltrane+${RUN_SUFFIX}@example.com"
USER_B_PASSWORD="A-L0ve-Supreme_1965!"
CHANNEL_NAME="on-call-rotation-${RUN_SUFFIX}"
MESSAGE_CONTENT="Has the incident been resolved? (${RUN_SUFFIX})"

WORKDIR="$(mktemp -d)"
declare -a BACKGROUND_PIDS=()

# Everything backgrounded (port-forwards, the WS listener) and the scratch
# dir are torn down here regardless of how the script exits.
cleanup() {
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
# `|| true` on the curl substitution: a connection-level failure (refused,
# reset) must reach our own fail() message instead of a bare `set -e` abort.
HTTP_STATUS=""
http_post() {
  local url="$1" body="$2" outfile="$3" token="${4:-}"
  local -a auth_header=()
  [[ -n "$token" ]] && auth_header=(-H "Authorization: Bearer ${token}")
  HTTP_STATUS="$(curl -sS -o "$outfile" -w '%{http_code}' -X POST "$url" \
    -H 'Content-Type: application/json' \
    "${auth_header[@]}" \
    -d "$body")" || true
  [[ -n "$HTTP_STATUS" ]] || fail "POST ${url} (connection failed)" "$outfile"
}

http_get() {
  local url="$1" outfile="$2" token="${3:-}"
  local -a auth_header=()
  [[ -n "$token" ]] && auth_header=(-H "Authorization: Bearer ${token}")
  HTTP_STATUS="$(curl -sS -o "$outfile" -w '%{http_code}' "${auth_header[@]}" "$url")" || true
  [[ -n "$HTTP_STATUS" ]] || fail "GET ${url} (connection failed)" "$outfile"
}

# Stage 1: tunnel the three ClusterIP services this test talks to, and don't
# proceed until each one is actually accepting connections. `/livez` is a
# bare liveness check (always 200 once the process is up) — exactly what
# proves the tunnel itself works, as opposed to `/readyz` which depends on
# Postgres/Kafka/Scylla and would conflate two different failure modes here.
start_port_forwards() {
  kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" port-forward \
    svc/user-service "${USER_SERVICE_LOCAL_PORT}:3001" \
    >"$WORKDIR/pf-user-service.log" 2>&1 &
  BACKGROUND_PIDS+=($!)

  kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" port-forward \
    svc/chat-api "${CHAT_API_LOCAL_PORT}:3002" \
    >"$WORKDIR/pf-chat-api.log" 2>&1 &
  BACKGROUND_PIDS+=($!)

  kubectl --context "$KIND_CONTEXT" -n "$NAMESPACE" port-forward \
    svc/chat-ws-gateway "${CHAT_WS_GATEWAY_LOCAL_PORT}:3002" \
    >"$WORKDIR/pf-chat-ws-gateway.log" 2>&1 &
  BACKGROUND_PIDS+=($!)

  wait_for_livez "$USER_SERVICE_URL" "user-service" "$WORKDIR/pf-user-service.log"
  wait_for_livez "$CHAT_API_URL" "chat-api" "$WORKDIR/pf-chat-api.log"
  wait_for_livez "$CHAT_WS_GATEWAY_URL" "chat-ws-gateway" "$WORKDIR/pf-chat-ws-gateway.log"
}

wait_for_livez() {
  local base_url="$1" name="$2" log_file="$3"
  local http_url="${base_url/ws:/http:}"
  local attempt
  for attempt in $(seq 1 30); do
    if curl -fsS -o /dev/null "${http_url}/livez" 2>/dev/null; then
      echo "port-forward ready: ${name} (${attempt}/30)"
      return 0
    fi
    sleep 1
  done
  fail "port-forward never became reachable: ${name}" "$log_file"
}

# Stage 2: register both users (tolerating 409 on a same-second rerun) and
# log in — /api/users never returns a token, only /api/auth/login does.
register_user() {
  local username="$1" email="$2" password="$3"
  local body outfile
  body="$(jq -n --arg u "$username" --arg e "$email" --arg p "$password" \
    '{username: $u, email: $e, password: $p}')"
  outfile="$WORKDIR/register-${username}.json"
  http_post "${USER_SERVICE_URL}/api/users" "$body" "$outfile"
  case "$HTTP_STATUS" in
    201) ;;
    409) echo "user ${username} already exists, continuing" ;;
    *) fail "register user ${username} (HTTP ${HTTP_STATUS})" "$outfile" ;;
  esac
}

# Sets $LOGIN_TOKEN/$LOGIN_USER_ID rather than echoing them: called directly
# (never as `x=$(login_user ...)`), so its internal fail() — on a subshell,
# `exit` would only kill the subshell and leave the caller running blind —
# actually terminates the script.
LOGIN_TOKEN=""
LOGIN_USER_ID=""
login_user() {
  local username="$1" password="$2"
  local body outfile
  body="$(jq -n --arg u "$username" --arg p "$password" '{username: $u, password: $p}')"
  outfile="$WORKDIR/login-${username}.json"
  http_post "${USER_SERVICE_URL}/api/auth/login" "$body" "$outfile"
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

  echo "registered + logged in: A=${USER_A_ID} (${USER_A_USERNAME}), B=${USER_B_ID} (${USER_B_USERNAME})"
}

# Stage 3: user A creates a private channel with B as a member. Channel
# creation itself has no hard dependency on B's user-events replica row
# (channel_members has no FK to the replica table, and message-send's
# UserResolver falls back to user-service over gRPC when the replica is
# stale) — so this can't fail on replica lag by construction. The retry
# here is a safety net for the cluster still warming up, not a replica gate;
# the real proof that the replica pipeline works is the message flow below.
create_channel_with_member_b() {
  local body outfile attempt
  body="$(jq -n --arg n "$CHANNEL_NAME" --arg m "$USER_B_ID" \
    '{channel_type: "private", name: $n, members: [$m]}')"
  outfile="$WORKDIR/create-channel.json"

  for attempt in $(seq 1 10); do
    http_post "${CHAT_API_URL}/api/channels" "$body" "$outfile" "$USER_A_TOKEN"
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

# Stage 4: B connects first and listens. The per-channel URL is the only
# "subscribe" step this protocol has — there is no separate subscribe frame
# (ClientMessage only has SendMessage/Ping). Auth rides in
# Sec-WebSocket-Protocol, not the query string, per websocket_auth_tests.rs.
start_b_listener() {
  B_FRAMES_FILE="$WORKDIR/b-frames.jsonl"
  websocat "${CHAT_WS_GATEWAY_URL}/ws/channels/${CHANNEL_ID}" \
    --protocol "bearer, ${USER_B_TOKEN}" \
    </dev/null >"$B_FRAMES_FILE" 2>"$WORKDIR/b-ws.err" &
  BACKGROUND_PIDS+=($!)

  local attempt
  for attempt in $(seq 1 15); do
    grep -q '"type":"connected"' "$B_FRAMES_FILE" 2>/dev/null && return 0
    sleep 1
  done
  fail "B's WebSocket never reached Connected state" "$WORKDIR/b-ws.err"
}

# Stage 5: A sends one SendMessage frame and disconnects (`-u`: don't wait
# on a reply). Frame shape is ClientMessage::SendMessage from
# inbound/websocket/messages.rs: {"type":"send_message","content":"..."}.
send_message_from_a() {
  local frame
  frame="$(jq -n --arg c "$MESSAGE_CONTENT" '{type: "send_message", content: $c}')"
  printf '%s\n' "$frame" | websocat "${CHAT_WS_GATEWAY_URL}/ws/channels/${CHANNEL_ID}" \
    --protocol "bearer, ${USER_A_TOKEN}" -1 -u -n \
    >"$WORKDIR/a-ws-send.log" 2>&1 \
    || fail "A failed to send message over WebSocket" "$WORKDIR/a-ws-send.log"
}

# Stage 6: B's ServerMessage::NewMessage frame carries the content verbatim
# — this is gateway -> connection registry fan-out, live in-process.
assert_b_received_message() {
  local attempt
  for attempt in $(seq 1 30); do
    grep -qF "$MESSAGE_CONTENT" "$B_FRAMES_FILE" 2>/dev/null && return 0
    sleep 1
  done
  fail "B never received the message over WebSocket within 30s" "$B_FRAMES_FILE"
}

# Stage 7: chat-api's history endpoint reads from Scylla, populated only via
# gateway -> Kafka -> chat-worker's persister — this is the pipeline proof.
assert_history_contains_message() {
  local outfile="$WORKDIR/history.json"
  local attempt
  for attempt in $(seq 1 30); do
    http_get "${CHAT_API_URL}/api/channels/${CHANNEL_ID}/messages" "$outfile" "$USER_A_TOKEN"
    if [[ "$HTTP_STATUS" == "200" ]] \
      && jq -e --arg c "$MESSAGE_CONTENT" 'any(.[]; .content == $c)' "$outfile" >/dev/null 2>&1; then
      return 0
    fi
    sleep 2
  done
  fail "message never landed in channel history (Kafka -> persister -> Scylla)" "$outfile"
}

main() {
  start_port_forwards
  register_and_login_users
  create_channel_with_member_b
  start_b_listener
  send_message_from_a
  assert_b_received_message
  assert_history_contains_message

  echo
  echo "PASS: full message flow verified in-cluster"
  echo "  user A: ${USER_A_ID} (${USER_A_USERNAME})"
  echo "  user B: ${USER_B_ID} (${USER_B_USERNAME})"
  echo "  channel: ${CHANNEL_ID} (${CHANNEL_NAME})"
  echo "  message: ${MESSAGE_CONTENT}"
  echo "  delivered live over WebSocket and confirmed in channel history"
}

main "$@"
