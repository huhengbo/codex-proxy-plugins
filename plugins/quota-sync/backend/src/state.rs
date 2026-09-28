use std::collections::BTreeMap;

use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::host::{StateGetRequest, StateGetResult, StatePutRequest, StatePutResult},
    client::HostClient,
};
use serde::{Deserialize, Serialize};

use crate::{config::AccountMapping, host_calls};

const STATE_NAMESPACE: &str = "quota_sync";
const STATE_KEY: &str = "runtime";
const MAX_EVENTS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedSettings {
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
    #[serde(default)]
    pub mappings: Vec<AccountMapping>,
    #[serde(default)]
    pub account_aliases: BTreeMap<String, String>,
}

impl Default for ManagedSettings {
    fn default() -> Self {
        Self {
            dry_run: true,
            mappings: Vec::new(),
            account_aliases: BTreeMap::new(),
        }
    }
}

const fn default_dry_run() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeState {
    #[serde(default)]
    pub settings: ManagedSettings,
    #[serde(default)]
    pub accounts: Vec<AccountRuntime>,
    #[serde(default)]
    pub events: Vec<SyncEvent>,
    #[serde(default)]
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountRuntime {
    pub account_id: String,
    #[serde(default)]
    pub baseline: Option<WindowSnapshot>,
    #[serde(default)]
    pub pending: Option<PendingReset>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_refresh_attempt_ms: Option<i64>,
}

impl AccountRuntime {
    pub fn new(account_id: String) -> Self {
        Self {
            account_id,
            baseline: None,
            pending: None,
            last_error: None,
            last_refresh_attempt_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowSnapshot {
    pub window_key: String,
    pub observed_at_ms: i64,
    pub used_percent: f64,
    pub reset_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingReset {
    pub kind: ResetKind,
    pub before: WindowSnapshot,
    pub candidate: WindowSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetKind {
    Boundary,
    EarlyRecovery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmedReset {
    pub kind: ResetKind,
    pub before: WindowSnapshot,
    pub after: WindowSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncEvent {
    pub id: String,
    pub account_id: String,
    pub kind: ResetKind,
    pub detected_at_ms: i64,
    pub previous_used_percent: f64,
    pub current_used_percent: f64,
    pub previous_reset_at_ms: Option<i64>,
    pub current_reset_at_ms: Option<i64>,
    pub outcome: ResetOutcome,
    pub keys: Vec<KeyResetResult>,
}

impl SyncEvent {
    pub fn prepared(
        id: String,
        account_id: String,
        confirmed: &ConfirmedReset,
        detected_at_ms: i64,
        key_ids: &[String],
    ) -> Self {
        Self {
            id,
            account_id,
            kind: confirmed.kind,
            detected_at_ms,
            previous_used_percent: confirmed.before.used_percent,
            current_used_percent: confirmed.after.used_percent,
            previous_reset_at_ms: confirmed.before.reset_at_ms,
            current_reset_at_ms: confirmed.after.reset_at_ms,
            outcome: ResetOutcome::Prepared,
            keys: key_ids
                .iter()
                .map(|key_id| KeyResetResult {
                    key_id: key_id.clone(),
                    status: KeyResetStatus::Pending,
                })
                .collect(),
        }
    }

    pub fn set_all_dry_run(&mut self) {
        for key in &mut self.keys {
            key.status = KeyResetStatus::DryRun;
        }
        self.outcome = ResetOutcome::DryRun;
    }

    pub fn set_key_status(&mut self, key_id: &str, status: KeyResetStatus) {
        if let Some(key) = self.keys.iter_mut().find(|key| key.key_id == key_id) {
            key.status = status;
        }
        self.recompute_outcome();
    }

    fn recompute_outcome(&mut self) {
        if self
            .keys
            .iter()
            .any(|key| key.status == KeyResetStatus::Pending)
        {
            self.outcome = ResetOutcome::Prepared;
        } else if self
            .keys
            .iter()
            .all(|key| key.status == KeyResetStatus::DryRun)
        {
            self.outcome = ResetOutcome::DryRun;
        } else if self
            .keys
            .iter()
            .all(|key| key.status == KeyResetStatus::Reset)
        {
            self.outcome = ResetOutcome::Completed;
        } else {
            self.outcome = ResetOutcome::PartialFailure;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetOutcome {
    Prepared,
    DryRun,
    Completed,
    PartialFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyResetResult {
    pub key_id: String,
    pub status: KeyResetStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyResetStatus {
    Pending,
    DryRun,
    Reset,
    ScopeMismatch,
    FactsUnavailable,
    FailedUnknown,
}

pub struct LoadedRuntime {
    pub value: RuntimeState,
    version: Option<u64>,
}

impl LoadedRuntime {
    /// 从插件私有状态读取运行时快照。
    ///
    /// # Errors
    ///
    /// 宿主回调失败或已保存状态无法解码时返回错误。
    pub async fn load(host: &HostClient) -> Result<Self, PluginFault> {
        let result: StateGetResult = host_calls::metadata(
            host,
            "host.state.get",
            &StateGetRequest {
                namespace: STATE_NAMESPACE.to_owned(),
                key: STATE_KEY.to_owned(),
            },
        )
        .await?
        .0;
        let Some(record) = result.record else {
            return Ok(Self {
                value: RuntimeState::default(),
                version: None,
            });
        };
        let value = serde_json::from_value(record.value)
            .map_err(|_| PluginFault::new(ErrorCode::Fault, "quota-sync 私有状态无法解码"))?;
        Ok(Self {
            value,
            version: Some(record.version),
        })
    }

    /// 使用宿主 CAS 写回运行状态。
    ///
    /// # Errors
    ///
    /// 状态编码失败、版本冲突或宿主回调失败时返回错误。
    pub async fn save(&mut self, host: &HostClient) -> Result<(), PluginFault> {
        let request = StatePutRequest {
            namespace: STATE_NAMESPACE.to_owned(),
            key: STATE_KEY.to_owned(),
            value: serde_json::to_value(&self.value)
                .map_err(|_| PluginFault::new(ErrorCode::Fault, "quota-sync 私有状态无法编码"))?,
            expected_version: self.version,
        };
        let result: StatePutResult = host_calls::metadata(host, "host.state.put", &request)
            .await?
            .0;
        self.version = Some(result.version);
        Ok(())
    }
}

impl RuntimeState {
    pub fn account_mut(&mut self, account_id: &str) -> &mut AccountRuntime {
        if let Some(index) = self
            .accounts
            .iter()
            .position(|account| account.account_id == account_id)
        {
            return &mut self.accounts[index];
        }
        self.accounts
            .push(AccountRuntime::new(account_id.to_owned()));
        self.accounts.last_mut().expect("account was just inserted")
    }

    pub fn event_mut(&mut self, event_id: &str) -> Option<&mut SyncEvent> {
        self.events.iter_mut().find(|event| event.id == event_id)
    }

    pub fn contains_event(&self, event_id: &str) -> bool {
        self.events.iter().any(|event| event.id == event_id)
    }

    pub fn push_event(&mut self, event: SyncEvent) {
        self.events.push(event);
        if self.events.len() > MAX_EVENTS {
            let overflow = self.events.len() - MAX_EVENTS;
            drop(self.events.drain(..overflow));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConfirmedReset, KeyResetStatus, ResetKind, ResetOutcome, SyncEvent, WindowSnapshot,
    };

    fn snapshot(observed_at_ms: i64, used_percent: f64) -> WindowSnapshot {
        WindowSnapshot {
            window_key: "weekly".to_owned(),
            observed_at_ms,
            used_percent,
            reset_at_ms: Some(10_000),
        }
    }

    #[test]
    fn event_stays_prepared_until_every_key_has_a_result() {
        let confirmed = ConfirmedReset {
            kind: ResetKind::Boundary,
            before: snapshot(1, 80.0),
            after: snapshot(2, 1.0),
        };
        let mut event = SyncEvent::prepared(
            "event".to_owned(),
            "acct".to_owned(),
            &confirmed,
            2,
            &["key_a".to_owned(), "key_b".to_owned()],
        );
        event.set_key_status("key_a", KeyResetStatus::Reset);
        assert_eq!(event.outcome, ResetOutcome::Prepared);
        event.set_key_status("key_b", KeyResetStatus::Reset);
        assert_eq!(event.outcome, ResetOutcome::Completed);
    }
}
