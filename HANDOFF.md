# HANDOFF — MyCode 迁移工作

更新时间：2026-09-25

当前分支：`myXCode-features`

设计文档：`docs/mycode-wire-adapter-spec.md`

> **迁移完成前，默认不把上游 `main` 直接合入当前分支。**
>
> 允许在 fork owner 明确要求时：
>
> - `git fetch upstream main`
> - fast-forward 本地 `main`
> - 使用临时 worktree 做 cherry-pick/merge 探测
> - 经评估后把必要补丁 cherry-pick 到 `myXCode-features`
>
> 不得直接把 `main` 合入当前分支。当前本地 `main` 已按 owner 要求
> fast-forward 到 `upstream/main = ac7634b9f`（2026-09-22），并已
> cherry-pick 79 个上游补丁（4 个基础补丁 + 75 个 TUI/multi-agent
> 补丁）；当前分支仍未合并上游 `main`。

---

## 0. 一句话状态

Chat、ResponsesNative、ResponsesFunctionOnly 三条 wire 路径已经稳定接入，
并具备 canonical history、ToolPlan、SSE 解析、返回侧工具名反解、跨 wire
conformance 和第一类 provider extension 配置能力。

`edit_file`、`read_file`、`write_file` 已按“纯逻辑 crate + core sandbox
adapter”落地：算法不接触文件系统，core 统一通过 `ExecutorFileSystem`、
`ToolOrchestrator` 和 `FileSystemSandboxContext` 执行。

当前剩余工作：

- Anthropic Messages（明确暂缓）；
- 最终完整 `just test` 验证；
- 后续按需要把 generic Responses 默认策略推广到更多 provider；
- 文件工具仍需独立 `ApprovalAction::FileMutation` 和 approval cache key；
- 上游 130 个提交已完成整体合并探测；当前已同步 79 个补丁，剩余
  提交按需继续 cherry-pick；
- **商业化/遥测“移除代码”轮**：analytics 事件管道、curated 插件仓库
  同步、cloud-config 云开关，以及 `analytics.enabled`/`feedback.enabled`
  默认关闭。详见第 11 节。

---

## 1. 分支与提交

当前分支：

```text
myXCode-features
```

当前 HEAD 附近的迁移/同步提交：

```text
7c5e7b678 feat(mycode): block commercial telemetry at its egress points
cc3f00bc5 fix(mycode): restore codex-analytics test compilation
9824b3263 feat(mycode): add fork telemetry policy crate
8216c959d feat(mycode): enable advanced fork feature defaults
8b14e886f chore(mycode): bump fork version to 26.155.2-beta
b139bcb7f docs(mycode): record TUI and multi-agent sync
0a431f1fb chore(app-server-protocol): refresh stable precomputed exports
abde2862d Stream agent snapshots from local status subscriptions (#47267)
fe200bf2f Subscribe authors to message-board channels created by posting (#47259)
1d62014d7 Preserve explicit message-board unsubscribes when posting (#47257)
cf13c25fa Return agent snapshots from `close_agent` (#47252)
fffee2b4c Enforce current provider requirements for the model catalog (#46917)
9019b6b98 Honor the system clock preference in TUI completion timestamps (#46845)
9e2b43d90 Preserve required Windows runtime variables for filesystem helpers (#47108)
e201138bf Decouple tool restrictions from session identity with `ToolPolicy` (#46999)
eb510d3bc test(app-server): initialize provider extensions
198514604 feat(tui): render file tool verbs
6010c6793 feat(mycode): preserve file change tool names
4586387c3 feat(mycode): add sandboxed file tools
c160a5a2d7 feat(mycode): add provider wire adapter extensions
d7c230d430 test(mycode): add cross-wire conformance coverage
03e64024a9 feat(mycode): decode responses function-only tool calls
119d4c8514 feat(mycode): adapt responses function-only requests
67704621c0 feat(mycode): promote wire adapter to provider policy
c5639d3de6 refactor(mycode): keep chat wire encoding in adapter
```

