use std::{
    collections::{BTreeMap, BTreeSet},
    time::{SystemTime, UNIX_EPOCH},
};

use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::{
        data::{
            AccountFacts, AccountFactsQuery, ClientKeyFactsQuery, QuotaFacts, QuotaFactsQuery,
        },
        host::{ClientKey, KeyListRequest},
        key_budgets::KeyBudget,
        management::{
            ManagementPage, ManagementRegistration, ManagementResource, ManagementResponse,
            ManagementRoute,
        },
    },
    client::{HostClient, TypedCall, TypedReply},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    Config,
    config::validate_mappings,
    scope::key_scope_allows_account,
    state::{AccountRuntime, LoadedRuntime, ManagedSettings, SyncEvent},
};

const JSON_CONTENT_TYPE: &str = "application/json";
const MAXIMUM_BODY_BYTES: usize = 256 * 1024;
const MAXIMUM_KEYS: usize = 1_000;

pub fn registration() -> ManagementRegistration {
    let get = |path: &str| ManagementRoute {
        method: "GET".to_owned(),
        path: path.to_owned(),
        request_content_types: Vec::new(),
        response_content_types: vec![JSON_CONTENT_TYPE.to_owned()],
    };
    let post = |path: &str| ManagementRoute {
        method: "POST".to_owned(),
        path: path.to_owned(),
        request_content_types: vec![JSON_CONTENT_TYPE.to_owned()],
        response_content_types: vec![JSON_CONTENT_TYPE.to_owned()],
    };
    ManagementRegistration {
        routes: vec![
            get("api/snapshot"),
            post("api/settings"),
            post("api/refresh-account"),
            post("api/clear-events"),
        ],
        resources: ["web/index.html", "web/app.js", "web/app.css"]
            .into_iter()
            .map(|path| ManagementResource {
                path: path.to_owned(),
                public: false,
            })
            .collect(),
        pages: vec![ManagementPage {
            id: "quota-sync".to_owned(),
            title: "额度联动".to_owned(),
            description: Some("同步 OpenAI 周额度重置到关联 Client Key".to_owned()),
            entry: "web/index.html".to_owned(),
            icon: None,
        }],
        callbacks: Vec::new(),
    }
}

pub async fn handle(
    config: &Config,
    call: TypedCall<gateway_plugin_sdk::call::management::ManagementRequest>,
) -> Result<TypedReply<ManagementResponse>, PluginFault> {
    route(config, call).await.or_else(ApiError::into_reply)
}

