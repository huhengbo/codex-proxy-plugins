# 额度联动（quota-sync）

监控指定 OpenAI 账号的**周额度窗口**，当检测到正常周期重置或明显的提前额度恢复时，对配置中关联的 Client Key 执行同步重置。

当前版本已经实现检测、二次确认、私有状态持久化、事件去重和 Dry Run。由于宿主插件 SDK 尚未开放“重置已有 Client Key 预算”的受控回调，真正的 Key 重置调用暂时留在 `backend/src/reset.rs` 中；跟踪上游：<https://github.com/zyycn/codex-proxy-rs/issues/299>。

## 工作方式

宿主的 `maintenance` 能力会在实例启用、恢复、配置变化时触发，并每约 30 秒补偿执行一次。插件通过 `data` 权限读取已有额度观测，不主动访问 OpenAI，也不读取账号凭据。

每个账号独立维护：

1. 第一次看到有效周额度时只建立基线，不触发同步。
2. 正常周窗口的 `reset_at` 前进且重置后用量较低时，建立候选事件。
3. 如果 `reset_at` 未变化，但已用比例出现大幅下降，也会建立“提前恢复”候选事件。
4. 候选事件必须由**下一个更新的额度样本再次确认**，降低瞬时数据抖动造成的误触发。
5. 确认后记录事件并处理关联 Key；同一批样本不会重复触发。

配置的账号会通过宿主 `data` 接口再次确认属于 OpenAI。若同一账号同时存在多个 `604800` 秒窗口，插件不会猜测目标窗口，需要为该账号配置 `quotaWindowKey`。

## 配置

建议第一阶段保持 `dryRun: true`，先观察几次真实重置行为。

```json
{
  "dryRun": true,
  "mappings": [
    {
      "accountId": "acct_xxx",
      "clientKeyIds": ["key_xxx", "key_yyy"]
    }
  ]
}
```

高级检测参数都有默认值：

| 字段 | 默认值 | 含义 |
| --- | ---: | --- |
| `weeklyWindowSeconds` | `604800` | 周窗口长度 |
| `maxObservationAgeSeconds` | `1800` | 忽略过旧额度快照 |
| `boundaryGraceSeconds` | `300` | 正常重置边界容差 |
| `earlyResetDropPercent` | `50` | 提前恢复需要的最低下降百分点 |
| `postResetMaxUsedPercent` | `25` | 候选重置后的最高已用比例 |
| `confirmationGrowthPercent` | `20` | 第二个确认样本允许的增长百分点 |

## 当前限制

- 只处理周额度；5 小时窗口暂不纳入。
- `host.data.quota.get` 读取的是宿主已有额度观测，不会主动刷新上游；因此实际检测速度取决于宿主额度快照更新频率。
- 当前 SDK 不能重置管理员已有 Client Key 的额度。`dryRun=false` 时会把事件记录为 `sdk_unavailable`，不会调用 Admin API 或要求管理员 API Key。
- SDK 能力开放后，应只修改 `backend/src/reset.rs`，其余检测和状态机保持不变。

## 本地检查

本插件固定到 codex-proxy-rs v3.16.0 的 SDK commit：

```text
0534dd8f2679e6f2d08a4f6df1b79abc5cbd4716
```

执行：

```bash
cargo fmt --manifest-path plugins/quota-sync/backend/Cargo.toml --check
cargo clippy --manifest-path plugins/quota-sync/backend/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path plugins/quota-sync/backend/Cargo.toml
```

打包时使用同一 commit 的 `cpr-plugin`：

```bash
cargo install --locked --git https://github.com/zyycn/codex-proxy-rs.git \
  --rev 0534dd8f2679e6f2d08a4f6df1b79abc5cbd4716 \
  codex-proxy-plugin-cli --root .tools

PLUGIN_CLI="$PWD/.tools/bin/cpr-plugin" bash scripts/package-quota-sync
```