本轮另外连续 cherry-pick 了 75 个上游提交，覆盖 TUI 产品化
（fullscreen transcript、mouse selection、usage/analytics、math/Mermaid）
以及 multi-agent/message board（`codex-agent-message-board-extension`、
SQLite 持久化、协作工具、agent snapshots/resume/eviction）。组合应用
只人工处理了预计算 schema 二进制，随后已重新生成 schema 并更新
`MODULE.bazel.lock`。

更早的相关提交：

```text
f0013687f2 Honor an environment API key on every entry point
6763c3684b feat(tui): add runtime status line metrics
```

`main` 只在 fork owner 明确要求时做 fast-forward；不要把 `main`
直接合入当前分支。

---

## 2. 为什么做这次迁移

旧 fork 的实现把 Chat Completions 语义直接放进了 `codex-core`、
`codex-api`、`codex-tools` 和 `codex-model-provider`。结果是：

- 上游改动很难 cherry-pick；
- Chat/Responses 历史会互相污染；
- 工具命名空间会丢失；
- subagent 投递在 function-only wire 上容易断；
- `wire_api = "chat"` 把协议选择和历史兼容语义混在一起。

这次迁移的目标不是“恢复旧 Chat wire”，而是：

> 保持 Codex 内部历史为 canonical `ResponseItem`，只在 provider 边界做
> wire 适配。

---

## 3. 已完成架构

### 3.1 Provider transport 边界

`codex-api` 现在有 provider-owned transport 抽象：

```text
codex-rs/codex-api/src/transport.rs
codex-rs/codex-api/src/endpoint/responses.rs
codex-rs/model-provider/src/provider.rs
```

默认实现仍然是原 Responses HTTP client。这样 provider 可以返回不同 transport，
而 agent loop 不需要知道新的 wire 格式。

### 3.2 纯翻译 crate

新增：

```text
codex-rs/ext/mycode/model-wire/
```

crate：

```text
codex-mycode-model-wire
```

职责：

- `CanonicalRequest`
- `CanonicalItem`
- `CanonicalToolCall`
- `CanonicalToolResult`
- `CanonicalEvent`
- `WireCapabilities`
- `WireAdapter`
- `ToolPlan`
- namespace flattening
- 可逆工具身份映射
- Chat request 构造

该 crate 不依赖 core、session、auth、HTTP、TUI。

### 3.3 Chat transport adapter

新增：

```text
codex-rs/ext/mycode/chat-adapter/
```

crate：

```text
codex-mycode-chat-adapter
```

职责：

- POST `chat/completions`
- 复用 `codex_api::EndpointSession`
- auth、retry、request telemetry、SSE telemetry
- Chat SSE 解析
- tool-call argument delta 合并
- 归一化为 `ResponseEvent`
- `[DONE]` 完成处理
- context/quota/overload 错误归一化

### 3.4 Provider 选择

当前 `ProviderCapabilities` 有过渡字段：

```rust
pub response_adapter: ResponseAdapter,
```

枚举：

```rust
pub enum ResponseAdapter {
    Responses,
    ChatCompletions,
}
```

配置暂时通过保留 header：

```toml
[model_providers.example.http_headers]
x-codex-response-adapter = "chat_completions"
```

该 header：

- 在本地被消费；
- 不会发给 provider；
- 会经过 `ModelProviderInfo::validate`；
- 不是最终配置设计。

不要恢复：

```toml
wire_api = "chat"
```

### 3.5 Core dispatch

`ModelClientSession::stream` 现在按 provider capability 分派：

```text
Responses        -> 原 WebSocket/HTTP Responses 路径
ChatCompletions  -> Chat Completions adapter
```

Chat 路径：

```text
Prompt + ResponseItem history
  -> ToolPlan::new(..., function_only())
  -> adapt history copy
  -> CanonicalRequest
  -> ChatCompletionsClient
  -> Chat SSE
  -> decode flat tool names
  -> ResponseEvent
```

Chat wire encoding 已经收口在 `ChatCompletionsClient`，core 不再直接构造
Chat body。

### 3.6 ToolPlan

当前实现路径：

```text
codex-rs/ext/mycode/model-wire/src/tool_plan.rs
```

它负责：

- 按 `WireCapabilities` 隐藏不支持的工具；
- 把 namespace 工具扁平化为 `namespace__name`；
- 维护 wire name -> canonical `ToolName` 映射；
- 适配历史调用；
- 在 Chat SSE 返回时反解工具名。

