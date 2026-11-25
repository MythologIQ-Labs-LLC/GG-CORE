{{/*
Expand the name of the chart.
*/}}
{{- define "gg-core.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "gg-core.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "gg-core.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "gg-core.labels" -}}
helm.sh/chart: {{ include "gg-core.chart" . }}
{{ include "gg-core.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "gg-core.selectorLabels" -}}
app.kubernetes.io/name: {{ include "gg-core.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "gg-core.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "gg-core.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/* Bounded-capability root init: make the emptyDir owner UID match the daemon. */}}
{{- define "gg-core.socketInitContainer" -}}
- name: prepare-socket-dir
  image: "{{ .root.Values.image.repository }}:{{ .tag }}"
  imagePullPolicy: {{ .root.Values.image.pullPolicy }}
  command:
    - /app/gg-core
  args:
    - prepare-k8s-socket-dir
    - --uid
    - {{ required "securityContext.runAsUser must be a non-root UID" .root.Values.securityContext.runAsUser | quote }}
  env:
    - name: GG_CORE_SOCKET_PATH
      value: {{ .root.Values.config.socketPath | quote }}
  securityContext:
    runAsUser: 0
    runAsNonRoot: false
    readOnlyRootFilesystem: true
    allowPrivilegeEscalation: false
    capabilities:
      drop: ["ALL"]
      add: ["CHOWN"]
  volumeMounts:
    - name: socket
      mountPath: /var/run/gg-core
{{- end -}}
