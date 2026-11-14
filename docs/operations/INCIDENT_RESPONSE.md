# Incident Response Procedures

**Document Version:** 1.0.0
**Last Updated:** 2026-02-18
**Applies To:** GG-CORE (procedural template; historical 2026-02 baseline)

This document defines incident response procedures for the GG-CORE standalone runtime, including severity classification, response workflows, and deployment-specific incident handling.

---

## Table of Contents

1. [Severity Levels](#severity-levels)
2. [Response Procedures](#response-procedures)
3. [Escalation Matrix](#escalation-matrix)
4. [Deployment-Specific Incidents](#deployment-specific-incidents)
5. [Communication Templates](#communication-templates)
6. [On-Call Runbook](#on-call-runbook)

---

## Severity Levels

### SEV1: Complete Service Outage

**Definition:** Complete loss of inference capability affecting all users.

| Attribute | Value |
|-----------|-------|
| Impact | 100% of users unable to perform inference |
| Response Time | Immediate (within 15 minutes) |
| Resolution Target | 1 hour |
| Escalation | Automatic to engineering leadership |
| Communication | Every 30 minutes |

**Examples:**
- All CORE Runtime pods in CrashLoopBackOff
- IPC communication completely broken
- Model loading fails for all models
- Sandbox initialization failure across cluster

**Indicators:**
```
gg_core_runtime_state \!= 1 (all instances)
gg_core_requests_total{status="error"} rate > 95%
gg_core_health_ready == 0 (all instances)
```

### SEV2: Partial Outage or Severe Degradation

**Definition:** Significant functionality loss or severe performance degradation affecting a substantial portion of users.

| Attribute | Value |
|-----------|-------|
| Impact | 25-99% of users affected |
| Response Time | Within 30 minutes |
| Resolution Target | 4 hours |
| Escalation | Engineering team lead after 1 hour |
| Communication | Every 1 hour |

**Examples:**
- Canary deployment failing with high error rate
- Blue-green switch stuck in Verifying state
- Specific model fails to load while others work
- P99 latency exceeds 10x baseline

**Indicators:**
```
gg_core_canary_error_rate > 0.05
gg_core_p99_latency_seconds > baseline * 10
gg_core_model_load_failures_total increasing rapidly
```

### SEV3: Minor Degradation

**Definition:** Noticeable but limited impact on functionality or performance.

| Attribute | Value |
|-----------|-------|
| Impact | 1-25% of users affected or minor performance impact |
| Response Time | Within 2 hours |
| Resolution Target | 24 hours |
| Escalation | Team lead after 8 hours |
| Communication | Daily updates |

**Examples:**
- Single canary replica unhealthy
- Metrics collection intermittent
- Slightly elevated error rate (< 1%)
- Memory usage trending high but not critical

**Indicators:**
```
gg_core_replica_count < desired (but service operational)
gg_core_memory_used_ratio > 0.8
gg_core_error_rate > 0.001 AND < 0.01
```

### SEV4: Informational

**Definition:** Issues requiring attention but not impacting users.

| Attribute | Value |
|-----------|-------|
| Impact | No user impact |
| Response Time | Next business day |
| Resolution Target | 1 week |
| Escalation | None required |
| Communication | Weekly summary |

**Examples:**
- Deprecated API usage warnings
- Non-critical log errors
- Development environment issues
- Documentation gaps discovered

---

## Response Procedures

### Phase 1: Detection

**Automatic Detection:**
- Prometheus alerting via AlertManager
- Kubernetes health check failures (liveness/readiness probes)
- Canary analysis failure triggers
- Blue-green verification failures

**Manual Detection:**
- User reports via support channels
- On-call engineer monitoring
- Scheduled health checks

**Detection Checklist:**
```
[ ] Alert received and acknowledged
[ ] Initial symptom documented
[ ] Affected scope identified (namespace, cluster, region)
[ ] Timeline established (when did it start?)
[ ] Recent changes reviewed (last 24h deployments)
```

### Phase 2: Triage

**Severity Assessment:**
1. Determine user impact percentage
2. Classify by severity level (SEV1-4)
3. Assign incident commander (SEV1-2)
4. Create incident channel/ticket

**Triage Commands:**
```bash
# Check overall deployment status
GG-CORE deployment status

# Inspect canary state
GG-CORE canary inspect

# Inspect blue-green state
GG-CORE bluegreen inspect

# Get health report
GG-CORE health --verbose
```

**Triage Checklist:**
```
[ ] Severity level assigned
[ ] Incident commander designated (SEV1-2)
[ ] Communication channel established
[ ] Stakeholders notified
[ ] Initial status posted
```

### Phase 3: Mitigation

**Immediate Actions (Choose appropriate):**

| Action | Command | Use When |
|--------|---------|----------|
| Rollback canary | GG-CORE rollback --canary | Canary causing errors |
| Rollback blue-green | GG-CORE rollback --bluegreen | Active env unhealthy |
| Force rollback | GG-CORE rollback --force | Automated rollback stuck |
| Scale up stable | kubectl scale deploy veritas-stable --replicas=N | Need more capacity |
| Restart pods | kubectl rollout restart deploy/GG-CORE | Transient state issue |

**Mitigation Priority:**
1. Stop the bleeding (restore service)
2. Prevent expansion (isolate affected components)
3. Preserve evidence (do not delete logs/state)

**Mitigation Checklist:**
```
[ ] Mitigation action selected
[ ] Action executed
[ ] Service status verified
[ ] User impact confirmed reduced/resolved
[ ] Mitigation documented
```

### Phase 4: Resolution

**Root Cause Investigation:**
1. Collect all relevant logs and metrics
2. Identify exact failure point
3. Determine contributing factors
4. Implement permanent fix
5. Test fix in staging

**Resolution Checklist:**
```
[ ] Root cause identified
[ ] Fix developed and tested
[ ] Fix deployed to production
[ ] Service fully restored
[ ] Monitoring confirms resolution
```

### Phase 5: Post-Mortem

**Timeline:** Within 48 hours of resolution for SEV1-2.

**Post-Mortem Requirements:**
1. Create RCA document using template
2. Schedule blameless review meeting
3. Document action items with owners
4. Update runbooks and documentation
5. Implement prevention measures

---

## Escalation Matrix

### SEV1 Escalation Path

| Time | Action | Escalation Target |
|------|--------|-------------------|
| 0 min | Alert triggered | On-call engineer |
| 15 min | No response | Secondary on-call |
| 30 min | No resolution path | Engineering Manager |
| 1 hour | Not mitigated | Director of Engineering |
| 2 hours | Not resolved | VP Engineering |

### SEV2 Escalation Path

| Time | Action | Escalation Target |
|------|--------|-------------------|
| 0 min | Alert triggered | On-call engineer |
| 30 min | No response | Secondary on-call |
| 1 hour | No resolution path | Engineering Manager |
| 4 hours | Not resolved | Director of Engineering |

### Contact Information

| Role | Contact Method | Response SLA |
|------|---------------|--------------|
| On-call Primary | PagerDuty | 15 min |
| On-call Secondary | PagerDuty | 15 min |
| Engineering Manager | Slack + Phone | 30 min |
| Director | Phone | 1 hour |

---

## Deployment-Specific Incidents

### Canary Deployment Failure

**Symptoms:**
- VeritasCanary status.phase = "Failed"
- Canary pods not receiving traffic
- High error rate on canary
- Analysis runs failing

**Diagnostic Steps:**
```bash
# 1. Check canary status
GG-CORE canary inspect

# 2. View canary pod logs
kubectl logs -l deployment=canary -c GG-CORE --tail=100

# 3. Check analysis results
kubectl describe veritascanary <name>

# 4. View metrics
kubectl exec -it <stable-pod> -- GG-CORE metrics canary
```

**Resolution Options:**
| Scenario | Action |
|----------|--------|
| Canary crashing | GG-CORE rollback --canary |
| Metrics not collecting | Check Prometheus scraping config |
| Analysis failing | Adjust thresholds or extend duration |
| Traffic not splitting | Verify service mesh configuration |

### Blue-Green Switch Failure

**Symptoms:**
- VeritasEnvironment status.phase = "Failed" or stuck in "Switching"
- Inactive environment not becoming healthy
- Traffic not shifting to new environment

**Diagnostic Steps:**
```bash
# 1. Check environment status
GG-CORE bluegreen inspect

# 2. View both environment pods
kubectl get pods -l environment=blue
kubectl get pods -l environment=green

# 3. Check switch progress
kubectl describe veritasenvironment <name>

# 4. Verify health of target environment
kubectl exec -it <target-pod> -- GG-CORE health --verbose
```

**Resolution Options:**
| Scenario | Action |
|----------|--------|
| Target env unhealthy | Fix target or rollback |
| Switch timeout | GG-CORE bluegreen switch --force |
| Service routing stuck | Manually update service selector |
| State sync failed | Clear state and retry |

### Rollback Failure

**Symptoms:**
- Rollback command returns error
- Previous version not restoring
- Mixed version traffic

**Diagnostic Steps:**
```bash
# 1. Check rollback status
GG-CORE rollback --status

# 2. Verify previous image availability
kubectl describe deployment GG-CORE | grep Image

# 3. Check rollout history
kubectl rollout history deployment/GG-CORE
```

**Resolution Options:**
| Scenario | Action |
|----------|--------|
| Image pull failure | Verify registry access, retry |
| State corruption | Delete and recreate deployment |
| Stuck rollout | kubectl rollout undo or force restart |

### Metrics Collection Failure

**Symptoms:**
- Prometheus not scraping metrics
- AlertManager not triggering
- Dashboard showing no data

**Diagnostic Steps:**
```bash
# 1. Check metrics endpoint
kubectl exec -it <pod> -- curl localhost:9090/metrics

# 2. Verify ServiceMonitor
kubectl get servicemonitor GG-CORE -o yaml

# 3. Check Prometheus targets
kubectl port-forward svc/prometheus 9090:9090
# Then visit http://localhost:9090/targets

# 4. View telemetry status
GG-CORE telemetry status
```

**Resolution Options:**
| Scenario | Action |
|----------|--------|
| Metrics endpoint down | Restart pod or fix telemetry |
| Scrape config wrong | Update ServiceMonitor |
| Network policy blocking | Add egress rule for Prometheus |

---

## Communication Templates

### SEV1 Initial Notification

```
[SEV1 INCIDENT] GG-CORE standalone runtime - Complete Outage

Status: INVESTIGATING
Impact: All inference requests failing
Start Time: YYYY-MM-DD HH:MM UTC
Incident Commander: [Name]

Summary: [Brief description of symptoms]

Current Actions:
- [Action 1]
- [Action 2]

Next Update: [Time - within 30 minutes]

Incident Channel: #incident-YYYYMMDD-core
```

### SEV1 Update Template

```
[SEV1 UPDATE] GG-CORE standalone runtime

Status: MITIGATING / MONITORING / RESOLVED
Time: YYYY-MM-DD HH:MM UTC

Update:
[What has changed since last update]

Current Status:
- User Impact: [X%] of requests affected
- Root Cause: [Identified/Investigating]
- ETA to Resolution: [Time or Unknown]

Next Update: [Time]
```

### SEV2 Initial Notification

```
[SEV2 INCIDENT] GG-CORE standalone runtime - Partial Degradation

Status: INVESTIGATING
Impact: [Description of affected functionality]
Start Time: YYYY-MM-DD HH:MM UTC

Summary: [Brief description]

Monitoring: We are actively investigating and will provide updates hourly.

Next Update: [Time - within 1 hour]
```

### Resolution Notification

```
[RESOLVED] GG-CORE standalone runtime - [Incident Title]

Resolution Time: YYYY-MM-DD HH:MM UTC
Duration: [X hours Y minutes]

Root Cause: [Brief description]

Resolution: [What fixed it]

Impact Summary:
- Duration: [Time]
- Users Affected: [Estimate]
- Requests Failed: [Number if known]

Post-Mortem: Scheduled for [Date/Time] - [Link to RCA doc]
```

---

## On-Call Runbook

### Shift Start Checklist

```
[ ] Review open incidents and handoff notes
[ ] Verify pager is working (test page)
[ ] Check system status dashboards
[ ] Review recent deployments (last 24h)
[ ] Confirm access to all required systems
```

### Daily Monitoring

```
[ ] Check Prometheus alerts (no firing alerts)
[ ] Review error rate trends (< 0.1%)
[ ] Verify canary deployments healthy
[ ] Check resource utilization (< 80%)
[ ] Review slow query logs if applicable
```

### Shift End Handoff

```
[ ] Document any ongoing issues
[ ] Update runbooks if needed
[ ] Brief incoming on-call
[ ] Transfer active incidents
[ ] Update shift log
```

---

## Appendix: Quick Reference

### Critical Commands

```bash
# Health checks
GG-CORE health
GG-CORE live
GG-CORE ready

# Deployment status
GG-CORE deployment status
GG-CORE canary inspect
GG-CORE bluegreen inspect

# Rollback operations
GG-CORE rollback --canary
GG-CORE rollback --bluegreen
GG-CORE rollback --force

# Diagnostics
GG-CORE telemetry status
GG-CORE metrics summary
```

### Key Metrics

| Metric | Normal | Warning | Critical |
|--------|--------|---------|----------|
| Error Rate | < 0.1% | 0.1-1% | > 1% |
| P99 Latency | < 100ms | 100-500ms | > 500ms |
| Memory Usage | < 70% | 70-85% | > 85% |
| Pod Ready | 100% | 80-100% | < 80% |

### Alert Routing

| Alert | Severity | Notification |
|-------|----------|--------------|
| RuntimeDown | SEV1 | PagerDuty (immediate) |
| HighErrorRate | SEV2 | PagerDuty (5 min) |
| CanaryFailing | SEV2 | Slack + PagerDuty |
| MemoryHigh | SEV3 | Slack only |
| LatencyDegraded | SEV3 | Slack only |
