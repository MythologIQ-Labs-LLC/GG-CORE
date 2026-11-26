#!/usr/bin/env bash
# Helm-rendered source acceptance for GG-CORE's IPC-only Kubernetes chart.
# NOT an installed CNI, OCI, Secret, or real-model qualification (#164).
set -euo pipefail
command -v helm >/dev/null || { echo "helm CLI required" >&2; exit 2; }
chart=k8s/helm/gg-core
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

render() {
    local name="$1"
    shift
    helm template gg-core "$chart" --namespace gg-core "$@" > "$tmp/$name.yaml"
}

assert_count() {
    local file="$1" pattern="$2" expected="$3" actual
    actual=$(grep -Ec "$pattern" "$tmp/$file.yaml" || true)
    if [[ "$actual" -ne "$expected" ]]; then
        echo "$file: pattern [$pattern] expected $expected, found $actual" >&2
        exit 1
    fi
}

assert_contains() {
    local file="$1" needle="$2"
    grep -Fq "$needle" "$tmp/$file.yaml" || {
        echo "$file: missing [$needle]" >&2
        exit 1
    }
}

helm lint "$chart"
render default
assert_count default '^kind: NetworkPolicy$' 1
assert_count default '^kind: Role$' 0
assert_count default '^kind: RoleBinding$' 0
assert_count default '^kind: PrometheusRule$' 0
assert_count default 'grafana-dashboard' 0
assert_count default '^      automountServiceAccountToken: false$' 1
assert_contains default 'ingress: []'
assert_contains default 'egress: []'
assert_contains default 'policyTypes:'

render no_policy --set networkPolicy.enabled=false
assert_count no_policy '^kind: NetworkPolicy$' 0
render no_deny --set networkPolicy.denyAll=false
assert_count no_deny '^kind: NetworkPolicy$' 0

render secret --set-string auth.existingSecretName=gg-core-ipc-auth
assert_count secret 'secretKeyRef:' 1
assert_contains secret 'name: "gg-core-ipc-auth"'
assert_contains secret 'optional: false'
if helm template gg-core "$chart" --namespace gg-core \
    --set-string auth.existingSecretName=gg-core-ipc-auth \
    --set-string auth.secretKey='' > "$tmp/invalid.yaml" 2>/dev/null; then
    echo "missing auth Secret key must fail Helm rendering" >&2
    exit 1
fi

render canary --set canary.enabled=true \
    --set-string canary.canary.version=source-test
assert_count canary '^kind: NetworkPolicy$' 1
assert_count canary '^      automountServiceAccountToken: false$' 3
render bluegreen --set bluegreen.enabled=true \
    --set-string bluegreen.blue.version=source-test-blue \
    --set-string bluegreen.green.version=source-test-green
assert_count bluegreen '^kind: NetworkPolicy$' 1
assert_count bluegreen '^      automountServiceAccountToken: false$' 3

render monitoring --set monitoring.grafana.dashboard.enabled=true \
    --set monitoring.prometheus.rules.enabled=true
assert_count monitoring '^kind: PrometheusRule$' 1
assert_contains monitoring 'grafana-dashboard'
render rbac --set rbac.create=true
assert_count rbac '^kind: Role$' 1
assert_count rbac '^kind: RoleBinding$' 1
echo "PASS: Helm source rendering matrix (NOT installed qualification)"
