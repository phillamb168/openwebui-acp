{{- define "openwebui-acp.name" -}}
{{- printf "%s-openwebui-acp" .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "openwebui-acp.image" -}}
{{- if .Values.image.digest -}}
{{ .Values.image.repository }}@{{ .Values.image.digest }}
{{- else -}}
{{ .Values.image.repository }}:{{ .Values.image.tag }}
{{- end -}}
{{- end -}}

{{- define "openwebui-acp.labels" -}}
app.kubernetes.io/name: openwebui-acp
app.kubernetes.io/instance: {{ .Release.Name | quote }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service | quote }}
{{- end -}}
