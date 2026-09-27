<!-- prettier-ignore -->
<div align="center">

<img src="https://raw.githubusercontent.com/zyycn/codex-proxy-rs/main/frontend/public/favicon.svg" alt="Codex Proxy" width="80" height="80" />

# Codex Proxy Plugins

通过可运行的示例，开发你的 Codex Proxy 插件

[![CI](https://github.com/zyycn/codex-proxy-plugins/actions/workflows/ci.yml/badge.svg)](https://github.com/zyycn/codex-proxy-plugins/actions/workflows/ci.yml)
[![插件下载](https://img.shields.io/badge/下载-插件安装包-blue?style=flat-square)](https://github.com/zyycn/codex-proxy-plugins/releases)

[体验插件](#体验插件) · [本地开发](#本地开发) · [构建安装包](#构建安装包) · [文档](#文档)

</div>

[Codex Proxy RS](https://github.com/zyycn/codex-proxy-rs) 的插件示例仓库。后端使用公开 Rust SDK，页面使用宿主桥与 `@codex-proxy/ui`，可以独立构建和发布。

## 自定义插件

本 Fork 在官方示例之外维护实际使用插件：

- [**额度联动（quota-sync）**](plugins/quota-sync/README.md)：监控指定 OpenAI 账号的周额度窗口，识别正常重置和提前额度恢复，并为关联 Client Key 预留周额度同步重置。当前默认 Dry Run，实际 Key 重置等待上游 SDK 能力 [zyycn/codex-proxy-rs#299](https://github.com/zyycn/codex-proxy-rs/issues/299)。
- 官方 `examples/workbench` 保持原样，继续作为能力示例；`quota-sync` 单独固定到 codex-proxy-rs v3.16.0 对应 SDK commit，避免为了新能力直接追踪上游 `main`。

目前提供一个完整示例：[**插件工作台**](examples/workbench/README.md)。你可以先体验功能，再按需要阅读对应处理器：

| 功能 | 可以学到什么 |
| --- | --- |
| 基础示例 | 文本大写转换、模型路由、账号调度、请求与用量观察、管理接口 |
| 接入指南 | 终端命令、自定义客户端认证 |
| 文本工作台 | 摘要、翻译、改写、网页取文、流式生成、取消、继续调整与保存记录 |

## 体验插件

1. 从 [Releases](https://github.com/zyycn/codex-proxy-plugins/releases) 下载与**宿主运行平台**匹配的 `.tar.gz`，同时下载 `.sha256` 核对摘要。
2. 在宿主「插件管理」中上传安装包，查看声明的权限并安装。
3. 启用自动准备的默认配置，从「扩展页」打开「插件工作台」。模型示例需要选择已有的可用 Key 和模型。

> [!IMPORTANT]
> 插件接口仍处于实验阶段，发行包标记为 Pre-release。宿主须支持清单 v1、协议 v1，并满足 `>=3.14.0, <4.0.0`；`3.13.1` 不支持安装。通过 GitHub 来源安装时，需要明确填写发行标签并允许预发行。

工作台声明 `network`、`models`、`requests`、`public_endpoints` 四个访问域。插件进程与宿主使用相同系统身份；运行模型示例会产生真实用量。各能力的触发条件见[示例说明](examples/workbench/README.md#能力与边界)。

## 本地开发

准备 **Rust 1.97、Node.js 24、pnpm 12.6**，然后执行：

```bash
git clone https://github.com/zyycn/codex-proxy-plugins.git
cd codex-proxy-plugins
pnpm --dir examples/workbench/frontend install --frozen-lockfile
pnpm --dir examples/workbench/frontend dev
```

独立预览使用模拟宿主，适合阅读页面和调试交互，不调用真实模型。后端由宿主通过标准输入输出启动，实际能力需要构建安装包后验证。

```text
examples/workbench/
├── plugin.json    插件身份、能力、权限与资源声明
├── backend/       Rust 处理器与会话测试
├── frontend/      Vue 页面、宿主桥与独立预览
└── README.md      按能力阅读源码与接口说明
scripts/package   构建并打包
dist/             安装包与校验文件（不入库）
```

从 [`app.rs`](examples/workbench/backend/src/app.rs) 查看处理器如何组合，再按[源码导航](examples/workbench/README.md#从哪里读起)选择需要的能力。开发自己的插件时，只保留需要的处理器及对应清单声明。

SDK 固定到提交 `f770ba127d1293bb482921019e61cae7cb3b7de9`，UI 使用 `v0.3.0`，实际依赖由各自锁文件固定；无需检出宿主或组件库。需要联合修改时，使用宿主的[源码联调入口](https://github.com/zyycn/codex-proxy-rs/blob/main/docs/development.md)，正式构建仍使用锁定依赖。

在本仓库根目录执行检查：

```bash
pnpm --dir examples/workbench/frontend lint
pnpm --dir examples/workbench/frontend build
cargo fmt --manifest-path examples/workbench/backend/Cargo.toml --check
RUST_MIN_STACK=16777216 cargo clippy --manifest-path examples/workbench/backend/Cargo.toml --all-targets --all-features --locked -- -D warnings
RUST_MIN_STACK=16777216 cargo test --manifest-path examples/workbench/backend/Cargo.toml --locked
```

## 构建安装包

安装与 SDK 同一提交的打包工具，在仓库根目录运行：

```bash
cargo install --locked --git https://github.com/zyycn/codex-proxy-rs.git --rev f770ba127d1293bb482921019e61cae7cb3b7de9 codex-proxy-plugin-cli --root .tools
PLUGIN_CLI="$PWD/.tools/bin/cpr-plugin" bash scripts/package
```

脚本构建前后端，在 `dist/` 生成 `.tar.gz` 和 `.sha256`。默认使用本机平台，也可指定目标，例如：

```bash
PLUGIN_CLI="$PWD/.tools/bin/cpr-plugin" bash scripts/package aarch64-unknown-linux-gnu
```

支持 Linux x86_64、Linux aarch64、macOS aarch64；交叉构建需准备对应 Rust target 和链接工具。已安装插件使用包内资源，修改源码后需重新构建、打包并切换版本。

发行时同步 `plugin.json` 的版本和 `release/notes.md`，从 `main` 推送对应的 `v<插件版本>` 标签。[发布工作流](.github/workflows/release.yml)在检查通过后生成三个平台的安装包与校验文件。

## 文档

| 任务 | 入口 |
| --- | --- |
| 阅读示例与管理接口 | [插件工作台](examples/workbench/README.md) |
| 安装、配置与管理版本 | [宿主插件使用说明](https://github.com/zyycn/codex-proxy-rs/blob/main/docs/plugins.md) |
| 编写 Rust 插件 | [SDK](https://github.com/zyycn/codex-proxy-rs/blob/f770ba127d1293bb482921019e61cae7cb3b7de9/backend/crates/gateway-plugin/sdk/README.md) · [清单](https://github.com/zyycn/codex-proxy-rs/blob/f770ba127d1293bb482921019e61cae7cb3b7de9/backend/crates/gateway-plugin/sdk/docs/manifest.md) · [能力合同](https://github.com/zyycn/codex-proxy-rs/blob/f770ba127d1293bb482921019e61cae7cb3b7de9/backend/crates/gateway-plugin/sdk/docs/capabilities.md) |
| 自定义打包流程 | [插件 CLI](https://github.com/zyycn/codex-proxy-rs/blob/f770ba127d1293bb482921019e61cae7cb3b7de9/backend/apps/plugin-cli/README.md) |
| 编写管理页面 | [UI 组件库](https://github.com/zyycn/codex-proxy-ui) · [宿主主题约定](https://github.com/zyycn/codex-proxy-rs/blob/main/docs/theme.md) |
