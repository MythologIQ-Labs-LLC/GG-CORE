//! Per-context CPU thread policy (HOL plan, Addendum A, revision 6).
//!
//! Auto (`GG_CORE_N_THREADS` unset or 0): `max(1, min(logical, 16) / workers)`
//! so `workers` concurrent contexts share the cores instead of each claiming
//! all of them. An explicit `GG_CORE_N_THREADS` is used per context unchanged.
//! `workers` is the number of contexts the caller's path can run at once: the
//! daemon's worker-loop count, or 1 for embedded paths (not pool-bounded).

/// Cap on auto-resolved threads per context (diminishing returns above it).
const MAX_AUTO_THREADS: usize = 16;

/// Threads per context for `explicit` (0 = auto), `workers` and `logical` cores.
pub fn per_context(explicit: u32, workers: usize, logical: usize) -> u32 {
    if explicit > 0 {
        return explicit;
    }
    let cores = logical.clamp(1, MAX_AUTO_THREADS);
    let share = (cores / workers.max(1)).max(1);
    u32::try_from(share).unwrap_or(1)
}

/// [`per_context`] on this host's logical core count.
pub fn per_context_for_host(explicit: u32, workers: usize) -> u32 {
    per_context(explicit, workers, num_cpus::get())
}

/// Operator-facing description of the resolved value (`config show`).
pub fn describe(explicit: u32, workers: usize) -> String {
    if explicit > 0 {
        return format!("{explicit} (explicit, per context)");
    }
    let logical = num_cpus::get();
    let n = per_context(0, workers, logical);
    format!(
        "0 (auto: {n} threads per context = min({logical} logical, {MAX_AUTO_THREADS}) / {} workers)",
        workers.max(1)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_value_is_used_unchanged() {
        for workers in [0, 1, 2, 8, 64] {
            assert_eq!(per_context(6, workers, 12), 6);
        }
    }

    #[test]
    fn auto_divides_cores_by_workers() {
        let got: Vec<u32> = [1, 2, 4, 8]
            .iter()
            .map(|&w| per_context(0, w, 12))
            .collect();
        assert_eq!(got, [12, 6, 3, 1]);
    }

    #[test]
    fn auto_is_at_least_one_and_treats_zero_workers_as_one() {
        assert_eq!(per_context(0, 32, 12), 1);
        assert_eq!(per_context(0, 0, 12), 12);
        assert_eq!(per_context(0, 1, 0), 1);
        assert_eq!(per_context(0, 4, 1), 1);
    }

    #[test]
    fn auto_keeps_the_sixteen_thread_cap() {
        assert_eq!(per_context(0, 1, 32), 16);
        assert_eq!(per_context(0, 2, 32), 8);
    }

    #[test]
    fn describe_names_auto_and_explicit() {
        assert!(describe(0, 2).contains("/ 2 workers"), "{}", describe(0, 2));
        assert_eq!(describe(5, 2), "5 (explicit, per context)");
    }
}
