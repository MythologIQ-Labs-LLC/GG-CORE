//! Service-tier-aware model routing.
//!
//! Maps ServiceTier to appropriate model selection hints,
//! bridging the shim layer with the TierSynergy engine.

use crate::scheduler::Priority;
use crate::shim::ServiceTier;
use super::smart_loader::LoadHint;

/// Route a request to the appropriate model hint based on service tier.
///
/// Gold tier gets complex/quality models, Bronze gets light/fast models.
pub fn tier_to_load_hint(tier: ServiceTier) -> LoadHint {
    match tier {
        ServiceTier::Bronze => LoadHint::QuickQuery,
        ServiceTier::Silver => LoadHint::UserIdle,
        ServiceTier::Gold => LoadHint::ComplexTask,
    }
}

/// Resolve the effective priority, preferring interceptor override.
///
/// If the shim interceptor provides a priority, use it.
/// Otherwise fall back to the ServiceTier default mapping.
pub fn resolve_priority(
    interceptor_priority: Option<Priority>,
    tier: ServiceTier,
) -> Priority {
    interceptor_priority.unwrap_or_else(|| tier.to_priority())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bronze_routes_to_quick() {
        assert_eq!(tier_to_load_hint(ServiceTier::Bronze), LoadHint::QuickQuery);
    }

    #[test]
    fn test_gold_routes_to_complex() {
        assert_eq!(tier_to_load_hint(ServiceTier::Gold), LoadHint::ComplexTask);
    }

    #[test]
    fn test_resolve_priority_uses_override() {
        let result = resolve_priority(Some(Priority::Critical), ServiceTier::Bronze);
        assert_eq!(result, Priority::Critical);
    }

    #[test]
    fn test_resolve_priority_falls_back_to_tier() {
        let result = resolve_priority(None, ServiceTier::Gold);
        assert_eq!(result, Priority::High);
    }
}
