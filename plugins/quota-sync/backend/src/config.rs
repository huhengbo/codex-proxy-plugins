use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

const DEFAULT_WEEKLY_WINDOW_SECONDS: u64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub dry_run: bool,
    pub mappings: Vec<AccountMapping>,
    pub weekly_window_seconds: u64,
    pub max_observation_age_seconds: u64,
    pub boundary_grace_seconds: u64,
    pub early_reset_drop_percent: f64,
    pub post_reset_max_used_percent: f64,
    pub confirmation_growth_percent: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dry_run: true,
            mappings: Vec::new(),
            weekly_window_seconds: DEFAULT_WEEKLY_WINDOW_SECONDS,
            max_observation_age_seconds: 30 * 60,
            boundary_grace_seconds: 5 * 60,
            early_reset_drop_percent: 50.0,
            post_reset_max_used_percent: 25.0,
            confirmation_growth_percent: 20.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountMapping {
    pub account_id: String,
    pub client_key_ids: Vec<String>,
    #[serde(default)]
    pub quota_window_key: Option<String>,
}

impl Config {
    /// 验证无法完全由 JSON Schema 表达的跨字段约束。
    ///
    /// # Errors
    ///
    /// 配置包含重复账号、重复 Key、空标识或非法检测阈值时返回错误。
    pub fn validate(&self) -> Result<(), String> {
        if self.weekly_window_seconds == 0 {
            return Err("weeklyWindowSeconds 必须大于 0".to_owned());
        }
        if self.max_observation_age_seconds == 0 {
            return Err("maxObservationAgeSeconds 必须大于 0".to_owned());
        }
        for (name, value) in [
            ("earlyResetDropPercent", self.early_reset_drop_percent),
            ("postResetMaxUsedPercent", self.post_reset_max_used_percent),
            ("confirmationGrowthPercent", self.confirmation_growth_percent),
        ] {
            if !value.is_finite() || !(0.0..=100.0).contains(&value) {
                return Err(format!("{name} 必须在 0 到 100 之间"));
            }
        }

        let mut accounts = BTreeSet::new();
        for mapping in &self.mappings {
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
            let mut keys = BTreeSet::new();
            for key_id in &mapping.client_key_ids {
                if key_id.trim().is_empty() {
                    return Err(format!("账号 {} 包含空 Client Key ID", mapping.account_id));
                }
                if !keys.insert(key_id.as_str()) {
                    return Err(format!(
                        "账号 {} 包含重复 Client Key ID",
                        mapping.account_id
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
}

#[cfg(test)]
mod tests {
    use super::{AccountMapping, Config};

    #[test]
    fn duplicate_account_should_be_rejected() {
        let mapping = AccountMapping {
            account_id: "acct_a".to_owned(),
            client_key_ids: vec!["key_a".to_owned()],
            quota_window_key: None,
        };
        let config = Config {
            mappings: vec![mapping.clone(), mapping],
            ..Config::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn dry_run_should_be_enabled_by_default() {
        assert!(Config::default().dry_run);
    }
}
