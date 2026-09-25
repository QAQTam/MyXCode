# HANDOFF — MyCode v2 迁移工作

更新时间：2026-09-25

当前分支：`myXCode-features-v2`

设计文档：`docs/mycode-wire-adapter-spec.md`

---

## 0. 一句话状态

`myXCode-features-v2` 是**基于上游 `main` 直接重放**的新一代 fork 分支，取代旧的
`myXCode-features`（cherry-pick 谱系）。它已经包含：

- provider wire 适配（Chat / ResponsesNative / ResponsesFunctionOnly）
- rwe 文件工具（`read_file` / `edit_file` / `write_file`）
- 商业化遥测阻断（egress 层）
- fork feature 默认值
- fork 版本号 `26.157.0`

当前剩余工作：

- Anthropic Messages（明确暂缓）；
- 3 个 URL 字符串仍在二进制里（见第 6 节，**已决定暂不处理**）；
- 最终 workspace-wide `just test` 验证。

---

## 1. 为什么有 v2

旧分支 `myXCode-features` 用 **cherry-pick** 同步上游，导致：

- merge-base 停在 `5c5308fc9`（2026-09-20），分支永远"落后"；
- 每次同步要重放 277 个提交，撞上大量上游重构；
- 实测 merge 探测冲突 **123 个文件**。

v2 改为**基于上游 HEAD 重放 fork 自己的改动**，于是：

- 分支是 upstream 的**后代**，落后 0；
- 未来同步 = 普通 `merge` / `rebase`，不再是 cherry-pick 马拉松；
- fork 的全部改动收敛成 **3 层 patch**（第 4 节）。

---

## 2. 基线

```text
75e0e0aad97a86138b8b1ec87d9b544b4a35ecbf
2026-09-25T01:52:30Z
Prepare MCP calls directly from advertised tool identities (#47981)
```

> 上游 `main` 移动很快（实测 76 分钟推进 1 个提交）。**任何时候都以 SHA 为准，
> 不要用 `main`。**

**关于"哪个 commit 算稳定"的结论**（已核查，不要再重复调查）：

- 上游**没有**可用于对齐的 release tag（最近的 tag 是 2026-09-10，rust 系列停在 6 月）；
- 上游 `codex-rs/Cargo.toml` 的版本是占位 `0.0.0`，版本号在 release 时注入，**不在 main 里**；
- 上游 main 的 post-merge CI（`blocking-ci` / `postmerge-ci`）**长期红着**——最近 100 次
  main push 里 `blocking-ci` 0 成功，最后一次 success 是 2026-07-22。
  因此 **CI 颜色不能用来选基线**。

---

## 3. 当前分支状态

```text
领先 upstream/main: 10 个提交
落后 upstream/main: 0
```

提交（新 → 旧）：

```text
52c38b319 feat(mycode): enable advanced fork feature defaults
1eb3763ee chore(mycode): extend patch series with the telemetry block layer
4749c4ac5 test(mycode): adapt feedback tests to the blocked upload path
6a57c67d9 feat(mycode): block commercial telemetry at its egress points
f3d751e18 fix(mycode): restore codex-analytics test compilation
34d4c62d2 feat(mycode): add fork telemetry policy crate
6c2910c7c chore(mycode): refresh patch series for the version bump
4ccacf79b chore(mycode): bump fork version to 26.157.0
e20650d66 chore(mycode): add layered patch series for the fork delta
3a7bf9658 feat(mycode): replay wire adapter and file tools onto upstream main
```

---

## 4. Patch 系列

位于 `patches/mycode/`，用法与分层说明见该目录的 `README.md`。

| 层 | 内容 | 文件数 | 冲突风险 |
| --- | --- | --- | --- |
| `0001-mycode-crates.patch` | 新增 crate / 新模块 / 设计文档 / 验证脚本 | 34 | **零**（上游无这些路径） |
| `0002-mycode-seam.patch` | wire 适配 + rwe 文件工具 + 版本号（改上游文件） | 86 | **高**（seam 全在这层） |
| `0003-mycode-commercial.patch` | 遥测阻断（改上游文件） | 22 | 中 |

