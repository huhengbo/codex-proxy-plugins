//! OpenAI 周额度重置与下游 Client Key 额度同步插件。

mod app;
mod config;
mod detector;
mod host_calls;
mod management;
mod manifest;
mod reconcile;
mod reset;
mod state;

pub use app::plugin;
pub use config::{AccountMapping, Config};
pub use manifest::{PLUGIN_ID, manifest};
