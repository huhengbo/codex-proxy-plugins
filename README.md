<!-- prettier-ignore -->
<div align="center">

<img src="https://raw.githubusercontent.com/zyycn/codex-proxy-rs/main/frontend/public/favicon.svg" alt="Codex Proxy" width="80" height="80" />

# Huhengbo · Codex Proxy Plugins

基于 zyycn/codex-proxy-plugins 维护的个人 Codex Proxy 插件集合。

[![CI](https://github.com/huhengbo/codex-proxy-plugins/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/huhengbo/codex-proxy-plugins/actions/workflows/ci.yml)
[![Releases](https://img.shields.io/badge/Releases-plugin%20bundles-blue?style=flat-square)](https://github.com/huhengbo/codex-proxy-plugins/releases)

[插件目录](#插件目录) · [新增插件](#新增插件) · [本地检查](#本地检查) · [构建与发布](#构建与发布)

</div>

## 仓库定位

这个仓库可以同时维护多个独立插件。GitHub 仓库不是插件安装单位：每个 plugins/<name>/plugin.json 定义一个独立插件，最终分别打包成自己的 tar.gz 安装包。

examples/workbench 保留上游官方示例，主要用于查阅 SDK 的能力写法；实际维护的插件统一放在 plugins/。

~~~text
codex-proxy-plugins/
├── examples/
│   └── workbench/              上游官方示例，尽量保持原样
├── plugins/
│   ├── quota-sync/             一个独立插件
│   │   ├── plugin.json
│   │   ├── README.md
│   │   ├── backend/
│   │   └── frontend/           可选
│   └── <future-plugin>/        后续插件
├── scripts/
│   ├── check-plugins
│   ├── package-plugin
│   └── package-all
└── .github/workflows/
    ├── ci.yml
    ├── release.yml
    └── sync-sdk.yml
~~~

## 插件目录

| 插件 | 当前版本 | 状态 | 说明 |
| --- | ---: | --- | --- |
| [额度联动 / huhengbo.quota-sync](plugins/quota-sync/README.md) | 0.3.3 | 可用 | 监控 OpenAI 周额度重置，主动刷新额度观测，并同步清零关联 Client Key 的 weekly 已用金额 |

当前 `quota-sync` 最低宿主版本为 `codex-proxy-rs >=3.17.0`，SDK pin 已切到最新主线并由自动同步 workflow 持续维护。

## 多插件约定

每个 plugins/<name>/ 都是独立版本单元：

- 目录名必须与 plugin.json 的 name 一致。
- 插件 ID 为 publisher.name，仓库内不得重复。
- plugin.json.version 必须与 backend/Cargo.toml 的 package.version 一致。
- 每个插件必须提交 backend/Cargo.lock；CI、测试和发布构建统一使用 `--locked`。
- 所有自定义插件统一使用同一个 `gateway-plugin-sdk` commit，并以完整 SHA 固定以保证构建可复现。仓库的 `Sync Plugin SDK` workflow 每 6 小时尝试同步到 `codex-proxy-rs/main` 最新 commit；只有全部插件检查通过才会直接提交到本仓库 `main`，不兼容时保留当前 pin。
- 打包时必须使用与该插件 SDK commit 相同的 cpr-plugin。
- 插件 README 负责说明权限、宿主兼容范围、使用方式和已知限制。
- 前端资源统一使用 web/ 包路径。静态页面可以直接放 frontend/；需要构建的前端在 frontend/package.json 中固定 pnpm 版本并提交 lockfile。

仓库 Release 使用 bundle 版本，不替代每个插件自己的版本。一个 Release 可以同时包含多个插件和多个平台的安装包，因此某个插件升级时不要求其他插件同步改版本。

## 新增插件

新增插件时创建：

~~~text
plugins/<plugin-name>/
├── plugin.json
├── README.md
├── backend/
│   ├── Cargo.toml
│   ├── Cargo.lock
│   └── src/
└── frontend/      可选
~~~

完成后执行：

~~~bash
python3 scripts/plugin-repo.py validate
bash scripts/check-plugin <plugin-name>
~~~

仓库脚本会自动发现 plugins/*/plugin.json。只要遵守目录约定，新增插件通常不需要再修改 CI 或 Release workflow。

## 本地检查

查看已发现插件：

~~~bash
bash scripts/list-plugins
~~~

检查全部自定义插件：

~~~bash
bash scripts/check-plugins
~~~

CI 分成两个 Job：

1. Official example：继续检查上游 examples/workbench，避免 Fork 时把参考实现改坏。
2. Custom plugins：校验所有插件的目录、ID、版本、SDK pin 和 Cargo.lock，再逐插件执行 Rust fmt、clippy、test；存在前端时自动执行对应前端检查，并实际打一个 Linux x86_64 安装包作为 smoke test。

CI 同时检查通用脚本语法。后续新增 plugins/<name>/ 后会自动纳入。日常开发和自动更新均以 `main` 为默认分支。

## SDK 自动同步

仓库会定期尝试把所有自定义插件的 `gateway-plugin-sdk.rev` 更新到 `zyycn/codex-proxy-rs/main` 最新 commit：

~~~text
Sync Plugin SDK
  → 读取上游 main HEAD
  → 更新 plugins/*/backend/Cargo.toml
  → 重新生成 plugins/*/backend/Cargo.lock
  → 运行全部自定义插件检查
  → 全部通过：直接提交到 main
  → 任一失败：恢复旧 pin，不修改 main
~~~

也可以在 GitHub Actions 中手动运行 **Sync Plugin SDK**。本策略追求“最新且可编译”，不会为了追 commit 把主分支更新成不可用状态。

## 构建安装包

构建一个插件：

~~~bash
bash scripts/package-plugin quota-sync
~~~

指定目标平台：

~~~bash
bash scripts/package-plugin quota-sync aarch64-unknown-linux-gnu
~~~

构建全部插件：

~~~bash
bash scripts/package-all
~~~

通用打包脚本会从每个插件自己的 backend/Cargo.toml 读取 gateway-plugin-sdk.rev，并自动安装同 commit 的 cpr-plugin，缓存在：

~~~text
.tools/cpr-plugin-<sdk-commit>/
~~~

CI 会拒绝自定义插件使用不同 SDK commit。打包仍读取各插件 Cargo.toml 中的固定值，但正常情况下这些值必须完全一致；自动同步会统一更新全部插件。

当前打包目标沿用上游支持范围：

- x86_64-unknown-linux-gnu
- aarch64-unknown-linux-gnu
- aarch64-apple-darwin

## 构建与发布

Release Plugins workflow 会自动：

1. 运行完整 CI。
2. 在 Linux x86_64、Linux arm64、macOS arm64 三个平台执行 scripts/package-all。
3. 汇总全部插件生成的 tar.gz 和 sha256。
4. 从当前 plugins/*/plugin.json 自动生成 Release 插件列表。
5. 创建一个仓库级 bundle Release。

Tag 约定：

- bundle-v2026.09.1：稳定 bundle。
- bundle-preview-2026.09.1：预发行 bundle。
- 也可以从 Actions 手动运行 Release Plugins，填写已经存在的 tag，并选择是否标记 prerelease。

quota-sync 所需的 `key_budgets`、`quota_observations` 与 `key_facts` 已进入 codex-proxy-rs v3.17.0；可以按稳定 bundle 发布。`Sync Plugin SDK` 会继续定期追踪上游最新兼容 main。

宿主从 GitHub Release 安装时选择具体插件对应的 tar.gz asset；仓库里有几个插件不影响单个插件的安装和升级。

## Release 与插件版本

插件版本由各自 plugin.json 决定，例如：

~~~text
huhengbo.quota-sync  0.3.3
huhengbo.other       1.4.1
~~~

bundle tag 是仓库发布批次，例如：

~~~text
bundle-preview-2026.09.1
~~~

两者不是同一套版本。Release 可以同时包含不同版本号的多个插件。

## 与上游同步

自己的业务代码只放 plugins/ 和仓库级 scripts / CI。examples/workbench 尽量保持上游原样，这样后续同步官方插件示例时冲突更少。默认开发分支为 main；不再要求通过长期 custom 分支承载自定义插件。

相关资料：

- [Codex Proxy RS](https://github.com/zyycn/codex-proxy-rs)
- [官方插件示例仓库](https://github.com/zyycn/codex-proxy-plugins)
- [插件 SDK 能力说明](https://github.com/zyycn/codex-proxy-rs/blob/main/backend/crates/gateway-plugin/sdk/docs/capabilities.md)