```bash
git checkout <new-upstream-main>
git apply -3 patches/mycode/0001-mycode-crates.patch
git apply -3 patches/mycode/0002-mycode-seam.patch
git apply -3 patches/mycode/0003-mycode-commercial.patch
cd codex-rs && cargo metadata --format-version 1 >/dev/null   # 刷新 Cargo.lock
just write-config-schema
just write-app-server-schema && just write-app-server-schema --experimental
```

**已验证**：三个 patch 依次打到 `75e0e0aad` 上，得到的 tree 与分支 HEAD 完全一致
（差异仅限 patch 文件自身，生成时有意排除以避免自引用）。

### 4.1 为什么分层

实测（最近 1000 个 main 提交）：

| 组 | 上游触碰频率 |
| --- | --- |
| wire/tools seam（8 个热点文件） | 5.2% |
| 商业化模块整块 | 5.9% |

两者量级相同，所以真正要防的不是频率，而是**冲突类型**：

- 修改型冲突 → `git rebase` 写冲突标记，可解
- **删除型冲突**（你删 / 上游改）→ modify/delete，无法自动合并

因此**不要在上游文件里删代码**，改成 `const` 早返回 + `const if`，让编译器消除死分支。

### 4.2 真正的 seam

`0002` 的 86 个文件里，**85% 的改动集中在 8 个文件**：

```text
core/src/client.rs                     +323/-24   ← 占整个 seam 的 45%
model-provider/src/provider.rs         +130
model-provider-info/src/lib.rs          +98
core/src/safety.rs                      +36/-3
tui/src/diff_render.rs                  +31/-6
core/src/tools/events.rs                +22/-2
codex-api/src/endpoint/responses.rs     +12
tui/src/chatwidget/tool_lifecycle.rs    +10/-2
```

其余 30 个文件都只有 1~9 行。**优化 seam 就先动 `client.rs`**——把更多逻辑抽到
`ext/mycode/model-wire` 或新的 `core/src/mycode/` 模块，可以把最高风险面砍掉近一半。

---

## 5. 架构

### 5.1 纯翻译 crate

```text
codex-rs/ext/mycode/model-wire/       crate: codex-mycode-model-wire
```

- `CanonicalRequest` / `CanonicalItem` / `CanonicalToolCall` / `CanonicalToolResult` / `CanonicalEvent`
- `WireCapabilities` / `WireAdapter` / `ToolPlan`
- namespace flattening、可逆工具身份映射、Chat request 构造
- **不依赖** core / session / auth / HTTP / TUI

### 5.2 Chat transport adapter

```text
codex-rs/ext/mycode/chat-adapter/     crate: codex-mycode-chat-adapter
```

- POST `chat/completions`，复用 `codex_api::EndpointSession`（auth / retry / telemetry）
- Chat SSE 解析、tool-call argument delta 合并、归一化为 `ResponseEvent`
- `[DONE]` 处理、context/quota/overload 错误归一化

### 5.3 文件工具

```text
codex-rs/ext/mycode/file-tools/       crate: codex-mycode-file-tools   （纯逻辑，不碰 std::fs）
codex-rs/core/src/tools/handlers/file_tools/                            （core 薄适配）
```

- `read_file` 走 sandboxed `ExecutorFileSystem` streaming read
- `edit_file` / `write_file` 走 `FileMutationRuntime` → `ToolOrchestrator` + `ApprovalAction::ApplyPatch`
- sandbox denial 映射为 `SandboxErr::Denied`，可正常 escalation 重试
- `codex-apply-patch` 仅最小公开 `seek_sequence` 模块与 `AppliedPatchDelta::new`

### 5.4 Provider transport 边界

```text
codex-rs/codex-api/src/transport.rs
codex-rs/codex-api/src/endpoint/responses.rs
codex-rs/model-provider/src/provider.rs
```