注意：它不是用户可见的执行计划。

---

## 4. 当前已经能工作的事情

- ResponsesNative 默认路径保持原样。
- Provider 可以选择 Chat Completions adapter。
- Chat 请求使用 canonical `ResponseItem` 历史。
- Chat 请求支持 function-only 工具裁剪。
- Chat 请求支持 namespace 扁平化。
- Chat SSE 支持文本、reasoning、usage、tool-call delta。
- Chat 工具名可以在返回时反解。
- Chat tool call / tool output / `call_id` 可以编码到 Chat history。
- Chat adapter 具备 auth recovery、telemetry、trace 基础路径。
- Core 的 Chat request/stream/history 测试已经通过。

---

## 5. 已完成能力

### 5.1 ResponsesFunctionOnly

已完成。

目标：

- 普通 Responses endpoint 也能使用 function-only 工具面；
- namespace 工具扁平化；
- custom/freeform 工具隐藏；
- Responses Lite、tool_search、built-in tools 按能力关闭；
- 历史请求副本同步适配。

相关设计见 spec 的：

```text
ResponsesFunctionOnly
Request Flow
ToolPlan
```

### 5.2 Responses 返回侧工具名反解

已完成。Chat SSE 与 ResponsesFunctionOnly 都会在进入 router 前解码。

ResponsesFunctionOnly 必须也在事件进入 session/tool router 前反解：

```text
FunctionCall wire name
  -> ToolPlan::decode_flat_name
  -> canonical ToolName
  -> session/tool router
```

推荐插入点：

```text
codex-rs/core/src/client.rs
map_response_events / map_response_stream
```

### 5.3 `WireAdapter` 策略化

已完成。`ResponseAdapter` 只负责配置解析，运行时策略是：

```rust
pub enum WireAdapter {
    ResponsesNative,
    ResponsesFunctionOnly,
    ChatCompletionsFunctionOnly,
}
```

并让它拥有：

- capabilities；
- tool plan；
- history adaptation；
- tool-name decoding；
- transport kind。

不要引入 `async_trait` 或 `#[allow(async_fn_in_trait)]` 作为捷径。

### 5.4 跨 wire conformance

已覆盖：

- Chat -> ResponsesFunctionOnly；
- ResponsesFunctionOnly -> Chat；
- ResponsesNative -> function-only；
- function-only -> ResponsesNative；
- 多工具调用；
- subagent/AgentMessage 投递；
- 不把 Chat `tool_calls` 泄漏到 Responses；
- 不把 Responses namespace 泄漏到 function-only wire。

### 5.5 最终配置设计

已完成第一类配置层；legacy header 保留为兼容 alias。

目标配置形状：

```toml
[model_providers.example.extensions]
wire_adapter = "responses_function_only"
```

`ModelProviderInfo.extensions` 已在 `WireAdapter` 稳定后加入；结构体字面量
只做了机械补全，legacy header 仍保留为兼容路径。

### 5.6 Anthropic Messages

暂缓，作为后续独立阶段处理。

要求：

- 独立 adapter crate；
- 复用 `CanonicalRequest`、`CanonicalEvent`、`ToolPlan`；
- 不把 Anthropic message shape 引入 core session state。

### 5.7 e/w/r 文件工具

已完成第一版。

纯逻辑 crate：

```text
codex-rs/ext/mycode/file-tools/
```

crate：

```text
codex-mycode-file-tools
```

职责：

- `edit_file` exact/fuzzy 匹配和 diff；
- `read_file` 流式行扫描和 `cat -n` 渲染；
- `write_file` create-only plan；
- 三个工具的 JSON function spec。

该 crate 不依赖 `codex-exec-server`，不调用 `std::fs`。

core 薄适配：

```text
codex-rs/core/src/tools/handlers/file_tools/
```

- `read_file` 直接使用 sandboxed `ExecutorFileSystem` streaming read；
- `edit_file` / `write_file` 使用 `FileMutationRuntime`；
- `FileMutationRuntime` 复用 `ToolOrchestrator`、`ApprovalAction::ApplyPatch`
  和 `ToolEmitter`；
