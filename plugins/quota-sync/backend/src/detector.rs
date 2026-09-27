use crate::{
    config::Config,
    state::{AccountRuntime, ConfirmedReset, PendingReset, ResetKind, WindowSnapshot},
};

pub struct ObservationResult {
    pub changed: bool,
    pub confirmed: Option<ConfirmedReset>,
}

pub fn observe(
    state: &mut AccountRuntime,
    current: WindowSnapshot,
    config: &Config,
) -> ObservationResult {
    let Some(baseline) = state.baseline.clone() else {
        state.baseline = Some(current);
        state.pending = None;
        return ObservationResult {
            changed: true,
            confirmed: None,
        };
    };

    if current.window_key != baseline.window_key {
        state.baseline = Some(current);
        state.pending = None;
        return ObservationResult {
            changed: true,
            confirmed: None,
        };
    }

    if let Some(pending) = state.pending.clone() {
        if current.observed_at_ms <= pending.candidate.observed_at_ms {
            return ObservationResult {
                changed: false,
                confirmed: None,
            };
        }
        if confirms(&pending, &current, config) {
            state.baseline = Some(current.clone());
            state.pending = None;
            return ObservationResult {
                changed: true,
                confirmed: Some(ConfirmedReset {
                    kind: pending.kind,
                    before: pending.before,
                    after: current,
                }),
            };
        }
        if let Some(kind) = candidate(&pending.before, &current, config) {
            state.pending = Some(PendingReset {
                kind,
                before: pending.before,
                candidate: current,
            });
        } else {
            state.baseline = Some(current);
            state.pending = None;
        }
        return ObservationResult {
            changed: true,
            confirmed: None,
        };
    }

    if current.observed_at_ms <= baseline.observed_at_ms {
        return ObservationResult {
            changed: false,
            confirmed: None,
        };
    }

    if let Some(kind) = candidate(&baseline, &current, config) {
        state.pending = Some(PendingReset {
            kind,
            before: baseline,
            candidate: current,
        });
    } else {
        state.baseline = Some(current);
    }
    ObservationResult {
        changed: true,
        confirmed: None,
    }
}

fn candidate(
    before: &WindowSnapshot,
    current: &WindowSnapshot,
    config: &Config,
) -> Option<ResetKind> {
    let drop = before.used_percent - current.used_percent;
    let reset_advanced = matches!(
        (before.reset_at_ms, current.reset_at_ms),
        (Some(previous), Some(next)) if next > previous
    );
    let grace_ms = millis(config.boundary_grace_seconds);
    let old_boundary_reached = before
        .reset_at_ms
        .is_some_and(|reset_at| current.observed_at_ms.saturating_add(grace_ms) >= reset_at);
    let post_reset_low = current.used_percent <= config.post_reset_max_used_percent;

    if reset_advanced
        && post_reset_low
        && (old_boundary_reached || drop >= config.early_reset_drop_percent)
    {
        return Some(ResetKind::Boundary);
    }
    if drop >= config.early_reset_drop_percent && post_reset_low {
        return Some(ResetKind::EarlyRecovery);
    }
    None
}

fn confirms(pending: &PendingReset, current: &WindowSnapshot, config: &Config) -> bool {
    if current.window_key != pending.candidate.window_key {
        return false;
    }
    let maximum_used =
        (pending.candidate.used_percent + config.confirmation_growth_percent).min(100.0);
    if current.used_percent > maximum_used {
        return false;
    }
    match pending.kind {
        ResetKind::Boundary => match (pending.candidate.reset_at_ms, current.reset_at_ms) {
            (Some(candidate), Some(now)) => now >= candidate,
            _ => false,
        },
        ResetKind::EarlyRecovery => match (pending.candidate.reset_at_ms, current.reset_at_ms) {
            (Some(candidate), Some(now)) => now >= candidate,
            _ => true,
        },
    }
}

