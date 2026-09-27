use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::host::{LogRequest, LogResult},
    client::{HostClient, SessionError},
};
use serde::{Serialize, de::DeserializeOwned};

pub(crate) async fn metadata<I, O>(
    host: &HostClient,
    method: &str,
    input: &I,
) -> Result<(O, Vec<u8>), PluginFault>
where
    I: Serialize,
    O: DeserializeOwned,
{
    let reply = host
        .call(
            method,
            serde_json::to_value(input).map_err(|_| invalid_callback())?,
            Vec::new(),
        )
        .await
        .map_err(SessionError::into_plugin_fault)?;
    let result = serde_json::from_value(reply.result).map_err(|_| invalid_callback())?;
    Ok((result, reply.payload))
}

pub(crate) async fn log(host: &HostClient, request: &LogRequest) -> Result<LogResult, PluginFault> {
    metadata(host, "host.log", request)
        .await
        .map(|(result, _)| result)
}

fn invalid_callback() -> PluginFault {
    PluginFault::new(ErrorCode::Fault, "宿主回调返回了无效响应")
}