- sandbox denial 会映射为 `SandboxErr::Denied`，允许正常 escalation 和重试；
- 工具注册只要求 `environment_mode.has_environment()`，不再受
  `model_info.apply_patch_tool_type` 影响。

`codex-apply-patch` 只做了最小公开：

- `seek_sequence` 模块；
- `AppliedPatchDelta::new`。

已完成：

- TUI/file-change 事件显示真实 `edit_file` / `write_file` 工具名；
- 上游 `ToolPolicy` 补丁已 cherry-pick，完成工具限制与 session identity
  解耦。

尚未做：

- 独立 `ApprovalAction::FileMutation` 和独立 approval cache key。

---

## 6. 阶段状态

### Step 1：WireAdapter 策略化（已完成）

目标文件：

```text
codex-rs/ext/mycode/model-wire/src/capabilities.rs
codex-rs/model-provider/src/provider.rs
codex-rs/core/src/client.rs
```

产出：

- `WireAdapter` 承担 capabilities/tool plan/history/decoder/transport kind；
- 保持 Chat 现有测试绿。

### Step 2：ResponsesFunctionOnly 请求适配（已完成）

目标文件：

```text
codex-rs/core/src/client.rs
codex-rs/model-provider/src/provider.rs
codex-rs/ext/mycode/model-wire/src/tool_plan.rs
```

产出：

- function-only Responses request；
- 顶层 `tools`；
- 无 `additional_tools`；
- 无 namespace/custom/builtin；
- 历史副本适配。

### Step 3：Responses 返回侧解码（已完成）

目标文件：

```text
codex-rs/core/src/client.rs
```

产出：

- 将 active `ToolPlan` 传入 response mapping；
- `FunctionCall` 进入 router 前解码；
- 未知 flat name 有明确 fallback。

### Step 4：Conformance（已完成）

建议新增或恢复：

```text
codex-rs/core/tests/suite/multiadapt_conformance.rs
```

至少覆盖：

```text
chat_single_tool_round_trip
chat_multi_tool_round_trip
responses_function_only_single_tool_round_trip
responses_function_only_multi_tool_round_trip
switching_from_chat_to_responses_resumes_canonical_history
switching_from_responses_to_chat_resumes_canonical_history
subagent_delivery_on_function_only_wires
no_chat_tool_calls_leak_into_responses
```

### Step 5：Anthropic（暂缓）

前四步已稳定；Anthropic 作为后续独立阶段处理。

### Step 6：最终配置层（已完成）

已支持：

```toml
[model_providers.example.extensions]
wire_adapter = "responses_function_only"
```

legacy header 仍作为兼容 alias。

### Step 7：e/w/r 文件工具（第一版已完成）

目标文件：

```text
codex-rs/ext/mycode/file-tools/
codex-rs/core/src/tools/handlers/file_tools/
codex-rs/core/src/tools/spec_plan.rs
```

产出：

- 纯逻辑与 I/O 分层；
- `read_file` 走 sandboxed streaming filesystem；
- `edit_file` / `write_file` 走 `ToolOrchestrator`；
- sandbox denial 可进入审批和 escalation；
- `apply_patch` 算法通过 `seek_sequence` 复用。

---

## 7. 迁移纪律

### 7.1 上游 main

迁移完成前默认不把上游 `main` 合入当前分支：

- 不做普通 `git pull`；
- 不 checkout `main`；
- 不 reset 当前分支到 `main`；
- fork owner 明确要求时，可以 fetch、fast-forward 本地 `main`，或在
  临时 worktree 做 cherry-pick/merge 探测；
- 只有经过评估的补丁才 cherry-pick 到 `myXCode-features`。

### 7.2 Cherry-pick 友好

优先：

- 新 crate；
- 新 module；
- provider extension；
- transport adapter；
- 小型 core seam。

避免：

- 大范围修改上游 struct；
- 把 wire-specific 分支塞进 tool router；
- 恢复已删除的历史 wire 语义；
- 在 core 中复制 Chat/Anthropic message 类型。

### 7.3 历史不变量

