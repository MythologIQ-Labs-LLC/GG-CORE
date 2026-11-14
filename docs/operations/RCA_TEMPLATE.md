# Root Cause Analysis (RCA) Template

**Document Version:** 1.0.0
**Last Updated:** 2026-02-18
**Applies To:** GG-CORE (procedural template; historical 2026-02 baseline)

This template provides a structured format for conducting blameless post-mortems and root cause analysis for incidents affecting the GG-CORE standalone runtime.

---

## How to Use This Template

1. Copy this template to create a new RCA document
2. Name the file: `RCA-YYYY-MM-DD-<brief-title>.md`
3. Fill in all sections within 48 hours of incident resolution
4. Schedule review meeting within 5 business days
5. Track action items to completion

---

# RCA: [Incident Title]

## Document Information

| Field | Value |
|-------|-------|
| Incident ID | INC-YYYY-NNNN |
| Severity | SEV1 / SEV2 / SEV3 |
| Date | YYYY-MM-DD |
| Author | [Name] |
| Reviewers | [Names] |
| Status | Draft / In Review / Approved |

---

## 1. Incident Summary

### 1.1 Executive Summary

_One paragraph summary suitable for leadership. Include impact, duration, and root cause at a high level._

```
[Write executive summary here]
```

### 1.2 Timeline

| Time (UTC) | Event | Actor |
|------------|-------|-------|
| YYYY-MM-DD HH:MM | [First symptom observed] | System/Person |
| YYYY-MM-DD HH:MM | [Alert triggered] | AlertManager |
| YYYY-MM-DD HH:MM | [Incident declared] | [Name] |
| YYYY-MM-DD HH:MM | [Mitigation started] | [Name] |
| YYYY-MM-DD HH:MM | [Mitigation complete] | [Name] |
| YYYY-MM-DD HH:MM | [Root cause identified] | [Name] |
| YYYY-MM-DD HH:MM | [Fix deployed] | [Name] |
| YYYY-MM-DD HH:MM | [Incident resolved] | [Name] |

### 1.3 Impact Assessment

| Metric | Value |
|--------|-------|
| Duration | X hours Y minutes |
| Users Affected | [Number or percentage] |
| Requests Failed | [Number] |
| Revenue Impact | [If applicable] |
| SLA Breach | Yes / No |

**Affected Components:**
- [ ] CORE Runtime pods
- [ ] IPC communication
- [ ] Model loading
- [ ] Inference execution
- [ ] Canary deployment
- [ ] Blue-green deployment
- [ ] Metrics collection
- [ ] Other: _____________

### 1.4 Detection Method

**How was the incident detected?**
- [ ] Automated alert (specify: _____________)
- [ ] User report
- [ ] Manual monitoring
- [ ] External notification
- [ ] Other: _____________

**Detection latency:** [Time from first symptom to detection]

---

## 2. Root Cause Analysis

### 2.1 The 5 Whys

_Start with the symptom and ask "why" until you reach the root cause._

**Symptom:** [What was observed]

1. **Why did [symptom] occur?**
   - Because: [Answer]

2. **Why did [answer 1] occur?**
   - Because: [Answer]

3. **Why did [answer 2] occur?**
   - Because: [Answer]

4. **Why did [answer 3] occur?**
   - Because: [Answer]

5. **Why did [answer 4] occur?**
   - Because: [Answer] <- ROOT CAUSE

### 2.2 Root Cause Statement

```
[Clear, concise statement of the root cause]
```

### 2.3 Contributing Factors

_List factors that did not directly cause the incident but made it worse or prolonged resolution._

| Factor | Description | Contribution |
|--------|-------------|--------------|
| [Factor 1] | [Description] | [How it contributed] |
| [Factor 2] | [Description] | [How it contributed] |
| [Factor 3] | [Description] | [How it contributed] |

### 2.4 Timeline Reconstruction

_Detailed technical timeline with evidence._

```
[Include relevant log snippets, metric graphs, or command outputs]
```

**Evidence Collected:**
- Log file: [path/location]
- Metrics snapshot: [link/location]
- Configuration at time of incident: [link/location]
- Related alerts: [list]

---

## 3. Response Analysis

### 3.1 What Went Well

| Item | Description |
|------|-------------|
| [Item 1] | [What worked and why] |
| [Item 2] | [What worked and why] |
| [Item 3] | [What worked and why] |

### 3.2 What Could Be Improved

| Item | Description | Proposed Improvement |
|------|-------------|---------------------|
| [Item 1] | [What did not work] | [How to improve] |
| [Item 2] | [What did not work] | [How to improve] |
| [Item 3] | [What did not work] | [How to improve] |

### 3.3 Lucky Factors

_Things that prevented the incident from being worse._

- [Factor 1]
- [Factor 2]

---

## 4. Action Items

### 4.1 Immediate Fixes (Complete within 1 week)

| ID | Action | Owner | Due Date | Status |
|----|--------|-------|----------|--------|
| AI-001 | [Action description] | [Name] | YYYY-MM-DD | Not Started / In Progress / Complete |
| AI-002 | [Action description] | [Name] | YYYY-MM-DD | Not Started / In Progress / Complete |

### 4.2 Long-Term Improvements (Complete within 1 quarter)

| ID | Action | Owner | Due Date | Status |
|----|--------|-------|----------|--------|
| AI-003 | [Action description] | [Name] | YYYY-MM-DD | Not Started / In Progress / Complete |
| AI-004 | [Action description] | [Name] | YYYY-MM-DD | Not Started / In Progress / Complete |

### 4.3 Prevention Measures

| Category | Measure | Implementation |
|----------|---------|----------------|
| Monitoring | [New alert or dashboard] | [How to implement] |
| Testing | [New test case] | [How to implement] |
| Process | [Process change] | [How to implement] |
| Documentation | [Doc update] | [How to implement] |
| Architecture | [System change] | [How to implement] |

---

## 5. Lessons Learned

### 5.1 Key Takeaways

1. [Lesson 1]
2. [Lesson 2]
3. [Lesson 3]

### 5.2 Knowledge Sharing

**Documentation Updates:**
- [ ] Runbook updated
- [ ] Troubleshooting guide updated
- [ ] Architecture docs updated
- [ ] Alert thresholds adjusted

**Training Needs:**
- [ ] Team briefing scheduled
- [ ] Knowledge base article created
- [ ] Incident simulation planned

---

## 6. Appendices

### Appendix A: Raw Logs

```
[Include relevant log excerpts]
```

### Appendix B: Metrics Graphs

_Attach or link to relevant metrics visualizations._

### Appendix C: Configuration Snapshots

```yaml
# Relevant configuration at time of incident
```

### Appendix D: Related Incidents

| Incident ID | Date | Relationship |
|-------------|------|--------------|
| [ID] | [Date] | [How related] |

---

## 7. Approval

| Role | Name | Date | Signature |
|------|------|------|-----------|
| Author | | | |
| Reviewer | | | |
| Engineering Manager | | | |

---

## Change Log

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | YYYY-MM-DD | [Name] | Initial draft |
| 1.1 | YYYY-MM-DD | [Name] | [Changes] |
