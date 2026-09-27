use std::sync::Arc;

use gateway_plugin_sdk::client::{AuthorError, ComposedPlugin, PluginBuilder, methods};

use crate::{Config, reconcile};

/// 使用公开 SDK 组装额度同步维护处理器。
///
/// # Errors
///
/// 清单与处理器声明不一致时返回错误。
pub fn plugin(config: Config) -> Result<ComposedPlugin, AuthorError> {
    let config = Arc::new(config);
    PluginBuilder::from_json(include_bytes!("../../plugin.json"))?
        .on(methods::RECONCILE, move |call| {
            let config = Arc::clone(&config);
            async move { reconcile::reconcile(&config, call).await }
        })?
        .build()
}
