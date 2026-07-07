#!/usr/bin/env bash
set -euo pipefail

# Validate Grafana dashboard JSON payloads used by provisioning.
shopt -s nullglob
files=(ops/observability/grafana/dashboards/*.json)

if (( ${#files[@]} == 0 )); then
  echo "No dashboard JSON files found." >&2
  exit 1
fi

for file in "${files[@]}"; do
  jq -e '.title and (.panels | type == "array") and ((.panels | length) > 0)' "$file" >/dev/null
  echo "validated: $file"
done