async fn route(
    config: &Config,
    call: TypedCall<gateway_plugin_sdk::call::management::ManagementRequest>,
) -> ApiResult {
    if !call.request.query.is_empty() {
        return Err(ApiError::invalid("此接口不支持查询参数"));
    }
    if call.payload.len() > MAXIMUM_BODY_BYTES {
        return Err(ApiError::invalid("请求正文超过插件限制"));
    }
    match (call.request.method.as_str(), call.request.path.as_str()) {
        ("GET", "api/snapshot") => snapshot(config, call).await,
        ("POST", "api/settings") => save_settings(config, call).await,
        ("POST", "api/refresh-account") => refresh_account(call).await,
        ("POST", "api/clear-events") => clear_events(call).await,
        _ => Err(ApiError::new(404, "not_found", "未找到插件管理接口")),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotResponse {
    settings: ManagedSettings,
    weekly_window_seconds: u64,
    accounts: Vec<AccountView>,
    keys: Vec<KeyView>,
    events: Vec<SyncEvent>,
    updated_at_ms: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountView {
    account_id: String,
    enabled: bool,
    group_ids: Vec<String>,
    quota: Option<QuotaFacts>,
    runtime: Option<AccountRuntime>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyView {
    id: String,
    name: String,
    enabled: bool,
    group_ids: Option<Vec<String>>,
    budget: Option<KeyBudget>,
}

async fn snapshot(
    config: &Config,
    call: TypedCall<gateway_plugin_sdk::call::management::ManagementRequest>,
) -> ApiResult {
    let runtime = LoadedRuntime::load(&call.host).await.map_err(ApiError::from)?;
    let accounts = list_openai_accounts(&call.host)
        .await
        .map_err(ApiError::from)?;
    let keys = list_keys(&call.host).await.map_err(ApiError::from)?;

    let mut account_views = Vec::with_capacity(accounts.len());
    for account in accounts {
        let quota = call
            .host
            .quota_facts(QuotaFactsQuery {
                account_id: account.account_id.clone(),
            })
            .await
            .ok();
        let state = runtime
            .value
            .accounts
            .iter()
            .find(|state| state.account_id == account.account_id)
            .cloned();
        account_views.push(AccountView {
            account_id: account.account_id,
            enabled: account.enabled,
            group_ids: account.group_ids,
            quota,
            runtime: state,
        });
    }

    json_reply(&SnapshotResponse {
        settings: runtime.value.settings.clone(),
        weekly_window_seconds: config.weekly_window_seconds,
        accounts: account_views,
        keys,
        events: runtime.value.events.clone(),
        updated_at_ms: runtime.value.updated_at_ms,
    })
}

async fn save_settings(
    config: &Config,
    call: TypedCall<gateway_plugin_sdk::call::management::ManagementRequest>,
) -> ApiResult {
    let settings: ManagedSettings =
        serde_json::from_slice(&call.payload).map_err(|_| ApiError::invalid("设置不是合法 JSON"))?;
    validate_mappings(&settings.mappings).map_err(ApiError::invalid)?;

    let accounts = list_openai_accounts(&call.host)
        .await
        .map_err(ApiError::from)?
        .into_iter()
        .map(|account| (account.account_id.clone(), account))
        .collect::<BTreeMap<_, _>>();
    let keys = list_key_identities(&call.host)
        .await
        .map_err(ApiError::from)?;
    let key_ids = keys
        .iter()
        .map(|key| key.id.as_str())
        .collect::<BTreeSet<_>>();

    for mapping in &settings.mappings {
        let Some(account) = accounts.get(&mapping.account_id) else {
            return Err(ApiError::invalid(format!(
                "OpenAI 账号不存在：{}",
                mapping.account_id
            )));
        };

        for key_id in &mapping.client_key_ids {
            if !key_ids.contains(key_id.as_str()) {
                return Err(ApiError::invalid(format!("Client Key 不存在：{key_id}")));
            }
            let facts = call
                .host
                .key_facts(ClientKeyFactsQuery {
                    client_key_id: key_id.clone(),
                })
                .await
                .map_err(ApiError::from)?;
            if !key_scope_allows_account(&facts.group_ids, &account.group_ids) {
                return Err(ApiError::invalid(format!(
                    "Client Key {key_id} 当前账号组范围不包含账号 {}",
                    mapping.account_id
                )));
            }
        }

        if let Some(window_key) = mapping.quota_window_key.as_deref()
            && let Ok(quota) = call
                .host
                .quota_facts(QuotaFactsQuery {
                    account_id: mapping.account_id.clone(),
                })
                .await
            && !quota.windows.iter().any(|window| {
                window.key == window_key
                    && window.window_seconds == Some(config.weekly_window_seconds)
            })
        {
            return Err(ApiError::invalid(format!(
                "账号 {} 当前不存在指定周窗口：{window_key}",
                mapping.account_id
            )));
        }
    }

    let mut runtime = LoadedRuntime::load(&call.host).await.map_err(ApiError::from)?;
    runtime.value.settings = settings.clone();
    runtime.value.updated_at_ms = now_ms();
    runtime
        .save(&call.host)
        .await
        .map_err(ApiError::from)?;
    json_reply(&settings)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RefreshAccountRequest {
    account_id: String,
}

async fn refresh_account(
    call: TypedCall<gateway_plugin_sdk::call::management::ManagementRequest>,
) -> ApiResult {
    let request: RefreshAccountRequest = serde_json::from_slice(&call.payload)
        .map_err(|_| ApiError::invalid("刷新参数不是合法 JSON"))?;
    let accounts = list_openai_accounts(&call.host)
        .await
        .map_err(ApiError::from)?;
    if !accounts
        .iter()
        .any(|account| account.account_id == request.account_id)
    {
        return Err(ApiError::invalid("指定的 OpenAI 账号不存在"));
    }
    let quota = call
        .host
        .refresh_account_quota(QuotaFactsQuery {
            account_id: request.account_id,
        })
        .await
        .map_err(ApiError::from)?;
    json_reply(&quota)
}

async fn clear_events(
    call: TypedCall<gateway_plugin_sdk::call::management::ManagementRequest>,
) -> ApiResult {
    if !call.payload.is_empty() {
        return Err(ApiError::invalid("清空事件不接受请求正文"));
    }
    let mut runtime = LoadedRuntime::load(&call.host).await.map_err(ApiError::from)?;
    runtime.value.events.clear();
    runtime.value.updated_at_ms = now_ms();
    runtime
        .save(&call.host)
        .await
        .map_err(ApiError::from)?;
    json_reply(&json!({ "cleared": true }))
}

async fn list_openai_accounts(host: &HostClient) -> Result<Vec<AccountFacts>, PluginFault> {
    let mut cursor = None;
    let mut accounts = Vec::new();
    loop {
        let page = host
            .account_facts(AccountFactsQuery {
                provider_id: Some("openai".to_owned()),
                cursor,
                limit: 200,
            })
            .await?;
        accounts.extend(page.accounts);
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = Some(next);
    }
    Ok(accounts)
}

async fn list_key_identities(host: &HostClient) -> Result<Vec<ClientKey>, PluginFault> {
    let mut cursor = None;
    let mut keys = Vec::new();
    loop {
        let page = host
            .list_keys(KeyListRequest { cursor, limit: 200 })
            .await?;
        keys.extend(page.keys);
        if keys.len() >= MAXIMUM_KEYS {
            keys.truncate(MAXIMUM_KEYS);
            break;
        }
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = Some(next);
    }
    Ok(keys)
}

async fn list_keys(host: &HostClient) -> Result<Vec<KeyView>, PluginFault> {
    let keys = list_key_identities(host).await?;
    let mut views = Vec::with_capacity(keys.len());
    for key in keys {
        let facts = host
            .key_facts(ClientKeyFactsQuery {
                client_key_id: key.id.clone(),
            })
            .await
            .ok();
        let budget = host
            .get_key_budget(gateway_plugin_sdk::call::key_budgets::GetKeyBudgetRequest {
                client_key_id: key.id.clone(),
            })
            .await
            .ok();
        views.push(KeyView {
            id: key.id,
            name: key.name,
            enabled: facts.as_ref().map_or(key.enabled, |facts| facts.enabled),
            group_ids: facts.map(|facts| facts.group_ids),
            budget,
        });
    }
    Ok(views)
}

struct ApiError {
    status: u16,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(400, "invalid_request", message)
    }

    fn into_reply(self) -> Result<TypedReply<ManagementResponse>, PluginFault> {
        encode(
            self.status,
            &json!({ "error": { "code": self.code, "message": self.message } }),
        )
    }
}

impl From<PluginFault> for ApiError {
    fn from(error: PluginFault) -> Self {
        if error.code == ErrorCode::Conflict {
            Self::new(409, "conflict", "状态已变化，请重新加载后再保存")
        } else {
            Self::new(502, "host_callback", "宿主操作未完成")
        }
    }
}

type ApiResult = Result<TypedReply<ManagementResponse>, ApiError>;

fn json_reply(body: &impl Serialize) -> ApiResult {
    encode(200, body).map_err(|_| ApiError::new(500, "encoding", "管理接口响应编码失败"))
}

fn encode(
    status: u16,
    body: &impl Serialize,
) -> Result<TypedReply<ManagementResponse>, PluginFault> {
    let payload = serde_json::to_vec(body)
        .map_err(|_| PluginFault::new(ErrorCode::Fault, "管理接口响应编码失败"))?;
    Ok(TypedReply::new(ManagementResponse {
        status,
        content_type: JSON_CONTENT_TYPE.to_owned(),
    })
    .with_payload(payload))
}

fn now_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u128, |duration| duration.as_millis());
    i64::try_from(millis).unwrap_or(i64::MAX)
}
