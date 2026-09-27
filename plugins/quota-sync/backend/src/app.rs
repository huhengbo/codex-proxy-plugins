use std::sync::Arc;

use gateway_plugin_sdk::client::{AuthorError, ComposedPlugin, PluginBuilder, methods};

use crate::{Config, management, reconcile};

/// 使用公开 SDK 组装额度同步维护与管理处理器。
///
/// # Errors
///
/// 清单与处理器声明不一致时返回错误。
pub fn plugin(config: Config) -> Result<ComposedPlugin, AuthorError> {
    let config = Arc::new(config);
    let management_config = Arc::clone(&config);
    let reconcile_config = Arc::clone(&config);

    PluginBuilder::from_json(include_bytes!("../../plugin.json"))?
        .management(management::registration(), move |call| {
            let config = Arc::clone(&management_config);
            async move { management::handle(&config, call).await }
        })?
        .on(methods::RECONCILE, move |call| {
            let config = Arc::clone(&reconcile_config);
            async move { reconcile::reconcile(&config, call).await }
        })?
        .build()
}
