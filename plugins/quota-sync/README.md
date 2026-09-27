# 额度联动（quota-sync）

监控指定 OpenAI 账号的**周额度窗口**。当插件确认账号进入新的周额度周期，或检测到明显的提前额度恢复时，同步清零明确关联 Client Key 的周额度已用金额。

插件要求 **codex-proxy-rs >= 3.17.0**。预算管理、主动额度观测和 Key 分组事实均已进入正式版；SDK 依赖按仓库策略固定到上游最新兼容 `main` commit，并由 `Sync Plugin SDK` 自动更新。

## 功能

- 从管理页面选择需要监控的 OpenAI 账号，并为每个账号勾选一个或多个 Client Key。
- 一个 Client Key 只能关联一个监控账号，避免多个上游账号分别重置时重复补额度。
- 保存映射时使用 `key_facts` 校验 Key 当前账号组范围；真正执行 reset 前再次复核，管理员后续修改分组不会导致误清。
- 默认开启 **Dry Run**，用于先观察实际重置识别结果。
- 宿主维护任务约每 30 秒执行一次；普通情况下已有 quota 观测超过默认 5 分钟时主动刷新。
- 发现 reset 候选后，默认每 60 秒尝试取得第二个新 quota 样本，不需要再等完整的 5 分钟刷新周期。
- 第一次看到有效周额度只建立 baseline，不触发同步。
- 正常周窗口变化和明显提前恢复都需要第二个更新样本确认，减少瞬时 quota 抖动造成误清零。
- 确认后调用 `HostClient::reset_key_budget(..., Weekly)` 清零关联 Key 的周已用金额。
- reset 事件会**先持久化再执行写操作**；即使插件在写入后、结果落盘前退出，下一轮也不会自动重复清零。
- SDK 明确说明 timeout / 断连不能证明写入未提交，因此 `failed_unknown` 和残留 `prepared` 事件只提示人工确认，不自动重试。
- 最近 64 个同步事件保存在插件私有状态中，可从管理页查看或清空。

## 管理页面

安装并启用插件后，从宿主「扩展页」打开「额度联动」：

1. 左侧选择 OpenAI 账号。
2. 右侧勾选要关联的 Client Key。
3. 页面显示 Key 当前周预算、账号组范围，以及是否能路由到所选账号。
4. 如果同一账号出现多个 7 天窗口，选择明确的 quota window。
5. 保持 Dry Run 开启观察一段时间；确认识别符合预期后再关闭。
6. 点击「保存映射」。映射保存在插件私有状态，不需要重新编辑插件实例配置。
7. 「刷新所选额度」通过宿主原生 Provider 管理路径立即刷新一次额度观测。
8. 最近同步事件会展示每个 Key 的独立处理结果；`prepared` / `failed_unknown` 需要人工核对，不自动补偿执行。

页面只接收账号 ID、非秘密 Key 身份、账号组事实和预算投影，不读取 OpenAI 凭据或 Client Key 明文。管理页使用宿主当前支持的经典脚本加载方式，不使用 ES Module；同时提供移动端响应式布局，窄屏下账号、Key、操作按钮、窗口选择器和事件记录都会切换为单列并避免横向溢出。

## 权限

| 权限 | 用途 |
| --- | --- |
| `data` | 枚举 OpenAI 账号、读取账号组与 Key 分组范围、读取已有额度观测 |
| `quota_observations` | 主动刷新账号额度观测 |
| `key_budgets` | 查询 Client Key 日/周预算，并重置 weekly 已用金额 |

`key_budgets` 可以管理全部当前及未来 Client Key 的预算，包括管理员或其他插件创建的 Key，但不授予 Key 明文、名称/分组修改、RPM、并发或模型执行权限。账号到 Key 的业务关联完全由本插件维护，宿主不会自动建立关系。

## 映射安全规则

- 同一个 Client Key 不能同时关联多个监控账号。
- Key 有显式账号组绑定时，所选账号必须至少共享一个分组。
- Key 没有显式分组时宿主语义是 `AllAccounts`，插件允许手工关联，但管理页会明确标记；此时应确保实际路由策略确实让这个 Key 对应所选账号。
- reset 前再次读取 `key_facts`。范围已经失配时记为 `scope_mismatch` 并跳过写操作。
- 无法读取 Key facts 时记为 `facts_unavailable`，不执行 reset。
- reset 调用本身返回错误时记为 `failed_unknown`，因为无法安全断言宿主未提交写入。

## 检测参数

插件设置页中的配置项主标签使用中文 `title`，字段 key 仍保持稳定的英文机器名，说明文字继续显示为中文帮助信息。

| 字段 | 默认值 | 含义 |
| --- | ---: | --- |
| `weeklyWindowSeconds` | `604800` | 周窗口长度 |
| `quotaRefreshIntervalSeconds` | `300` | 普通状态下已有观测超过多久主动刷新 |
| `confirmationRefreshIntervalSeconds` | `60` | reset 候选期间取得第二样本的最短刷新间隔 |
| `maxObservationAgeSeconds` | `1800` | 超过该时间的观测不参与判断 |
| `boundaryGraceSeconds` | `300` | 正常重置边界容差 |
| `earlyResetDropPercent` | `50` | 提前恢复候选所需的最低下降百分点 |
| `postResetMaxUsedPercent` | `25` | 候选重置后的最高已用比例 |
| `confirmationGrowthPercent` | `20` | 第二确认样本允许增长的百分点 |

## 事件结果

事件级 `outcome`：

- `dry_run`：只记录，不执行 Key 写入。
- `completed`：全部目标 Key 已确认 reset 成功。
- `partial_failure`：至少一个 Key 被安全跳过、读取失败或写入结果未知。
- `prepared`：reset 意图已经持久化但流程没有完成；通常表示进程中断，需要人工核对宿主实际预算状态。

Key 级状态：

- `reset`：宿主确认 weekly reset 成功。
- `scope_mismatch`：Key 当前路由范围已不包含目标账号，未执行写操作。
- `facts_unavailable`：无法安全复核 Key 范围，未执行写操作。
- `failed_unknown`：reset 回调失败，写入是否已提交未知，不自动重试。
- `pending`：事件已持久化但该 Key 尚未得到最终处理结果。
- `dry_run`：Dry Run 模式。

## 当前范围

- 目前只同步周额度；5 小时窗口暂不纳入。
- OpenAI 账号被停用时保留映射、清除旧检测基线并暂停同步；账号重新启用后从新的 baseline 重新开始监控。
- quota refresh 只是刷新观测，不修改 OpenAI 额度，也不会消费上游额度重置券。
- `reset_key_budget(Weekly)` 只清零已用金额，不会移动 Client Key 自己的 weekly 到期时间；管理页会同时显示 Key 的本地重置时间。若要求两个系统周期边界严格一一对齐，当前 SDK 还没有修改 Key weekly 边界的接口。
- 一个已确认的 reset event 自动执行一次，不做后台补偿重试。
- 管理员可根据事件记录和宿主预算事实自行决定是否人工重置某个结果未知的 Key。

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

通用打包脚本会自动读取本插件 `backend/Cargo.toml` 中的 SDK commit，并安装匹配版本的 `cpr-plugin`。仓库的 SDK 同步 workflow 会周期性尝试更新到 `zyycn/codex-proxy-rs/main` 最新 commit，只有完整插件检查通过才会提交到 `main`。