配置（第一类配置层，legacy header 保留为兼容 alias）：

```toml
[model_providers.example.extensions]
wire_adapter = "responses_function_only"
```

**不要恢复** `wire_api = "chat"`。

### 5.5 历史不变量

- 内部历史永远是 canonical `ResponseItem`
- wire-specific shape 不进入持久历史
- 跨 wire 切换保持 message order
- 保持 `FunctionCall.call_id` / `FunctionCallOutput.call_id`
- namespace 身份必须可逆
- 不支持的工具有明确 fail-closed 行为

---

## 6. 商业化遥测阻断

### 6.1 设计

阻断点全部收敛到 fork 自持的 `codex-rs/ext/mycode/policy`：

```rust
pub const BLOCK_COMMERCIAL_TELEMETRY: bool = true;
```

调用点用 **`const` 早返回** 或 **`const if`**，由编译器消除死分支。选这个方案而不是
`--cfg` / `RUSTFLAGS` 的原因：

- 不需要动构建系统，**Cargo 与 Bazel 两条发布路径同时生效**；
- 不会覆盖 `codex-rs/.cargo/config.toml` 里 Windows 专用的
  `-C link-arg=/STACK:8388608` 等 `rustflags`；
- 上游源码原样保留，**cherry-pick / rebase 继续可用**；
- 本地 `just test` 与上游测试语义不变（除下面列出的适配）。

已验证：跨 crate 的 `const bool` 在 **debug 构建**下也会消除死分支里的字面量。

### 6.2 已阻断

| 项 | 位置 |
| --- | --- |
| Statsig 指标端点 + 硬编码 API key | `otel/src/config.rs` |
| `otel.metrics_exporter` 默认值 | `core/src/config/otel.rs`、`config/src/types.rs` |
| Sentry DSN + 反馈上传 | `feedback/src/lib.rs`、`report_upload.rs` |
| 更新探测 URL（GitHub / Homebrew / 桌面 CDN） | `tui/src/updates.rs`、`cli/src/doctor/updates.rs` |
| `check_for_update_on_startup` 默认值 | `core/src/config/mod.rs` |
| 公告 tip 抓取 | `tui/src/tooltips.rs` |
| 宠物素材 CDN | `tui/src/pets/asset_pack.rs` |

### 6.3 已知残留：3 个 URL 字符串（**已决定暂不处理**）

```text
/codex/analytics-events/events      analytics/src/client.rs
plugins/export/curated              core-plugins/src/startup_sync.rs
github.com/openai/plugins.git       core-plugins/src/startup_sync.rs
```

这些是 `DEFERRED_PAYLOADS`：**行为上已被默认配置阻断**，只是字面量还编译在二进制里。

**为什么不做**：把它们从二进制里消掉必须让引用变成死代码，而死代码就是行为改变。
实测代价：

| Crate | 失败测试 |
| --- | --- |
| `codex-core --lib`（analytics） | 3 / 8 |
| `codex-core-plugins` | **27 / 462** |
| `codex-app-server`（analytics） | **25 / 30** |

合计 **55+ 个上游测试要改写**，而且这些改写会全部进入 `0003` patch，成为未来每次
同步上游的**永久冲突源**——正好破坏"wire/tools 能一直 patch"这个前提。

**关键事实**：残留的是 **URL/路径，不是凭据**。Statsig API key 与 Sentry DSN 都已消除。
安全敏感部分已经解决。

如果发布流程要求扫描零告警，再作为**独立的最上层 patch（`0004`）**做，即使它冲突严重
也不影响 `0001`–`0003` 的稳定性。

### 6.4 验证脚本

```bash
python3 scripts/mycode-verify-telemetry-blocked.py <binary> [<binary> ...]
```

- `BLOCKED_PAYLOADS` 命中即退出码 1，**应接进发布流程**；
- `DEFERRED_PAYLOADS` 只告警。

建议在 `scripts/build_codex_package.py` 打包后、以及 Bazel
`//codex-rs/cli:release_binaries` 之后各跑一次。