- 内部历史永远是 canonical `ResponseItem`。
- Wire-specific shape 不进入持久历史。
- 跨 wire 切换保持 message order。
- 保持 `FunctionCall.call_id`。
- 保持 `FunctionCallOutput.call_id`。
- namespace 身份必须可逆。
- 不支持的工具有明确 fail-closed 行为。

---

## 8. 验证现状

最近通过：

```text
just fmt
just test -p codex-mycode-model-wire                         # 15 passed
just test -p codex-model-provider-info                       # 39 passed
just test -p codex-model-provider                            # 102 passed
just test -p codex-config                                    # 339 passed
just test -p codex-core --lib responses_function_only_       # 3 passed
just test -p codex-core --lib chat_completions_adapter_      # 2 passed
just test -p codex-core --test all 'multiadapt_conformance::' # 10 passed
just test -p codex-mycode-file-tools                         # 18 passed
just test -p codex-apply-patch                               # 99 passed
just test -p codex-core --lib spec_plan                      # 56 passed
just test -p codex-core --lib file_tools                     # 3 passed
just test -p codex-tui clock_format                          # 4 passed
just test -p codex-app-server model_list_requirements        # 2 passed
just test -p codex-exec-server fs_sandbox                    # 15 passed
just test -p codex-agent-message-board-extension             # 7 passed
just test -p codex-core --test all agent_message_board       # 6 passed
just test -p codex-app-server thread_delete                  # 7 passed
just test -p codex-app-server-protocol                       # 309 passed
just test -p codex-tui                                       # 5434 passed, 4 existing cursor failures
just fix -p codex-mycode-file-tools
just fix -p codex-apply-patch
just fix -p codex-core
just fix -p codex-tui
just fix -p codex-app-server
just fix -p codex-extension-api
just fix -p codex-agent-message-board-extension
just write-app-server-schema
just write-app-server-schema --experimental
just bazel-lock-update
```

2026-09-25 商业化/遥测阻断轮追加（每项都与改动前的 HEAD 基线逐项对比，
**新增失败为 0**）：

```text
just fmt
python3 scripts/mycode-verify-telemetry-blocked.py codex-rs/target/debug/codex
just test -p codex-mycode-policy -p codex-otel -p codex-feedback \
  -p codex-analytics -p codex-config -p codex-core-plugins   # 1012 passed
just test -p codex-core                                      # 383 failed（基线 384，新增 0）
just test -p codex-tui                                       #  45 failed（基线  45，新增 0）
just test -p codex-app-server                                # 103 failed（基线 103，新增 0）
just test -p codex-cli                                       #  18 failed（基线  19，新增 0）
```

基线复现方式：`git stash push` 后在未改动状态重跑同一 crate，导出
`target/nextest/local/junit.xml` 的失败集合做差集。上表“基线”列即该次
结果，全部为 fork 既有失败，与本次改动无关。

二进制扫描实测（debug 构建，是比 release 更难消除的场景）：

```text
ok: 1 binary/binaries carry no blocked telemetry payloads
note: 3 deferred payload(s) are still compiled in
```

注意：

- 机器上同时有其他大型 Rust 构建时，Cargo 会等待 artifact lock；
- 链接超大 core test binary 时曾遇到 `lld` bus error，重跑后通过；
- 不要因为链接器错误而改业务代码；
- 迁移期间不要执行 workspace-wide `cargo test`，除非用户明确要求。
- `codex-core --lib` 全量在当前机器上仍有多 agent 既有超时；已在 Stage 6 前的
  HEAD 复现，和 wire adapter 改动无关。
- 2026-09-24 磁盘曾 100% 占满导致构建失败（主因是 `~/项目/qaqh-backend/target`
  275G）。测试前先 `df -h /home`；本仓库 `codex-rs/target` 约 12G。
- `codex-core` / `codex-tui` / `codex-app-server` 的既有失败里，有相当一部分是
  Bazel-only 测试二进制缺失（`could not locate binary "test_stdio_server"`）
  和超时（nextest `TMT`），不要误判为业务回归。
- 若在本仓库内跑测试，注意 `.snap.new` 会因 multi-agent prompt 注入产生；
  这些快照漂移在改动前就存在，确认后可用
  `find codex-rs -name '*.snap.new' -delete` 清理。

