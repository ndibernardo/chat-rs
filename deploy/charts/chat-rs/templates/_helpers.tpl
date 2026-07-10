{{/*
Standard labels for a workload. Call as:
  include "chat-rs.labels" (dict "root" $ "workload" "user-service")
*/}}
{{- define "chat-rs.labels" -}}
{{ include "chat-rs.selectorLabels" . }}
app.kubernetes.io/part-of: chat-rs
app.kubernetes.io/managed-by: {{ .root.Release.Service }}
helm.sh/chart: {{ .root.Chart.Name }}-{{ .root.Chart.Version }}
{{- end -}}

{{/*
Selector labels — the stable subset used in matchLabels. Never add a label
here that can change across upgrades, or Deployment selectors break (the
field is immutable once created). Call as chat-rs.labels above.
*/}}
{{- define "chat-rs.selectorLabels" -}}
app.kubernetes.io/name: {{ .workload }}
app.kubernetes.io/instance: {{ .root.Release.Name }}
{{- end -}}

{{/*
Env shared by every workload in this chart, app or Job alike. Call with the
root context: include "chat-rs.commonEnv" .
*/}}
{{- define "chat-rs.commonEnv" -}}
- name: RUN_MODE
  value: kubernetes
- name: LOG_FORMAT
  value: json
# Feeds Kafka consumer group static membership so a rolling pod keeps its
# partition assignment across a restart instead of triggering a rebalance.
- name: POD_NAME
  valueFrom:
    fieldRef:
      fieldPath: metadata.name
{{- end -}}

{{/*
Env shared by the user-service Deployments and the migrate Job — kept in
one place so Job and Deployment env can't drift apart. Call with the root
context: include "chat-rs.userCoreEnv" .
*/}}
{{- define "chat-rs.userCoreEnv" -}}
- name: DATABASE__URL
  valueFrom:
    secretKeyRef:
      name: user-db-app
      key: uri
- name: KAFKA__BROKERS
  value: {{ .Values.kafka.brokers | quote }}
- name: CORS__ALLOWED_ORIGINS
  value: {{ .Values.cors.allowedOrigins | quote }}
{{- end -}}

{{/*
Env shared by the chat-api/chat-ws-gateway/chat-worker Deployments and the
chat-migrate Job — kept in one place so Job and Deployment env can't drift
apart. Call with the root context: include "chat-rs.chatCoreEnv" .
*/}}
{{- define "chat-rs.chatCoreEnv" -}}
- name: DATABASE__URL
  valueFrom:
    secretKeyRef:
      name: chat-db-app
      key: uri
- name: CASSANDRA__NODES
  value: {{ .Values.cassandra.nodes | quote }}
- name: CASSANDRA__DATACENTER
  value: {{ .Values.cassandra.datacenter | quote }}
# Env values are always strings on the wire — an unquoted number here
# renders as a YAML int and the API server rejects it against
# corev1.EnvVar.Value.
- name: CASSANDRA__REPLICATION_FACTOR
  value: {{ .Values.cassandra.replicationFactor | quote }}
- name: USER_SERVICE__GRPC_URL
  value: {{ .Values.userServiceGrpcUrl | quote }}
- name: KAFKA__BROKERS
  value: {{ .Values.kafka.brokers | quote }}
- name: CORS__ALLOWED_ORIGINS
  value: {{ .Values.cors.allowedOrigins | quote }}
{{- end -}}
