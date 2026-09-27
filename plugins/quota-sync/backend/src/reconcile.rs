use std::{
    collections::BTreeSet,
    time::{SystemTime, UNIX_EPOCH},
};

use gateway_plugin_sdk::{
    PluginFault,
    call::{
        data::{AccountFactsQuery, QuotaFacts, QuotaFactsQuery, QuotaWindowFacts},
        host::{LogLevel, LogRequest},
    },
    client::{Empty, TypedCall, TypedReply},
};
use serde_json::json;

use crate::{
    config::{AccountMapping, Config},
    detector, host_calls, reset,
    state::{LoadedRuntime, ResetKind, RuntimeState, SyncEvent, WindowSnapshot},
};

pub async fn reconcile(
    config: &Config,
    call: TypedCall<Empty>,
) -> Result<TypedReply<Empty>, PluginFault> {
    let mut runtime = LoadedRuntime::load(&call.host).await?;
    let mut dirty = prune_removed_accounts(&mut runtime.value, config);
    let now_ms = now_ms();
    let openai_accounts = openai_account_ids(&call).await?;

    for mapping in &config.mappings {
        if !openai_accounts.contains(mapping.account_id.as_str()) {
            dirty |= set_error(
                runtime.value.account_mut(&mapping.account_id),
                "openai_account_not_found",
            );
            continue;
        }
        let quota = match call
            .host
            .quota_facts(QuotaFactsQuery {
                account_id: mapping.account_id.clone(),
            })
            .await
        {
            Ok(value) => value,
            Err(_) => {
                dirty |= set_error(
                    runtime.value.account_mut(&mapping.account_id),
                    "quota_unavailable",
                );
                continue;
            }
        };

        let current = match weekly_snapshot(&quota, mapping, config, now_ms) {
            Ok(value) => value,
            Err(code) => {
                dirty |= set_error(runtime.value.account_mut(&mapping.account_id), code);
                continue;
            }
        };

        let observation = {
            let account = runtime.value.account_mut(&mapping.account_id);
            dirty |= clear_error(account);
            detector::observe(account, current, config)
        };
        dirty |= observation.changed;

        let Some(confirmed) = observation.confirmed else {
            continue;
        };
        let execution =
            reset::reset_weekly_keys(&call.host, &mapping.client_key_ids, config.dry_run);
        let event_id = event_id(&mapping.account_id, confirmed.kind, &confirmed.after);
        runtime.value.push_event(SyncEvent {
            id: event_id,
            account_id: mapping.account_id.clone(),
            kind: confirmed.kind,
            detected_at_ms: now_ms,
            previous_used_percent: confirmed.before.used_percent,
            current_used_percent: confirmed.after.used_percent,
            previous_reset_at_ms: confirmed.before.reset_at_ms,
            current_reset_at_ms: confirmed.after.reset_at_ms,
            outcome: execution.outcome,
            keys: execution.keys,
        });
        dirty = true;
        log_reset(&call, mapping, confirmed.kind, config.dry_run).await;
    }

    if dirty {
        runtime.value.updated_at_ms = now_ms;
        runtime.save(&call.host).await?;
    }
    Ok(TypedReply::new(Empty {}))
}

async fn openai_account_ids(call: &TypedCall<Empty>) -> Result<BTreeSet<String>, PluginFault> {
    let mut cursor = None;
    let mut accounts = BTreeSet::new();
    loop {
        let page = call
            .host
            .account_facts(AccountFactsQuery {
                provider_id: Some("openai".to_owned()),
                cursor,
                limit: 200,
            })
            .await?;
        accounts.extend(page.accounts.into_iter().map(|account| account.account_id));
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = Some(next);
    }
    Ok(accounts)
}

fn weekly_snapshot(
    quota: &QuotaFacts,
    mapping: &AccountMapping,
    config: &Config,
    now_ms: i64,
) -> Result<WindowSnapshot, &'static str> {
    let observed_at_ms = quota.observed_at_ms.ok_or("quota_not_observed")?;
    if observed_at_ms < 0 {
        return Err("quota_observed_at_invalid");
    }
    let maximum_age_ms = millis(config.max_observation_age_seconds);
    if now_ms.saturating_sub(observed_at_ms) > maximum_age_ms {
        return Err("quota_observation_stale");
    }
    if observed_at_ms.saturating_sub(now_ms) > millis(5 * 60) {
        return Err("quota_observation_from_future");
    }

    let mut matching = quota
        .windows
        .iter()
        .filter(|window| window.window_seconds == Some(config.weekly_window_seconds))
        .filter(|window| {
            mapping
                .quota_window_key
                .as_deref()
                .is_none_or(|key| window.key == key)
        });
    let Some(window) = matching.next() else {
        return Err("weekly_window_not_found");
    };
    if matching.next().is_some() {
        return Err("weekly_window_ambiguous");
    }
    snapshot(window, observed_at_ms)
}

fn snapshot(window: &QuotaWindowFacts, observed_at_ms: i64) -> Result<WindowSnapshot, &'static str> {
    let used_percent = window.used_percent.ok_or("weekly_used_percent_unknown")?;
    if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
        return Err("weekly_used_percent_invalid");
    }
    Ok(WindowSnapshot {
        window_key: window.key.clone(),
        observed_at_ms,
        used_percent,
        reset_at_ms: window.reset_at_ms,
    })
}

fn prune_removed_accounts(state: &mut RuntimeState, config: &Config) -> bool {
    let configured = config
        .mappings
        .iter()
        .map(|mapping| mapping.account_id.as_str())
        .collect::<BTreeSet<_>>();
    let before = state.accounts.len();
    state
        .accounts
        .retain(|account| configured.contains(account.account_id.as_str()));
    before != state.accounts.len()
}

fn set_error(account: &mut crate::state::AccountRuntime, code: &str) -> bool {
    if account.last_error.as_deref() == Some(code) {
        return false;
    }
    account.last_error = Some(code.to_owned());
    true
}

fn clear_error(account: &mut crate::state::AccountRuntime) -> bool {
    account.last_error.take().is_some()
}

fn event_id(account_id: &str, kind: ResetKind, after: &WindowSnapshot) -> String {
    let kind = match kind {
        ResetKind::Boundary => "boundary",
        ResetKind::EarlyRecovery => "early_recovery",
    };
    format!("{account_id}:{kind}:{}", after.observed_at_ms)
}

async fn log_reset(
    call: &TypedCall<Empty>,
    mapping: &AccountMapping,
    kind: ResetKind,
    dry_run: bool,
) {
    let kind = match kind {
        ResetKind::Boundary => "boundary",
        ResetKind::EarlyRecovery => "early_recovery",
    };
    let request = LogRequest {
        event: "quota_sync.weekly_reset_detected".to_owned(),
        level: LogLevel::Info,
        fields: [
            ("kind".to_owned(), json!(kind)),
            ("dry_run".to_owned(), json!(dry_run)),
            ("account_id".to_owned(), json!(mapping.account_id.as_str())),
            (
                "target_key_count".to_owned(),
                json!(mapping.client_key_ids.len()),
            ),
        ]
        .into_iter()
        .collect(),
    };
    let _ = host_calls::log(&call.host, &request).await;
}

fn now_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u128, |duration| duration.as_millis());
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn millis(seconds: u64) -> i64 {
    i64::try_from(seconds)
        .ok()
        .and_then(|value| value.checked_mul(1_000))
        .unwrap_or(i64::MAX)
}