---

## 9. 关键文件索引

```text
docs/mycode-wire-adapter-spec.md
codex-rs/ext/mycode/model-wire/src/canonical.rs
codex-rs/ext/mycode/model-wire/src/capabilities.rs
codex-rs/ext/mycode/model-wire/src/chat.rs
codex-rs/ext/mycode/model-wire/src/tool_plan.rs
codex-rs/ext/mycode/chat-adapter/src/client.rs
codex-rs/ext/mycode/chat-adapter/src/sse.rs
codex-rs/ext/mycode/file-tools/src/edit.rs
codex-rs/ext/mycode/file-tools/src/read.rs
codex-rs/ext/mycode/file-tools/src/spec.rs
codex-rs/core/src/tools/handlers/file_tools/runtime.rs
codex-rs/core/src/tools/handlers/file_tools/file_tools_tests.rs
codex-rs/codex-api/src/transport.rs
codex-rs/model-provider-info/src/lib.rs
codex-rs/model-provider/src/provider.rs
codex-rs/core/src/client.rs
codex-rs/core/src/client_tests.rs
```

商业化/遥测阻断轮新增：

```text
codex-rs/ext/mycode/policy/src/lib.rs        # 单一策略开关与派生默认值
codex-rs/otel/src/config.rs                  # Statsig 端点/密钥阻断
codex-rs/feedback/src/lib.rs                 # 反馈上传阻断
codex-rs/feedback/src/report_upload.rs       # 反馈持久化传输阻断
codex-rs/tui/src/updates.rs                  # 更新探测 URL 阻断
codex-rs/tui/src/tooltips.rs                 # 公告 tip 抓取阻断
codex-rs/tui/src/pets/asset_pack.rs          # 宠物素材 CDN 阻断
codex-rs/cli/src/doctor/updates.rs           # doctor 更新/桌面 CDN 探测阻断
codex-rs/core/src/config/otel.rs             # metrics exporter 默认值
codex-rs/config/src/types.rs                 # OtelConfig 默认值
scripts/mycode-verify-telemetry-blocked.py   # 构建产物字符串扫描
```

---

## 10. 交接检查清单

接手后先确认：

- [x] 当前分支是 `myXCode-features`；
- [x] 工作树包含 e/w/r 第一版改动；
- [x] 上游 `main` 已按 owner 要求 fast-forward；未合入当前分支；
- [x] 阅读 `docs/mycode-wire-adapter-spec.md`；
- [x] 完成 `WireAdapter` 策略化；
- [x] 完成 `ResponsesFunctionOnly`；
- [x] 完成 response-side tool name decoding；
- [x] 完成 cross-wire conformance；
- [x] 完成第一类 provider extension 配置层；
- [x] 完成 e/w/r 纯逻辑 crate 和 core sandbox adapter；
- [x] TUI 展示真实文件工具名；
- [x] 完成上游 `ToolPolicy` cherry-pick；
- [x] 完成 TUI 产品化 43 个上游提交同步；
- [x] 完成 multi-agent/message board 38 个上游提交同步；
- [ ] 文件工具独立 approval action/cache key 尚未实现；
- [ ] Anthropic Messages 暂缓，尚未实现；
- [ ] 最终 workspace-wide `just test` 尚未执行；
- [x] 所有改动继续按小提交拆分，保持 cherry-pick 友好；
- [x] 建立 `codex-mycode-policy` 单一策略开关；
- [x] 阻断 Statsig 指标、Sentry 反馈、更新探测、公告 tip、宠物素材 CDN；
- [x] 用 `scripts/mycode-verify-telemetry-blocked.py` 验证构建产物；
- [ ] analytics 事件管道尚未移除（下一轮“移除代码”）；
- [ ] curated 插件仓库同步尚未移除（下一轮“移除代码”）；
- [ ] cloud-config 云开关尚未处理（下一轮“移除代码”）；
- [ ] `analytics.enabled` / `feedback.enabled` 默认值尚未翻转（下一轮）。

---

## 11. 商业化与遥测阻断（2026-09-25）

### 11.1 设计原则

阻断点全部收敛到 fork 自持的 `codex-rs/ext/mycode/policy`：

