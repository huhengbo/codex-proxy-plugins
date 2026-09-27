use gateway_plugin_sdk::{
    call::{
        data::ClientKeyFactsQuery,
        key_budgets::{BudgetPeriod, ResetKeyBudgetRequest},
    },
    client::HostClient,
};

use crate::{scope::key_scope_allows_account, state::KeyResetStatus};

/// 对单个 Key 执行一次 weekly reset。
///
/// 写操作返回错误时不能判断宿主是否已经提交，因此统一记为 FailedUnknown，调用方不得自动重试。
pub async fn reset_weekly_key(
    host: &HostClient,
    account_group_ids: &[String],
    key_id: &str,
) -> KeyResetStatus {
    let facts = match host
        .key_facts(ClientKeyFactsQuery {
            client_key_id: key_id.to_owned(),
        })
        .await
    {
        Ok(facts) => facts,
        Err(_) => return KeyResetStatus::FactsUnavailable,
    };

    if !key_scope_allows_account(&facts.group_ids, account_group_ids) {
        return KeyResetStatus::ScopeMismatch;
    }

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
