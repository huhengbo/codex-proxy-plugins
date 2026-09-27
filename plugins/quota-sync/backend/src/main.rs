use std::collections::BTreeSet;

use codex_proxy_plugin_quota_sync::{Config, PLUGIN_ID, manifest, plugin};
use gateway_plugin_sdk::client::{PluginSession, SessionConfig, SessionError};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = PluginSession::accept(
        tokio::io::stdin(),
        tokio::io::stdout(),
        SessionConfig::default(),
    )
    .await?;
    let manifest = manifest()?;
    let granted = session
        .handshake()
        .permissions
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if session.handshake().plugin_id != PLUGIN_ID
        || session.handshake().contributes != manifest.contributes
        || granted != manifest.permissions
    {
        return Err(SessionError::Handshake.into());
    }

    let config: Config = serde_json::from_value(session.handshake().configuration.clone())?;
    if let Err(message) = config.validate() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, message).into());
    }
    session.run(plugin(config)?).await?;
    Ok(())
}