fn millis(seconds: u64) -> i64 {
    i64::try_from(seconds)
        .ok()
        .and_then(|value| value.checked_mul(1_000))
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::observe;
    use crate::{
        config::Config,
        state::{AccountRuntime, ResetKind, WindowSnapshot},
    };

    fn snapshot(observed: i64, used: f64, reset: i64) -> WindowSnapshot {
        WindowSnapshot {
            window_key: "weekly".to_owned(),
            observed_at_ms: observed,
            used_percent: used,
            reset_at_ms: Some(reset),
        }
    }

    #[test]
    fn first_observation_only_builds_baseline() {
        let mut state = AccountRuntime::new("acct".to_owned());
        let result = observe(
            &mut state,
            snapshot(1_000, 80.0, 10_000),
            &Config::default(),
        );
        assert!(result.changed);
        assert!(result.confirmed.is_none());
        assert!(state.pending.is_none());
    }

    #[test]
    fn boundary_reset_requires_second_fresh_sample() {
        let mut state = AccountRuntime::new("acct".to_owned());
        let config = Config::default();
        let _ = observe(&mut state, snapshot(1_000, 80.0, 2_000), &config);
        let first = observe(&mut state, snapshot(2_100, 2.0, 20_000), &config);
        assert!(first.confirmed.is_none());
        assert!(state.pending.is_some());

        let second = observe(&mut state, snapshot(2_200, 4.0, 20_000), &config);
        let confirmed = second.confirmed.expect("reset should be confirmed");
        assert_eq!(confirmed.kind, ResetKind::Boundary);
        assert!(state.pending.is_none());
    }

    #[test]
    fn early_recovery_requires_large_drop_and_confirmation() {
        let mut state = AccountRuntime::new("acct".to_owned());
        let config = Config::default();
        let _ = observe(&mut state, snapshot(1_000, 90.0, 50_000), &config);
        let first = observe(&mut state, snapshot(2_000, 10.0, 50_000), &config);
        assert!(first.confirmed.is_none());
        assert!(state.pending.is_some());

        let second = observe(&mut state, snapshot(3_000, 12.0, 50_000), &config);
        let confirmed = second.confirmed.expect("recovery should be confirmed");
        assert_eq!(confirmed.kind, ResetKind::EarlyRecovery);
    }

    #[test]
    fn rebound_cancels_pending_reset() {
        let mut state = AccountRuntime::new("acct".to_owned());
        let config = Config::default();
        let _ = observe(&mut state, snapshot(1_000, 90.0, 50_000), &config);
        let _ = observe(&mut state, snapshot(2_000, 10.0, 50_000), &config);
        let result = observe(&mut state, snapshot(3_000, 80.0, 50_000), &config);
        assert!(result.confirmed.is_none());
        assert!(state.pending.is_none());
        assert_eq!(
            state.baseline.as_ref().map(|value| value.used_percent),
            Some(80.0)
        );
    }

    #[test]
    fn same_sample_cannot_confirm_pending_reset() {
        let mut state = AccountRuntime::new("acct".to_owned());
        let config = Config::default();
        let _ = observe(&mut state, snapshot(1_000, 90.0, 50_000), &config);
        let _ = observe(&mut state, snapshot(2_000, 10.0, 50_000), &config);
        let result = observe(&mut state, snapshot(2_000, 10.0, 50_000), &config);
        assert!(!result.changed);
        assert!(result.confirmed.is_none());
        assert!(state.pending.is_some());
    }

    #[test]
    fn confirmed_reset_is_not_repeated_without_another_drop() {
        let mut state = AccountRuntime::new("acct".to_owned());
        let config = Config::default();
        let _ = observe(&mut state, snapshot(1_000, 90.0, 2_000), &config);
        let _ = observe(&mut state, snapshot(2_100, 2.0, 20_000), &config);
        let confirmed = observe(&mut state, snapshot(2_200, 4.0, 20_000), &config);
        assert!(confirmed.confirmed.is_some());

        let later = observe(&mut state, snapshot(2_300, 6.0, 20_000), &config);
        assert!(later.confirmed.is_none());
        assert!(state.pending.is_none());
    }
}
