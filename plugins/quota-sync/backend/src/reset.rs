use gateway_plugin_sdk::client::HostClient;

use crate::state::{KeyResetResult, KeyResetStatus, ResetOutcome};

pub struct ResetExecution {
    pub outcome: ResetOutcome,
    pub keys: Vec<KeyResetResult>,
}

/// 对一次已确认的周额度重置执行所有关联 Key 的同步动作。
///
/// 当前 SDK 尚未开放已有 Client Key 的预算重置能力，因此非 dry-run 模式会明确记录
/// `sdk_unavailable`，不会假装重置成功。SDK 增加对应能力后，只需要替换此模块的调用层。
pub fn reset_weekly_keys(
    _host: &HostClient,
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

    // TODO(zyycn/codex-proxy-rs#299):
    // 在项目方开放受控的 Client Key budget reset SDK 回调后，在这里逐个调用 weekly reset。
    // 不应通过 Admin API Key 绕过插件权限体系。
    ResetExecution {
        outcome: ResetOutcome::SdkUnavailable,
        keys: key_ids
            .iter()
            .map(|key_id| KeyResetResult {
                key_id: key_id.clone(),
                status: KeyResetStatus::SdkUnavailable,
            })
            .collect(),
    }
}
