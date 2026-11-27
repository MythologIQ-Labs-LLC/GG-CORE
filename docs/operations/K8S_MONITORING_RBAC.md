# Kubernetes monitoring and RBAC: qualification boundary

**Source change only. No production Kubernetes support is certified.**

GG-CORE inference uses local authenticated IPC. The inference daemon does not
ship an enabled HTTP metrics listener or require Kubernetes API permissions.

This Helm chart now defaults optional Grafana dashboard and PrometheusRule
generation to disabled. It corrects uppercase helper references that did not
match the helper names defined in the chart. Those resources can be enabled
only when operators have independently qualified the metrics producer,
Prometheus Operator CRDs, data schema and appropriate monitoring authority.
Enabling them is not proof that any metrics exist or can be scraped.

False HTTP scrape annotations were removed from canary and blue-green
inference Pods. The runtime does not magically gain a port 9090 listener
because an annotation advertises one.

The chart now defaults rbac.create to false, avoiding creation of legacy
Role and RoleBinding resources that granted ConfigMap and custom-resource
read access to the inference ServiceAccount. An explicit opt-in to RBAC
requires permission review, ServiceAccount identity verification and
a separately justified component that needs Kubernetes API access.
The inference Pod must still disable token automount even when an
externally supplied ServiceAccount is selected (see PR #298).

Before a real deployment, run helm lint and helm template for default
values, both opt-in monitoring resources, opt-in RBAC, and each deployment
strategy; verify required CRDs, semantic validity and cluster admission.
Prove on an enforcing CNI that network ingress/egress are denied. Verify
that no inference Pod receives a projected API token, that local IPC probes
work and that a pinned licensed model actually serves. Keep issue #164
and v1.0.0 release HOLD until the complete installed evidence exists.