---

## 7. Fork feature 默认值

`codex-rs/features/src/lib.rs` 里有：

```rust
const FORK_DEFAULT_FEATURES: &[Feature] = &[
    Feature::CodeMode,
    Feature::ApplyPatchStreamingEvents,
    Feature::DefaultModeRequestUserInput,
    Feature::MultiAgentV2,
    Feature::AgentMessageBoard,
    Feature::SendMessageToUserAsync,
];
```

`Feature::default_enabled()` 返回 `spec.default_enabled || FORK_DEFAULT_FEATURES.contains(&self)`。

另外 `update_plan` 工具默认开启：

- `config/src/config_toml.rs`：`UpdatePlanToolConfig.enabled` 用 `default_true`
- `core/src/config/mod.rs`：`resolve_update_plan_enabled` 用 `is_none_or`

**新增 fork 默认值就加进 `FORK_DEFAULT_FEATURES`**，不要改各处 feature 检查。

⚠️ 注意上游把 feature 指标发射从 `features/src/lib.rs` **移到了**
`core/src/config/metrics.rs`。如果以后再有涉及 `default_enabled` 的改动，
记得改**新位置**。

---

## 8. 验证现状

### 8.1 二进制

```text
$ cargo build -p codex-cli --bin codex        # debug
$ python3 scripts/mycode-verify-telemetry-blocked.py codex-rs/target/debug/codex
ok: 1 binary/binaries carry no blocked telemetry payloads
note: 3 deferred payload(s) are still compiled in

$ ./codex-rs/target/debug/codex --version
codex-cli 26.157.0
```

### 8.2 测试（全部通过）

```text
just test -p codex-mycode-model-wire -p codex-mycode-file-tools      33 passed
just test -p codex-model-provider-info -p codex-model-provider -p codex-apply-patch   242 passed
just test -p codex-config                                            345 passed
just test -p codex-core-plugins                                      462 passed
just test -p codex-mycode-policy -p codex-otel -p codex-feedback -p codex-analytics   225 passed, 1 skipped
just test -p codex-core --lib responses_function_only_                 3 passed
just test -p codex-core --lib chat_completions_adapter_                2 passed
just test -p codex-core --lib file_tools                               3 passed
just test -p codex-core --lib safety                                  15 passed
just test -p codex-core --lib spec_plan                               56 passed
just test -p codex-core --lib client::tests                           35 passed
just test -p codex-core --test all 'multiadapt_conformance::'         10 passed
just test -p codex-app-server feedback                                24 passed
just test -p codex-app-server experimental_feature_list                9 passed
just test -p codex-cli doctor                                        137 passed
just test -p codex-tui pets                                           74 passed
just test -p codex-features                                           43 passed
```

`codex-core --lib` 全量：**2631 passed / 1 failed**。唯一失败是
`session::tests::managed_network_proxy_decider_survives_full_access_start`，
已在**干净基线 `75e0e0aad` 上复现同样错误**（代理返回
`reason: not_allowed_local`，沙箱阻断本地网络），属既有环境失败，与 fork 改动无关。

### 8.3 注意事项

- 机器上同时有其他大型 Rust 构建时，Cargo 会等待 artifact lock；
- 链接超大 core test binary 时曾遇到 `lld` bus error，重跑后通过；
  **不要因为链接器错误改业务代码**；
- 迁移期间不要执行 workspace-wide `cargo test`，除非用户明确要求；
- 磁盘：本仓库 `codex-rs/target` 约 **46G**（debug 全量）。测试前先 `df -h /home`；
- `.snap.new` 会因 multi-agent prompt 注入产生（改动前就存在），确认后可用
  `find codex-rs -name '*.snap.new' -delete` 清理；
- Bazel-only 测试二进制缺失（`could not locate binary "test_stdio_server"`）
  与 nextest 超时（`TMT`）不是业务回归。

---

## 9. 关键文件索引

