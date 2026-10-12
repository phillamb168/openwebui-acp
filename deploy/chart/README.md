# Kubernetes agent chart

This chart installs one always-on Buzz agent beside an existing relay, using the [VM deployment's container](../sidecar/README.md). The relay, Open WebUI, databases, providers and tools remain separate services. There is no agent Service, Ingress, listening port, Kubernetes API access, or task-environment provisioner. A [release download](../../docs/RELEASE.md#choose-a-download) can supply the chart and container; replace `deploy/chart` in the Helm commands with the downloaded chart's path and configure the loaded/published image tag.

## Prerequisites

- A working cluster, Helm, and a ReadWriteOnce PVC supporting POSIX ownership, file locks, atomic rename and fsync. Its driver must support `fsGroup` or provision a volume writable by UID/GID 10001. The chart declares Kubernetes 1.30+; executed coverage is Kind/Kubernetes 1.37.0 with local-path storage. Other storage drivers and managed clusters need operator validation.
- The [Open WebUI prerequisites](../../docs/SETUP.md#open-webui-prerequisites), plus a dedicated Buzz bot admitted to the relay and joined to its channels. See [identity setup](../sidecar/README.md#prerequisites).
- A registry reachable by nodes, or an image loaded on every eligible node. Build `deploy/sidecar/Dockerfile` for the node architecture, tag it for your registry, and publish under your own approval process. No prepublished project image is assumed. In disposable Kind, use `kind load docker-image openwebui-acp-sidecar:0.1.0-rc.2 --name CLUSTER`.
- Trusted HTTPS/WSS endpoints, DNS and outbound connectivity. Use the relay's canonical hostname. Private networking/VPN integration is operator-managed.

## Install using an existing Secret

Choose an explicit context and namespace. Replace `CONTEXT`; do not assume the current context is safe. Create private key files, then create the Secret from files so credentials do not enter Helm values, shell arguments or Git:

```sh
kubectl --context CONTEXT create namespace workspace-agent
kubectl --context CONTEXT -n workspace-agent create secret generic openwebui-acp-credentials \
  --from-file=openwebui-key=/private/path/openwebui-key \
  --from-file=buzz-key=/private/path/buzz-key
cp deploy/chart/example-values.yaml /private/path/agent-values.yaml
```

Edit the values: image repository/tag (prefer `image.digest` for an immutable deployment), Open WebUI URL/model, Buzz WSS URL, requester public keys and channel UUIDs. Sample identifiers are placeholders. Set `credentials.existingSecret` to the Secret name. `openwebui.requiredTools` may require existing model-default tool IDs; it grants no capabilities. Set `persistence.storageClass` if the cluster default is unsuitable, or `persistence.existingClaim` for an operator-owned claim.

```sh
helm lint deploy/chart -f /private/path/agent-values.yaml
helm template client-agent deploy/chart -f /private/path/agent-values.yaml
helm upgrade --install client-agent deploy/chart --kube-context CONTEXT \
  --namespace workspace-agent -f /private/path/agent-values.yaml --wait --timeout 5m
kubectl --context CONTEXT -n workspace-agent logs deployment/client-agent-openwebui-acp -c agent --tail=50
```

Review rendered manifests before installation. Validate with an allowed human mention in a new thread, a follow-up, a separate thread, and an idle Pod replacement; confirm chat identity and reply destination. Ready is not an end-to-end probe. Production test prompts/tools require the operator's approval.

## Security and lifecycle

Exactly one replica uses `Recreate`, allowing the exclusive journal lock to move between Pods. The chart disables service-account token mounting, runs as UID/GID 10001 with runtime-default seccomp, drops all capabilities, disallows privilege escalation and uses read-only container roots. Do not manually scale it or mount the journal in another active agent.

Kubernetes Secret projections are symlinks. A non-root init container validates that their targets stay inside the read-only mount, then copies bounded contents into regular 0600 files in a memory-backed volume. The agent sees only that private copy. A 0700 `state/` subdirectory on the PVC preserves the journal. Runtime file checks remain strict; keys are not stored on the PVC. Cluster administrators and identities with Secret/Pod access remain trusted. Configure RBAC, Secret encryption, backup access and egress policy for your cluster.

After changing the Secret, run `kubectl --context CONTEXT -n workspace-agent rollout restart deployment/client-agent-openwebui-acp`: the private copy updates at Pod startup. Retain the claim and account/bot identity for continuity. Changing accounts or identities requires separate state.

Before upgrading, wait until idle, record old values/image digest, and take an offline PVC backup. Use `helm upgrade --install` with the new chart/image, then check fresh requests and original-thread continuity. Helm rollback does not undo remote tool effects or restore state. Reconcile uncertain operations before journal rollback; see [RECOVERY.md](../../docs/RECOVERY.md).

`helm uninstall client-agent --kube-context CONTEXT -n workspace-agent` removes the Deployment but retains a chart-created PVC via `helm.sh/resource-policy: keep`. Record its name before uninstalling. Reinstall with `persistence.existingClaim: client-agent-openwebui-acp` (or the recorded name). The Secret is also operator-owned. Namespace deletion or explicit PVC deletion can destroy state despite the Helm annotation; apply storage reclaim and retention policies deliberately.

## Installing a Buzz relay separately

Buzz supplies `oci://ghcr.io/block/buzz/charts/buzz`. The executed fixture pairs chart **0.1.11** with relay **0.2.1**, overriding the chart's older default image:

```yaml
image:
  tag: "0.2.1"
  digest: sha256:4e31b7c7abb7d00b6f513dc559e58d2b980416f1dc400aa01bcf762cf2989cfc
```

Inspect the upstream chart's README/values with `helm show readme` and `helm show values oci://ghcr.io/block/buzz/charts/buzz --version 0.1.11`. Configure its canonical URL, owner, PostgreSQL, Redis, S3 storage, TLS/Ingress and membership. Verify the chart digest; the tested OCI digest is `sha256:3fe7050b97e33fc98b258cf3b66f2a48fc9c9f27065d4a6fe9817631f2d5292e`. The fixture's quickstart database/storage mode is disposable infrastructure, not a production operations recommendation. This agent chart neither depends on nor upgrades the relay chart.

The [Kind suite](../../docs/TESTING.md#kubernetes) installs both charts and tests tools, isolation, key/state permissions, denied authors/channels, queued requests, interrupted-turn recovery, Pod replacement, Helm upgrade and retained-PVC reinstall. This is one-cluster evidence, not multi-tenant, high-availability or arbitrary-CSI certification. Client knowledge/oracle services and ephemeral development stacks remain separate projects.
