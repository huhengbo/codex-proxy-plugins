use gateway_plugin_sdk::client::HostClient;

use crate::state::{KeyResetResult, KeyResetStatus, ResetOutcome};

pub struct ResetExecution {
    pub outcome: ResetOutcome,
    pub keys: Vec<KeyResetResult>,
}

/// 对一次已确认的周额度重置执行所有关联 Key 的同步动作。
///
/// 当前 SDK 尚未开放已有 Client Key 的预算重置能力，因此非 dry-run 模式会明确记录
/// `sdk_unavailable`，不会假装重置成功。SDK 增加对应能力后，只需要替换
/// `reset_one_weekly_key` 的实现。
pub async fn reset_weekly_keys(
    host: &HostClient,
    key_ids: &[String],
    dry_run: bool,
) -> ResetExecution {
    if dry_run {
        return ResetExecution {
            outcome: ResetOutcome::DryRun,
            keys: key_ids
                .iter()
                .map(|key_id| KeyResetResult {
                    key_id: key_id.clone(),
                    status: KeyResetStatus::DryRun,
                })
                .collect(),
        };
    }

    let mut keys = Vec::with_capacity(key_ids.len());
    for key_id in key_ids {
        keys.push(KeyResetResult {
            key_id: key_id.clone(),
            status: reset_one_weekly_key(host, key_id).await,
        });
    }
    ResetExecution {
        outcome: ResetOutcome::SdkUnavailable,
        keys,
    }
}

/// SDK 适配边界：上游 issue #299 落地后，把这里替换为官方的 weekly budget reset 回调。
///
/// 保持这一层只接收宿主 Key ID，不读取 Key 明文，也不要通过 Admin API Key 绕过插件权限体系。
async fn reset_one_weekly_key(_host: &HostClient, _key_id: &str) -> KeyResetStatus {
    // TODO(zyycn/codex-proxy-rs#299):
    // 期望形态示意（实际名称和授权语义以项目方最终 SDK 为准）：
    //
    // _host
    //     .reset_key_budget(KeyBudgetResetRequest {
    //         key_id: _key_id.to_owned(),
    //         period: KeyBudgetPeriod::Weekly,
    //     })
    //     .await?;
    //
    // 在官方合同出现前不发送未知 RPC 方法，避免依赖未冻结的私有协议。
    KeyResetStatus::SdkUnavailable
}
