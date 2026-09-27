use gateway_plugin_sdk::{
    call::key_budgets::{BudgetPeriod, ResetKeyBudgetRequest},
    client::HostClient,
};

use crate::state::{KeyResetResult, KeyResetStatus, ResetOutcome};

pub struct ResetExecution {
    pub outcome: ResetOutcome,
    pub keys: Vec<KeyResetResult>,
}

/// 对一次已确认的周额度重置执行所有关联 Key 的同步动作。
///
/// SDK 明确规定结果未知时不能盲目重试，因此单个 Key 的失败只记录结果；本次检测事件
/// 仍会推进 baseline，不在下一轮 maintenance 自动重复清零。
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
    let outcome = if keys
        .iter()
        .all(|result| result.status == KeyResetStatus::Reset)
    {
        ResetOutcome::Completed
    } else {
        ResetOutcome::PartialFailure
    };
    ResetExecution { outcome, keys }
}

async fn reset_one_weekly_key(host: &HostClient, key_id: &str) -> KeyResetStatus {
    match host
        .reset_key_budget(ResetKeyBudgetRequest {
            client_key_id: key_id.to_owned(),
            period: BudgetPeriod::Weekly,
        })
        .await
    {
        Ok(_) => KeyResetStatus::Reset,
        Err(_) => KeyResetStatus::FailedUnknown,
    }
}
