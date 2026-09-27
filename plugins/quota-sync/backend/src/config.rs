use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

const DEFAULT_WEEKLY_WINDOW_SECONDS: u64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub weekly_window_seconds: u64,
    pub quota_refresh_interval_seconds: u64,
    pub confirmation_refresh_interval_seconds: u64,
    pub max_observation_age_seconds: u64,
    pub boundary_grace_seconds: u64,
    pub early_reset_drop_percent: f64,
    pub post_reset_max_used_percent: f64,
    pub confirmation_growth_percent: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            weekly_window_seconds: DEFAULT_WEEKLY_WINDOW_SECONDS,
            quota_refresh_interval_seconds: 5 * 60,
            confirmation_refresh_interval_seconds: 60,
            max_observation_age_seconds: 30 * 60,
            boundary_grace_seconds: 5 * 60,
            early_reset_drop_percent: 50.0,
            post_reset_max_used_percent: 25.0,
            confirmation_growth_percent: 20.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountMapping {
    pub account_id: String,
    pub client_key_ids: Vec<String>,
    #[serde(default)]
    pub quota_window_key: Option<String>,
}

impl Config {
    /// 验证额度识别参数。
    ///
    /// # Errors
    ///
    /// 配置中的窗口、刷新周期或百分比阈值非法时返回错误。
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("weeklyWindowSeconds", self.weekly_window_seconds),
            (
                "quotaRefreshIntervalSeconds",
                self.quota_refresh_interval_seconds,
            ),
            (
                "confirmationRefreshIntervalSeconds",
                self.confirmation_refresh_interval_seconds,
            ),
            ("maxObservationAgeSeconds", self.max_observation_age_seconds),
        ] {
            if value == 0 {
                return Err(format!("{name} 必须大于 0"));
            }
        }
        for (name, value) in [
            ("earlyResetDropPercent", self.early_reset_drop_percent),
            ("postResetMaxUsedPercent", self.post_reset_max_used_percent),
            (
                "confirmationGrowthPercent",
                self.confirmation_growth_percent,
            ),
        ] {
            if !value.is_finite() || !(0.0..=100.0).contains(&value) {
                return Err(format!("{name} 必须在 0 到 100 之间"));
            }
        }
        Ok(())
    }
}

/// 验证账号到 Client Key 的业务映射。
///
/// # Errors
///
/// 映射包含重复账号、跨账号重复 Key、空标识或没有关联 Key 时返回错误。
pub(crate) fn validate_mappings(mappings: &[AccountMapping]) -> Result<(), String> {
    let mut accounts = BTreeSet::new();
    let mut assigned_keys = BTreeSet::new();
    for mapping in mappings {
        if mapping.account_id.trim().is_empty() {
            return Err("accountId 不能为空".to_owned());
        }
        if mapping.client_key_ids.is_empty() {
            return Err(format!(
                "账号 {} 至少需要关联一个 Client Key",
                mapping.account_id
            ));
        }
        if !accounts.insert(mapping.account_id.as_str()) {
            return Err(format!("账号 {} 重复配置", mapping.account_id));
        }
        let mut local_keys = BTreeSet::new();
        for key_id in &mapping.client_key_ids {
            if key_id.trim().is_empty() {
                return Err(format!("账号 {} 包含空 Client Key ID", mapping.account_id));
            }
            if !local_keys.insert(key_id.as_str()) {
                return Err(format!(
                    "账号 {} 包含重复 Client Key ID：{}",
                    mapping.account_id, key_id
                ));
            }
            if !assigned_keys.insert(key_id.as_str()) {
                return Err(format!(
                    "Client Key {key_id} 已关联其他监控账号；一个 Key 只能关联一个账号"
                ));
            }
        }
        if mapping
            .quota_window_key
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(format!(
                "账号 {} 的 quotaWindowKey 不能为空",
                mapping.account_id
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AccountMapping, Config, validate_mappings};

    fn mapping(account: &str, key: &str) -> AccountMapping {
        AccountMapping {
            account_id: account.to_owned(),
            client_key_ids: vec![key.to_owned()],
            quota_window_key: None,
        }
    }

    #[test]
    fn duplicate_account_should_be_rejected() {
        let value = mapping("acct_a", "key_a");
        assert!(validate_mappings(&[value.clone(), value]).is_err());
    }

    #[test]
    fn one_key_cannot_follow_multiple_accounts() {
        assert!(
            validate_mappings(&[
                mapping("acct_a", "key_shared"),
                mapping("acct_b", "key_shared"),
            ])
            .is_err()
        );
    }

    #[test]
    fn default_detection_config_should_be_valid() {
        assert!(Config::default().validate().is_ok());
    }
}
