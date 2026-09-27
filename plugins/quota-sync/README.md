# 额度联动（quota-sync）

监控指定 OpenAI 账号的周额度窗口。当插件确认账号进入新的周额度周期，或检测到明显的提前额度恢复时，同步清零管理页中关联 Client Key 的周额度已用金额。

当前开发版基于 [zyycn/codex-proxy-rs#300](https://github.com/zyycn/codex-proxy-rs/pull/300) 提供的 key_budgets 与 quota_observations SDK 合同。PR 尚未合并，因此插件需要包含该 PR 能力的宿主构建；上游发布正式版本后再切换到对应 release commit。

## 功能

- 从管理页面选择需要监控的 OpenAI 账号，并为每个账号勾选一个或多个 Client Key。
- 默认开启 Dry Run，用于先观察实际重置识别结果。
- 宿主维护任务约每 30 秒执行一次；已有 quota 观测超过默认 5 分钟时，通过 quota_observations 请求宿主主动刷新。
- 第一次看到有效周额度只建立 baseline，不触发同步。
- 正常周窗口变化和明显提前恢复都需要下一个更新样本再次确认，减少瞬时 quota 抖动造成误清零。
- 确认后通过 HostClient::reset_key_budget(..., Weekly) 清零关联 Key 的周已用金额。
- 每个 Key 的执行结果独立记录。SDK 明确说明 timeout / 断连不能证明写入未提交，因此失败结果不会自动重试，避免下一轮再次清掉新产生的消费。
- 最近 64 个同步事件保存在插件私有状态中。

## 管理页面

安装并启用插件后，从宿主「扩展页」打开「额度联动」：

1. 左侧选择 OpenAI 账号。
2. 右侧勾选要关联的 Client Key。
3. 如果同一账号出现多个 7 天窗口，选择明确的 quota window。
4. 保持 Dry Run 开启观察一段时间；确认识别符合预期后再关闭。
5. 点击「保存映射」。映射保存在插件私有状态，不需要重新编辑插件实例配置。
6. 「刷新所选额度」通过宿主 Provider 管理路径立即刷新一次额度观测。

页面只接收账号 ID、非秘密 Key 身份和预算投影，不读取 OpenAI 凭据或 Client Key 明文。

## 权限

插件声明三个访问域：

| 权限 | 用途 |
| --- | --- |
| data | 枚举 OpenAI 账号并读取基础事实 |
| quota_observations | 读取并主动刷新账号额度观测 |
| key_budgets | 查询 Client Key 日/周预算，并重置 weekly 已用金额 |

其中 key_budgets 按 PR #300 的合同作用于全部当前及未来 Client Key，包括管理员或其他插件创建的 Key，但不授予 Key 明文、名称、分组、RPM、并发或模型执行权限。账号到 Key 的关联完全由本插件维护，宿主不会自动建立关系。

## 检测参数

插件实例配置只保存检测阈值；账号映射与 Dry Run 在管理页维护。

| 字段 | 默认值 | 含义 |
| --- | ---: | --- |
| weeklyWindowSeconds | 604800 | 周窗口长度 |
| quotaRefreshIntervalSeconds | 300 | 已有观测超过多久时主动刷新 |
| maxObservationAgeSeconds | 1800 | 超过该时间的观测不参与判断 |
| boundaryGraceSeconds | 300 | 正常重置边界容差 |
| earlyResetDropPercent | 50 | 提前恢复候选所需的最低下降百分点 |
| postResetMaxUsedPercent | 25 | 候选重置后的最高已用比例 |
| confirmationGrowthPercent | 20 | 第二确认样本允许增长的百分点 |

## 当前范围

- 只处理周额度，5 小时窗口暂不纳入。
- 上游 quota refresh 只是刷新观测，不修改 OpenAI 额度，也不会消费上游重置券。
- 一个已确认的 reset event 只执行一次。某个 Key 返回失败或结果未知时，只记录事件，由管理员检查后决定是否人工处理，不做自动补偿重试。
- PR #300 尚未合并；当前 SDK 暂时固定到其 head commit `6311dab04166e581c9c44630320e29255bd00354`。仓库会自动尝试同步到上游最新 `main`，但只有包含本插件所需预算/额度接口并通过完整检查时才会更新。

## 检查与打包

从仓库根目录执行：

~~~bash
bash scripts/check-plugin quota-sync
bash scripts/package-plugin quota-sync
~~~

构建全部自定义插件：

~~~bash
bash scripts/package-all
~~~

通用打包脚本会自动读取本插件 backend/Cargo.toml 中的 SDK commit，并安装匹配版本的 cpr-plugin，不需要手工维护另一份 CLI commit。

当前插件依赖 PR #300，因此发布时使用仓库的 bundle-preview-* 预发行 tag。PR #300 正式合并并进入宿主 release 后，再把 SDK pin 和兼容说明切换到正式版本。
