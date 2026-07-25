//! Security Pipeline
//!
//! Pure, value-oriented facade over the ingress prompt-injection filter and
//! the egress output sanitizer. Each stage is optional, gated by
//! [`SecurityConfig`] flags at construction time.
//!
//! # Design (LD-1: pure pipeline, effects at the edge)
//! The pipeline performs NO effects: no telemetry, no logging, no I/O.
//! Both methods return outcome values (including stage latency) and the
//! caller — the scheduler worker — owns all side effects such as metric
//! emission and response sending.

use std::time::Instant;

use super::output_sanitizer::{OutputSanitizer, SanitizerConfig};
use super::prompt_injection::PromptInjectionFilter;
use super::SecurityConfig;

/// Verdict from an ingress prompt scan
#[derive(Debug, Clone)]
pub struct ScanVerdict {
    /// Whether the prompt is admitted to inference
    pub allowed: bool,
    /// Risk score (0-100) reported by the injection filter
    pub risk_score: u8,
    /// Scan duration in microseconds
    pub latency_us: u64,
}

/// Outcome of egress output sanitization
#[derive(Debug, Clone)]
pub struct SanitizedOutput {
    /// The (possibly rewritten) output text
    pub output: String,
    /// Whether the output was modified
    pub modified: bool,
    /// Number of redactions applied (PII redactions + content filters)
    pub redactions: usize,
    /// Sanitization duration in microseconds
    pub latency_us: u64,
}

/// Security pipeline facade owning the optional ingress/egress stages
pub struct SecurityPipeline {
    /// Ingress stage: prompt injection filter (None when disabled)
    injection: Option<PromptInjectionFilter>,
    /// Whether a detected injection blocks the request (vs. detect-only)
    block_on_detection: bool,
    /// Egress stage: output sanitizer (None when disabled)
    sanitizer: Option<OutputSanitizer>,
}

impl SecurityPipeline {
    /// Build a pipeline from a [`SecurityConfig`], enabling each stage
    /// only when its corresponding flag is set.
    pub fn from_config(cfg: &SecurityConfig) -> Self {
        let injection = cfg
            .enable_prompt_injection_detection
            .then(|| PromptInjectionFilter::new(cfg.block_prompt_injection));
        let sanitizer = cfg.enable_pii_detection.then(|| {
            OutputSanitizer::new(SanitizerConfig {
                redact_pii: cfg.redact_pii,
                ..SanitizerConfig::default()
            })
        });
        Self {
            injection,
            block_on_detection: cfg.block_prompt_injection,
            sanitizer,
        }
    }

    /// Build a pipeline from environment variables.
    /// See [`SecurityConfig::from_env`] for the recognized variables.
    pub fn from_env() -> Self {
        Self::from_config(&SecurityConfig::from_env())
    }

    /// Scan an inbound prompt for injection patterns.
    ///
    /// With the ingress stage disabled the prompt is always allowed with a
    /// zero risk score. In detect-only mode (`block_on_detection == false`)
    /// detections are scored but never block.
    pub fn scan_prompt(&self, prompt: &str) -> ScanVerdict {
        let start = Instant::now();
        let Some(filter) = &self.injection else {
            return ScanVerdict {
                allowed: true,
                risk_score: 0,
                latency_us: elapsed_us(start),
            };
        };
        // Block on aggregate RISK, not any single incidental substring match.
        // Case-insensitive substrings like "```", "---", or "dan" (matching
        // "abundant") would otherwise brick ordinary prompts. Genuine
        // injections score high (high-risk patterns are severity 5, +30 each)
        // or carry a severity-5 match, so both criteria admit them.
        const BLOCK_RISK_THRESHOLD: u8 = 50;
        let (_safe, risk_score, matches) = filter.scan(prompt);
        let has_high_severity = matches.iter().any(|m| m.severity >= 5);
        let blocked =
            self.block_on_detection && (risk_score >= BLOCK_RISK_THRESHOLD || has_high_severity);
        ScanVerdict {
            allowed: !blocked,
            risk_score,
            latency_us: elapsed_us(start),
        }
    }

    /// Sanitize generated output before it leaves the runtime.
    ///
    /// With the egress stage disabled the output passes through unmodified.
    pub fn sanitize_output(&self, output: &str) -> SanitizedOutput {
        let start = Instant::now();
        let Some(sanitizer) = &self.sanitizer else {
            return SanitizedOutput {
                output: output.to_string(),
                modified: false,
                redactions: 0,
                latency_us: elapsed_us(start),
            };
        };
        let result = sanitizer.sanitize(output);
        SanitizedOutput {
            output: result.output,
            modified: result.modified,
            redactions: result.pii_redacted + result.content_filtered,
            latency_us: elapsed_us(start),
        }
    }
}

/// Elapsed microseconds since `start`, saturating at `u64::MAX`.
fn elapsed_us(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
