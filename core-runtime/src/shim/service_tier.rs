//! Service tier definitions for multi-tenant resource allocation.
//!
//! Maps commercial service tiers to internal priority levels.
//! Default tier is Silver (normal priority).

use crate::scheduler::Priority;

/// Service tier for request prioritization.
///
/// Higher tiers receive higher priority and more generous rate limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceTier {
    /// Basic tier: low priority, strict rate limits.
    Bronze,
    /// Standard tier: normal priority, moderate rate limits.
    Silver,
    /// Premium tier: high priority, generous rate limits.
    Gold,
}

impl Default for ServiceTier {
    fn default() -> Self {
        Self::Silver
    }
}

impl ServiceTier {
    /// Map tier to scheduler priority.
    pub fn to_priority(self) -> Priority {
        match self {
            Self::Bronze => Priority::Low,
            Self::Silver => Priority::Normal,
            Self::Gold => Priority::High,
        }
    }

    /// Requests per second allowed for this tier.
    pub fn rate_limit_rps(self) -> u32 {
        match self {
            Self::Bronze => 5,
            Self::Silver => 20,
            Self::Gold => 100,
        }
    }

    /// Token bucket burst capacity for this tier.
    pub fn burst_capacity(self) -> u32 {
        match self {
            Self::Bronze => 10,
            Self::Silver => 40,
            Self::Gold => 200,
        }
    }
}

impl std::fmt::Display for ServiceTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bronze => write!(f, "Bronze"),
            Self::Silver => write!(f, "Silver"),
            Self::Gold => write!(f, "Gold"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_silver() {
        assert_eq!(ServiceTier::default(), ServiceTier::Silver);
    }

    #[test]
    fn test_tier_to_priority_ordering() {
        assert_eq!(ServiceTier::Bronze.to_priority(), Priority::Low);
        assert_eq!(ServiceTier::Silver.to_priority(), Priority::Normal);
        assert_eq!(ServiceTier::Gold.to_priority(), Priority::High);
    }

    #[test]
    fn test_rate_limits_increase_with_tier() {
        assert!(ServiceTier::Bronze.rate_limit_rps() < ServiceTier::Silver.rate_limit_rps());
        assert!(ServiceTier::Silver.rate_limit_rps() < ServiceTier::Gold.rate_limit_rps());
    }

    #[test]
    fn test_burst_increases_with_tier() {
        assert!(ServiceTier::Bronze.burst_capacity() < ServiceTier::Silver.burst_capacity());
        assert!(ServiceTier::Silver.burst_capacity() < ServiceTier::Gold.burst_capacity());
    }

    #[test]
    fn test_display() {
        assert_eq!(ServiceTier::Gold.to_string(), "Gold");
    }
}