```text
docs/mycode-wire-adapter-spec.md
patches/mycode/README.md

codex-rs/ext/mycode/model-wire/src/canonical.rs
codex-rs/ext/mycode/model-wire/src/capabilities.rs
codex-rs/ext/mycode/model-wire/src/chat.rs
codex-rs/ext/mycode/model-wire/src/tool_plan.rs
codex-rs/ext/mycode/chat-adapter/src/client.rs
codex-rs/ext/mycode/chat-adapter/src/sse.rs
codex-rs/ext/mycode/file-tools/src/edit.rs
codex-rs/ext/mycode/file-tools/src/read.rs
codex-rs/ext/mycode/file-tools/src/spec.rs
codex-rs/ext/mycode/policy/src/lib.rs

codex-rs/core/src/tools/handlers/file_tools/runtime.rs
codex-rs/core/src/client.rs
codex-rs/core/src/client_tests.rs
codex-rs/core/src/safety.rs
codex-rs/core/src/config/metrics.rs
codex-rs/core/tests/suite/multiadapt_conformance.rs

codex-rs/codex-api/src/transport.rs
codex-rs/model-provider-info/src/lib.rs
codex-rs/model-provider/src/provider.rs
codex-rs/features/src/lib.rs

codex-rs/otel/src/config.rs
codex-rs/feedback/src/lib.rs
codex-rs/tui/src/updates.rs
codex-rs/tui/src/tooltips.rs
codex-rs/tui/src/pets/asset_pack.rs
scripts/mycode-verify-telemetry-blocked.py
```

---

## 10. 迁移纪律

### 10.1 上游 main

v2 是 upstream 的后代，所以同步变成普通操作：

```bash
git fetch upstream main
git rebase upstream/main        # 或 merge，取决于是否已推送
```

**冲突只会出现在 `0002` 的 8 个热点文件里。**

### 10.2 保持可 patch

优先：

- 新 crate
- 新 module
- provider extension
- transport adapter
- 小型 core seam
- `const` 早返回 / `const if`（**不要删上游代码**）

避免：

- 大范围修改上游 struct
- 把 wire-specific 分支塞进 tool router
- 在 core 中复制 Chat / Anthropic message 类型
- 改写上游测试（改写过的测试会持续与上游冲突）

### 10.3 新增 fork 改动的流程

1. 在 v2 上提交改动
2. `just fmt` + 对应 crate 测试
3. 重新生成 patch 系列（见 `patches/mycode/README.md`）
4. **round-trip 校验**：把 patch 打到基线上，比对 tree hash
5. 推送

---

## 11. 交接检查清单

- [x] 当前分支是 `myXCode-features-v2`；
- [x] 基于上游 `main` `75e0e0aad`，落后 0；
- [x] wire 适配重放完成；
- [x] rwe 文件工具重放完成；
- [x] 商业化遥测阻断重放完成；
- [x] fork feature 默认值重放完成；
- [x] 版本号 `26.157.0`；
- [x] 3 层 patch 系列 + round-trip 校验；
- [x] 二进制扫描 0 blocked / 3 deferred；
- [x] 各 crate 测试通过；
- [ ] 最终 workspace-wide `just test` 尚未执行；
- [ ] Anthropic Messages 暂缓，尚未实现；
- [ ] 文件工具独立 `ApprovalAction::FileMutation` 和独立 approval cache key 尚未实现；
- [ ] seam 瘦身（`client.rs` 的 347 行）尚未做；
- [ ] 3 个 deferred URL 字符串已决定暂不处理（见 6.3）。

---

## 12. 旧分支 `myXCode-features`

旧分支的**全部有价值内容已并入 v2**。它现在只是归档：

- 它用 cherry-pick 同步上游，merge-base 停在 `5c5308fc9`，落后 353 / 领先 109；
- 遥测阻断的 4 个提交**只存在于本地**（`origin/myXCode-features` 停在 `4586387c3`，
  远程落后 91 个提交）；
- v2 已包含这些内容，因此旧分支可以安全归档或删除。
