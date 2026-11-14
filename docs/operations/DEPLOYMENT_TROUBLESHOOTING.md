# Deployment Troubleshooting Guide

**Document Version:** 1.0.0
**Last Updated:** 2026-02-18
**Applies To:** GG-CORE (historical deployment guide; validate commands against current release)

This guide provides diagnostic procedures and resolution steps for common deployment issues in the GG-CORE standalone runtime.

---

## Table of Contents

1. [Quick Diagnostics](#quick-diagnostics)
2. [Canary Deployment Issues](#canary-deployment-issues)
3. [Blue-Green Deployment Issues](#blue-green-deployment-issues)
4. [Common Error Codes](#common-error-codes)
5. [Debugging Tools](#debugging-tools)

---

## Quick Diagnostics

### Initial Assessment Commands

```bash
# Overall deployment status
GG-CORE deployment status

# Check all pod health
kubectl get pods -l app=GG-CORE -o wide

# Quick health check
GG-CORE health

# View recent events
kubectl get events --sort-by=.lastTimestamp | tail -20
```

### Health Status Matrix

| Check | Command | Healthy | Degraded | Critical |
|-------|---------|---------|----------|----------|
| Pods | kubectl get pods | All Running | Some NotReady | CrashLoopBackOff |
| Health | GG-CORE health | Exit 0 | - | Exit 1 |
| Metrics | curl :9090/metrics | 200 OK | Partial data | Connection refused |
| IPC | GG-CORE ready | Exit 0 | Timeout | Exit 1 |

---

## Canary Deployment Issues

### Issue: Canary Not Receiving Traffic

**Symptoms:**
- Canary pods running but no requests
- gg_core_requests_total{deployment="canary"} = 0
- Traffic weight set but not effective

**Diagnostic Steps:**

```bash
# 1. Check canary resource status
GG-CORE canary inspect

# 2. Verify traffic weight configuration
kubectl get veritascanary -o jsonpath="{.items[*].spec.trafficWeight}"

# 3. Check service endpoints
kubectl get endpoints GG-CORE-canary

# 4. Verify pod labels match service selector
kubectl get pods -l deployment=canary --show-labels

# 5. Check service mesh configuration (if using Istio)
kubectl get virtualservice GG-CORE -o yaml
```

**Resolution:**

| Cause | Resolution |
|-------|------------|
| Traffic weight = 0 | Set trafficWeight > 0 in VeritasCanary spec |
| Pod labels wrong | Verify pods have deployment=canary label |
| Service selector mismatch | Update service selector or pod labels |
| Service mesh misconfigured | Update VirtualService traffic split |
| NetworkPolicy blocking | Add rule allowing traffic to canary |

### Issue: Metrics Not Reporting

**Symptoms:**
- Canary analysis always fails
- Prometheus has no canary metrics
- Dashboard shows "No Data" for canary

**Diagnostic Steps:**

```bash
# 1. Check if metrics endpoint is working
kubectl exec -it <canary-pod> -- curl -s localhost:9090/metrics | head

# 2. Verify ServiceMonitor exists
kubectl get servicemonitor GG-CORE-canary

# 3. Check Prometheus targets
kubectl port-forward svc/prometheus 9090:9090
# Visit http://localhost:9090/targets and find canary

# 4. Verify metrics are being scraped
kubectl logs -l app=prometheus -c prometheus | grep canary

# 5. Check for label mismatches
kubectl get servicemonitor GG-CORE-canary -o yaml
```

**Resolution:**

| Cause | Resolution |
|-------|------------|
| Metrics endpoint down | Restart pod, check telemetry config |
| ServiceMonitor missing | Create ServiceMonitor for canary |
| Label selector wrong | Fix namespaceSelector or matchLabels |
| Port mismatch | Verify port in ServiceMonitor matches pod |
| RBAC issue | Grant Prometheus permission to scrape namespace |

### Issue: Unexpected Rollback

**Symptoms:**
- Canary automatically rolled back
- status.phase = "RollingBack" or "Failed"
- Analysis showing failures

**Diagnostic Steps:**

```bash
# 1. Check analysis history
GG-CORE canary inspect --history

# 2. View rollback reason
kubectl describe veritascanary <name> | grep -A5 "Conditions"

# 3. Check which metric failed
kubectl get veritascanary <name> -o jsonpath="{.status.conditions}"

# 4. Compare canary vs stable metrics
kubectl exec -it <stable-pod> -- GG-CORE metrics compare

# 5. View canary logs during failure window
kubectl logs -l deployment=canary --since=30m
```

**Resolution:**

| Cause | Resolution |
|-------|------------|
| Threshold too strict | Increase error-rate threshold |
| Analysis interval too short | Extend interval for stability |
| Canary has real bug | Fix the bug before redeploying |
| Traffic too low | Ensure minimum traffic for statistical significance |
| Baseline metrics wrong | Recalibrate thresholds from stable |

### Issue: Promotion Stuck

**Symptoms:**
- Canary passing analysis but not promoting
- status.phase = "Progressing" for extended time
- Traffic weight not increasing

**Diagnostic Steps:**

```bash
# 1. Check promotion configuration
kubectl get veritascanary <name> -o jsonpath="{.spec.promotion}"

# 2. Verify auto-promotion enabled
GG-CORE canary inspect | grep -i "auto.*promotion"

# 3. Check if paused
kubectl get veritascanary <name> -o jsonpath="{.status.phase}"

# 4. View controller logs
kubectl logs -l app=veritas-controller --tail=100

# 5. Check for manual intervention required
kubectl describe veritascanary <name> | grep -i "waiting"
```

**Resolution:**

| Cause | Resolution |
|-------|------------|
| Auto-promotion disabled | Enable spec.promotion.auto=true |
| Paused at step | Resume or wait for pause duration |
| Controller unhealthy | Restart veritas-controller |
| successLimit not met | Wait for more successful analyses |
| Manual approval required | Approve via kubectl annotate |

---

## Blue-Green Deployment Issues

### Issue: Switch Not Completing

**Symptoms:**
- Environment switch initiated but not finishing
- status.phase stuck in "Switching"
- Both environments receiving traffic

**Diagnostic Steps:**

```bash
# 1. Check environment status
GG-CORE bluegreen inspect

# 2. View switch progress
kubectl get veritasenvironment <name> -o jsonpath="{.status.switchProgress}"

# 3. Check target environment health
kubectl get pods -l environment=<target> -o wide

# 4. Verify service selector
kubectl get svc GG-CORE -o jsonpath="{.spec.selector}"

# 5. Check controller logs
kubectl logs -l app=veritas-controller | grep -i switch
```

**Resolution:**

| Cause | Resolution |
|-------|------------|
| Health check failing | Fix target env health issues |
| Timeout too short | Increase switch.healthCheck.timeout |
| minReadySeconds not met | Wait or reduce requirement |
| Service update failed | Manually update service selector |
| Controller crashed | Restart controller, check state |

### Issue: State Not Syncing

**Symptoms:**
- New environment has different config
- Model versions mismatched
- Cache state different between environments

**Diagnostic Steps:**

```bash
# 1. Compare configurations
kubectl get cm veritas-config-blue -o yaml > blue-config.yaml
kubectl get cm veritas-config-green -o yaml > green-config.yaml
diff blue-config.yaml green-config.yaml

# 2. Check model versions
kubectl exec -it <blue-pod> -- GG-CORE models list
kubectl exec -it <green-pod> -- GG-CORE models list

# 3. Verify environment labels
kubectl get pods --show-labels | grep veritas

# 4. Check for config drift
GG-CORE bluegreen diff
```

**Resolution:**

| Cause | Resolution |
|-------|------------|
| ConfigMap not synced | Apply same ConfigMap to both envs |
| Model mount different | Verify PVC mounts identical |
| Secrets mismatch | Sync secrets between namespaces |
| Environment variable drift | Update deployment manifests |

### Issue: Standby Environment Unhealthy

**Symptoms:**
- Inactive environment pods failing
- Cannot switch due to unhealthy target
- Preview service not responding

**Diagnostic Steps:**

```bash
# 1. Check standby pod status
kubectl get pods -l environment=<standby>

# 2. View pod events
kubectl describe pod -l environment=<standby>

# 3. Check resource availability
kubectl top nodes
kubectl describe nodes | grep -A5 "Allocated resources"

# 4. View pod logs
kubectl logs -l environment=<standby> --tail=50

# 5. Test health directly
kubectl exec -it <standby-pod> -- GG-CORE health --verbose
```

**Resolution:**

| Cause | Resolution |
|-------|------------|
| Resource exhaustion | Scale nodes or reduce standby replicas |
| Image pull failure | Verify registry access, check image tag |
| Config error | Fix configuration in standby env |
| PVC not bound | Check storage class and PVC status |
| Init container failing | Check init logs, fix prerequisites |

### Issue: DNS Not Updating

**Symptoms:**
- Traffic still going to old environment
- Service selector updated but DNS stale
- Clients receiving old pod IPs

**Diagnostic Steps:**

```bash
# 1. Check service endpoints
kubectl get endpoints GG-CORE

# 2. Verify DNS resolution
kubectl run -it --rm debug --image=busybox -- nslookup GG-CORE

# 3. Check CoreDNS
kubectl logs -l k8s-app=kube-dns -n kube-system --tail=20

# 4. View endpoint slices
kubectl get endpointslices -l kubernetes.io/service-name=GG-CORE

# 5. Test from inside cluster
kubectl exec -it <any-pod> -- curl GG-CORE:8080/health
```

**Resolution:**

| Cause | Resolution |
|-------|------------|
| DNS cache stale | Wait TTL or restart CoreDNS |
| Endpoints not updated | Verify pod readiness gates |
| Service selector wrong | Fix selector to match new pods |
| External DNS lag | Wait for propagation or flush |
| Client-side caching | Restart client applications |

---

## Common Error Codes

### Error Code Reference

| Code | Name | Description | Severity |
|------|------|-------------|----------|
| E1001 | IPC_CONNECTION_FAILED | Cannot connect to runtime socket | Critical |
| E1002 | IPC_TIMEOUT | Request timed out waiting for response | Warning |
| E1003 | IPC_PROTOCOL_ERROR | Malformed IPC message | Error |
| E2001 | MODEL_LOAD_FAILED | Failed to load model file | Critical |
| E2002 | MODEL_NOT_FOUND | Requested model not in registry | Error |
| E2003 | MODEL_VERSION_MISMATCH | Model version incompatible | Error |
| E3001 | SANDBOX_INIT_FAILED | Sandbox initialization failed | Critical |
| E3002 | SANDBOX_VIOLATION | Security boundary violation attempted | Critical |
| E3003 | RESOURCE_LIMIT_EXCEEDED | Memory or CPU limit exceeded | Error |
| E4001 | INFERENCE_FAILED | Inference execution error | Error |
| E4002 | TOKENIZER_ERROR | Tokenization failed | Error |
| E4003 | CONTEXT_OVERFLOW | Input exceeds context window | Warning |
| E5001 | DEPLOYMENT_FAILED | Deployment operation failed | Error |
| E5002 | ROLLBACK_FAILED | Rollback operation failed | Critical |
| E5003 | HEALTH_CHECK_FAILED | Health probe returned unhealthy | Warning |

### E1001: IPC_CONNECTION_FAILED

**Diagnostic Steps:**
```bash
# 1. Check socket exists
ls -la /var/run/veritas/GG-CORE.sock  # Unix
Get-ChildItem \.\pipe\ | Where-Object Name -match veritas  # Windows

# 2. Verify runtime is running
pgrep -a GG-CORE

# 3. Check socket permissions
stat /var/run/veritas/GG-CORE.sock

# 4. Test connection manually
GG-CORE health
```

**Resolution:**
1. Restart the CORE Runtime if not running
2. Check directory permissions on socket path
3. Verify no other process holding socket
4. Check SELinux/AppArmor policies

### E2001: MODEL_LOAD_FAILED

**Diagnostic Steps:**
```bash
# 1. Check model file exists
ls -la /models/<model-name>

# 2. Verify file integrity
sha256sum /models/<model-name>.gguf

# 3. Check available memory
free -h

# 4. View detailed error
GG-CORE models load <model-name> --verbose
```

**Resolution:**
1. Verify model file is not corrupted
2. Ensure sufficient memory available
3. Check model format compatibility
4. Verify read permissions on model directory

### E3001: SANDBOX_INIT_FAILED

**Diagnostic Steps:**
```bash
# 1. Check kernel capabilities
cat /proc/sys/kernel/unprivileged_userns_clone

# 2. Verify seccomp support
grep SECCOMP /boot/config-$(uname -r)

# 3. Check container runtime config
kubectl describe pod <pod> | grep -A10 "Security Context"

# 4. View sandbox logs
kubectl logs <pod> -c sandbox-init
```

**Resolution:**
1. Enable unprivileged user namespaces if needed
2. Verify seccomp profile is available
3. Check container security context
4. Review AppArmor/SELinux logs

### E5001: DEPLOYMENT_FAILED

**Diagnostic Steps:**
```bash
# 1. Check deployment status
GG-CORE deployment status --verbose

# 2. View recent events
kubectl get events --sort-by=.lastTimestamp | grep veritas

# 3. Check resource availability
kubectl describe nodes | grep -A5 "Allocated"

# 4. View controller logs
kubectl logs -l app=veritas-controller --tail=50
```

**Resolution:**
1. Check cluster resource availability
2. Verify image pull secrets
3. Review pod security policies
4. Check for conflicting deployments

---

## Debugging Tools

### GG-CORE CLI Commands

```bash
# Deployment Status
GG-CORE deployment status              # Overview of deployment state
GG-CORE deployment status --verbose     # Detailed status with metrics
GG-CORE deployment status --json        # JSON output for scripting

# Canary Operations
GG-CORE canary inspect                  # Current canary state
GG-CORE canary inspect --history        # Analysis history
GG-CORE canary inspect --metrics        # Current metric values

# Blue-Green Operations
GG-CORE bluegreen inspect               # Current environment state
GG-CORE bluegreen diff                  # Config diff between envs
GG-CORE bluegreen switch --dry-run      # Preview switch operation

# Rollback Operations
GG-CORE rollback --canary               # Rollback canary deployment
GG-CORE rollback --bluegreen            # Switch to previous env
GG-CORE rollback --force                # Force rollback (override checks)
GG-CORE rollback --status               # View rollback progress

# Health and Diagnostics
GG-CORE health                          # Full health check
GG-CORE health --verbose                # Detailed health report
GG-CORE live                            # Liveness probe
GG-CORE ready                           # Readiness probe

# Metrics and Telemetry
GG-CORE metrics summary                 # Key metrics summary
GG-CORE metrics canary                  # Canary-specific metrics
GG-CORE telemetry status                # Telemetry system status
```

### kubectl Debugging Commands

```bash
# Pod Investigation
kubectl get pods -l app=GG-CORE -o wide
kubectl describe pod <pod-name>
kubectl logs <pod-name> --tail=100
kubectl logs <pod-name> --previous          # Previous container logs

# Exec into Pod
kubectl exec -it <pod-name> -- /bin/sh
kubectl exec -it <pod-name> -- GG-CORE health --verbose

# Resource Status
kubectl top pods -l app=GG-CORE
kubectl get events --sort-by=.lastTimestamp

# CRD Status
kubectl get veritascanary -o wide
kubectl get veritasenvironment -o wide
kubectl describe veritascanary <name>
```

### Prometheus Queries

```promql
# Error rate
sum(rate(gg_core_requests_total{status="error"}[5m])) /
sum(rate(gg_core_requests_total[5m]))

# P99 latency
histogram_quantile(0.99, sum(rate(gg_core_request_duration_bucket[5m])) by (le))

# Canary error rate comparison
sum(rate(gg_core_requests_total{status="error",deployment="canary"}[5m])) /
sum(rate(gg_core_requests_total{deployment="canary"}[5m]))

# Memory usage
gg_core_memory_used_bytes / gg_core_memory_limit_bytes

# Pod readiness
sum(kube_pod_status_ready{namespace="veritas",condition="true"}) /
sum(kube_pod_status_ready{namespace="veritas"})
```

---

## Quick Reference Card

### Symptom to Action Map

| Symptom | First Command | Likely Issue |
|---------|---------------|--------------|
| All pods down | kubectl get pods | Cluster issue or config error |
| Canary failing | GG-CORE canary inspect | Threshold or code bug |
| Switch stuck | GG-CORE bluegreen inspect | Health check failing |
| High latency | GG-CORE metrics summary | Resource contention |
| No metrics | curl :9090/metrics | Telemetry config |
| Rollback failed | GG-CORE rollback --status | Image or state issue |

### Emergency Contacts

| Situation | Contact | Method |
|-----------|---------|--------|
| SEV1 Incident | On-call | PagerDuty |
| Cluster Issue | Platform Team | Slack #platform-oncall |
| Security Issue | Security Team | security@hearthlink.ai |