```rust
pub const BLOCK_COMMERCIAL_TELEMETRY: bool = true;
```

调用点用 **`const` 早返回** 或 **`const if`** 取值，由编译器消除死分支。
选这个方案而不是 `--cfg` / `RUSTFLAGS` 的原因：

- 不需要动构建系统，**Cargo 与 Bazel 两条发布路径同时生效**；
- 不会覆盖 `codex-rs/.cargo/config.toml` 里 Windows 专用的
  `-C link-arg=/STACK:8388608` 等 `rustflags`（env 形式的 RUSTFLAGS
  会整体覆盖它们）；
- 上游源码原样保留，**cherry-pick 继续可应用**；
- 本地 `just test` 与上游测试语义不变（除了下面列出的三处测试适配）。

已验证：跨 crate 的 `const bool` 在 **debug 构建**下也会把死分支里的字面量
消除，因此不依赖 release 优化。

### 11.2 本轮已阻断

| 项 | 位置 | 说明 |
| --- | --- | --- |
| Statsig 指标端点 + 硬编码 API key | `otel/src/config.rs` | `ab.chatgpt.com` 与 key 均不进二进制 |
| `otel.metrics_exporter` 默认值 | `core/src/config/otel.rs`、`config/src/types.rs` | 默认 `None` |
| Sentry DSN + 反馈上传 | `feedback/src/lib.rs`、`report_upload.rs` | DSN 不进二进制，上传直接拒绝 |
| 更新探测 URL | `tui/src/updates.rs`、`cli/src/doctor/updates.rs` | GitHub / Homebrew / 桌面 CDN |
| `check_for_update_on_startup` 默认值 | `core/src/config/mod.rs` | 默认 `false` |
| 公告 tip 抓取 | `tui/src/tooltips.rs` | 不再抓取，URL 消除 |
| 宠物素材 CDN | `tui/src/pets/asset_pack.rs` | 不再下载，URL 消除 |

测试适配（共 3 处，均为 fork 行为变化所致）：

- `tui/src/pets/asset_pack.rs`：URL 用例改为断言阻断行为；
- `app-server/tests/suite/v2/feedback.rs`：改为断言上传被拒绝；
- `core/src/config/config_tests.rs`：metrics exporter 默认值断言改为 `None`。

### 11.3 下一轮：移除代码（不是再阻断）

按 owner 的排序，下面这些不再叠加阻断层，而是直接删代码，同时把相关
上游测试一次性改写或删除：

1. **analytics 事件管道** `/codex/analytics-events/events`
   （`codex-analytics` + `core/src/session/session.rs` 构造点）。
   实测代价：会让 `codex-core` 新增 28 个、`codex-app-server` 新增 46 个
   失败，且 app-server 的 config helper 按文件分散、没有单点 opt-in。
2. **curated 插件仓库同步** `plugins/export/curated` +
   `github.com/openai/plugins.git`（`core-plugins/src/startup_sync.rs`）。
   实测代价：`marketplace_upgrade`、`curated_mcp_sync` 等 fixture 测试。
3. **cloud-config 云开关**（`codex-cloud-config`、`ConfigRequirements`
   下发的 managed requirements）。注意 `feedback.enabled` 可被云侧强制打开，
   而 `analytics` 不在 `ConfigRequirements` 里、无法被云侧强制。
4. **默认值翻转**：`analytics.enabled`、`feedback.enabled` 默认关闭。

### 11.4 验证脚本

```bash
python3 scripts/mycode-verify-telemetry-blocked.py <binary> [<binary> ...]
```

- `BLOCKED_PAYLOADS` 命中即退出码 1，应接进发布流程；
- `DEFERRED_PAYLOADS` 只告警，就是上面 11.3 的待办清单；
- 建议在 `scripts/build_codex_package.py` 打包后、以及 Bazel
  `//codex-rs/cli:release_binaries` 之后各跑一次。

### 11.5 提交

```text
9824b3263 feat(mycode): add fork telemetry policy crate
cc3f00bc5 fix(mycode): restore codex-analytics test compilation
7c5e7b678 feat(mycode): block commercial telemetry at its egress points
```
