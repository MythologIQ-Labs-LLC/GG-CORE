# Kubernetes operator preflight (qualification pending)

**Status: NOT an installed-qualification-approved deployment procedure.**
This page supersedes a legacy quickstart containing unverified Helm registries,
fictional HTTP inference endpoints, and an unsupported production-ready claim.
GG-CORE serves inference through local IPC. This chart does not expose an
HTTP completions or metrics server.

For local inference without Kubernetes, use the
[standalone guide](../USAGE_GUIDE.md). Installed Kubernetes acceptance is
tracked in [issue #164](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/164).

## What the source chart actually does

- The Helm chart lives at [k8s/helm/gg-core](../../k8s/helm/gg-core/).
  These instructions do not identify any verified hosted chart repository
  or published OCI runtime image.
- The Deployment invokes /app/gg-core serve and configures IPC-based
  live and ready exec probes, not HTTP endpoints.
- Models are mounted read-only from an operator-provided PVC. Model files,
  manifests, tokenizer assets, custody and license evidence must be staged
  independently. The inference daemon cannot fetch model weights.
- A secure socket needs a private, owner-controlled directory. The opt-in
  socketVolumeInit.enabled helper requires a matching image containing
  prepare-k8s-socket-dir and cluster approval for the root init container
  limited to CAP_CHOWN.
- CORE_AUTH_TOKEN is sourced from an existing namespaced Kubernetes Secret
  through auth.existingSecretName and auth.secretKey. No token value belongs
  in Helm values or a ConfigMap. Without a usable token the secure daemon
  refuses startup, rather than allowing anonymous inference.

The chart metadata and default image tag are historical, not proof that
a signed or deployable binary exists. Optional canary, blue-green and
Prometheus manifests do not themselves prove real HTTP metrics or traffic
promotion works.

## Local rendering preflight, without deploying

Prerequisites: Helm 3, a repository checkout, a verified OCI image and
independently staged, licensed model artifacts. The operator must create
the auth Secret in the intended namespace using an approved secrets manager.
The Secret must contain a nonempty key called token. Never paste its value
into command arguments, a YAML file, or source control.

~~~bash
test -n "$GG_CORE_IMAGE_REPOSITORY" || { echo "Verified image repo required"; exit 1; }
test -n "$GG_CORE_IMAGE_TAG" || { echo "Pinned image tag required"; exit 1; }
helm lint ./k8s/helm/gg-core
helm template gg-core ./k8s/helm/gg-core \
  --namespace gg-core \
  --set-string image.repository="$GG_CORE_IMAGE_REPOSITORY" \
  --set-string image.tag="$GG_CORE_IMAGE_TAG" \
  --set-string auth.existingSecretName="gg-core-ipc-auth" \
  --set-string auth.secretKey="token" \
  --set socketVolumeInit.enabled=true \
  > gg-core-rendered.yaml
~~~

This renders source only. It cannot verify that the Secret, image, binary,
socket helper, model PVC, OS dependencies, or Pod permissions actually work.
The chart's default networkPolicy.enabled=true and denyAll=true now render a
NetworkPolicy selecting all release pods, denying both ingress and egress.
Setting either switch false renders no NetworkPolicy; this is not isolation.
Pods explicitly disable Kubernetes ServiceAccount token automount even if
an operator supplies an external ServiceAccount. Only an enforcing CNI and
real negative ingress/egress tests can prove actual network containment.
Inspect the generated manifests for unwanted networked monitoring
or incompatible deployment-strategy resources. Do not apply them merely
because Helm lint or template succeeds.

## After independently qualified installation

Once a pinned image has been approved and its actual Deployment name is known:

~~~bash
kubectl -n gg-core exec deployment/REPLACE_WITH_DEPLOYMENT -c gg-core -- /app/gg-core live
kubectl -n gg-core exec deployment/REPLACE_WITH_DEPLOYMENT -c gg-core -- /app/gg-core ready
kubectl -n gg-core logs deployment/REPLACE_WITH_DEPLOYMENT -c gg-core
~~~

Liveness does not establish model readiness. Readiness alone does not
establish exact backend/model compatibility, real inference, or security.
Use authenticated local IPC and pinned model/binary receipts as described
in the [IPC protocol](../IPC_PROTOCOL_SCHEMA.md). Do not port-forward an
unimplemented HTTP endpoint.

The release gate remains **HOLD** until an exact image, model and source
tuple passes independent Kubernetes startup, Secret-negative, network-deny,
socket security, Pod admission, restart, real-model IPC and containment
qualification. Rendering and hosted Rust CI cannot substitute for this.
