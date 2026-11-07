//! Ingress security decision shared by every worker request kind, so the
//! generation and typed paths cannot drift in policy, wording or telemetry.

use crate::security::SecurityPipeline;
use crate::telemetry;

/// Caller-facing rejection message (identical for every request kind).
pub(crate) const INGRESS_REJECTED: &str = "request rejected by security policy";

/// Scan every input text. Any blocked text rejects the whole request; each
/// scan is recorded without content. No pipeline means no ingress policy
/// (test/embedded use only; production always attaches one).
pub(crate) fn scan_texts(
    security: Option<&SecurityPipeline>,
    model_id: &str,
    texts: &[&str],
) -> Result<(), String> {
    let Some(sec) = security else {
        return Ok(());
    };
    for text in texts {
        let verdict = sec.scan_prompt(text);
        telemetry::record_security_scan(
            model_id,
            verdict.latency_us,
            verdict.risk_score,
            !verdict.allowed,
        );
        if !verdict.allowed {
            return Err(INGRESS_REJECTED.to_string());
        }
    }
    Ok(())
}
