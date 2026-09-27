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
}

impl Default for ManagedSettings {
    fn default() -> Self {
        Self {
            dry_run: true,
            mappings: Vec::new(),
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
}

impl AccountRuntime {
    pub fn new(account_id: String) -> Self {
        Self {
            account_id,
            baseline: None,
            pending: None,
            last_error: None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetOutcome {
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
    DryRun,
    Reset,
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

    pub fn push_event(&mut self, event: SyncEvent) {
        self.events.push(event);
        if self.events.len() > MAX_EVENTS {
            let overflow = self.events.len() - MAX_EVENTS;
            drop(self.events.drain(..overflow));
        }
    }
}
