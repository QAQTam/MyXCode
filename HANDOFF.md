# HANDOFF — MyCode 迁移工作

更新时间：2026-09-21  
当前分支：`myXCode-features`  
设计文档：`docs/mycode-wire-adapter-spec.md`

> **迁移完成前，不允许同步上游 `main`。**
>
> 具体禁止：
>
> - 不要 `git fetch upstream`
> - 不要 `git pull`
> - 不要 `git checkout main`
> - 不要 `git reset --hard main`
> - 不要为了“先保持干净”而移动当前分支 HEAD
>
> `main` 继续作为上游镜像保留，不接收 fork 改动。等当前迁移完成并由 fork
> owner 明确解除限制后，再讨论上游同步。

---

## 0. 一句话状态

Chat Completions 的独立 wire adapter 已经接入 core，并具备 canonical history、
ToolPlan、SSE 解析、工具名反解和 provider 选择能力。

当前工作区干净，迁移没有完成：

- 还没有 `ResponsesFunctionOnly`
- 还没有 Responses 返回侧统一工具名反解
- 还没有跨 wire conformance
- 还没有 Anthropic Messages
- 当前配置仍是过渡用的保留 header

---

## 1. 分支与提交

当前分支：

```text
myXCode-features
```

当前 HEAD 附近的迁移提交：

```text
c5639d3de6 refactor(mycode): keep chat wire encoding in adapter
d86b294dfe docs(mycode): describe response adapter capability
ca907ddbf8 test(mycode): cover canonical chat tool history
17afc8f7eb feat(mycode): wire chat completions adapter into core
acc628e5fa feat(mycode): add standalone chat completions transport adapter
3113263356 feat(mycode): add provider-neutral model wire translation
bebb70fcee refactor: route response streaming through provider transport
```

更早的相关提交：

```text
f0013687f2 Honor an environment API key on every entry point
6763c3684b feat(tui): add runtime status line metrics
```

不要修改 `main`。不要执行任何会改变 `main` 的命令。

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

## 5. 当前还没有完成的事情

### 5.1 ResponsesFunctionOnly

这是下一阶段第一优先级。

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

现在只有 Chat SSE 使用 `ToolPlan::decode_flat_name`。

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

当前 `ResponseAdapter` 只选择 transport。下一步应把它演进成：

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

必须补：

- Chat -> ResponsesFunctionOnly；
- ResponsesFunctionOnly -> Chat；
- ResponsesNative -> function-only；
- function-only -> ResponsesNative；
- 多工具调用；
- subagent/AgentMessage 投递；
- 不把 Chat `tool_calls` 泄漏到 Responses；
- 不把 Responses namespace 泄漏到 function-only wire。

### 5.5 最终配置设计

当前保留 header 只是过渡。

目标配置形状：

```toml
[model_providers.example.extensions]
wire_adapter = "responses_function_only"
```

在 `WireAdapter` 稳定前，不要给上游 `ModelProviderInfo` 直接加字段。
那会造成大量结构体字面量改动，并放大未来 cherry-pick 冲突。

### 5.6 Anthropic Messages

Anthropic 放在 `WireAdapter` 和 `ResponsesFunctionOnly` 之后。

要求：

- 独立 adapter crate；
- 复用 `CanonicalRequest`、`CanonicalEvent`、`ToolPlan`；
- 不把 Anthropic message shape 引入 core session state。

---

## 6. 下一步建议顺序

### Step 1：WireAdapter 策略化

目标文件：

```text
codex-rs/ext/mycode/model-wire/src/capabilities.rs
codex-rs/model-provider/src/provider.rs
codex-rs/core/src/client.rs
```

产出：

- `WireAdapter` 承担 capabilities/tool plan/history/decoder/transport kind；
- 保持 Chat 现有测试绿。

### Step 2：ResponsesFunctionOnly 请求适配

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

### Step 3：Responses 返回侧解码

目标文件：

```text
codex-rs/core/src/client.rs
```

产出：

- 将 active `ToolPlan` 传入 response mapping；
- `FunctionCall` 进入 router 前解码；
- 未知 flat name 有明确 fallback。

### Step 4：Conformance

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

### Step 5：Anthropic

在前四步稳定后再开。

---

## 7. 迁移纪律

### 7.1 上游 main

迁移完成前：

- 不同步上游 `main`；
- 不 fetch；
- 不 pull；
- 不 checkout `main`；
- 不 reset 到 `main`。

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
cargo fmt --all -- --check
cargo check -p codex-mycode-chat-adapter -p codex-core --tests -j 4
cargo test -p codex-model-provider-info -j 4       # 35 passed
cargo test -p codex-model-provider -j 4            # 99 passed
cargo test -p codex-mycode-model-wire -j 4         # 12 passed
cargo test -p codex-mycode-chat-adapter -j 4       # 2 passed
cargo test -p codex-core --lib chat_completions_adapter_ -j 4  # 2 passed
```

注意：

- 机器上同时有其他大型 Rust 构建时，Cargo 会等待 artifact lock；
- 链接超大 core test binary 时曾遇到 `lld` bus error，重跑后通过；
- 不要因为链接器错误而改业务代码；
- 迁移期间不要执行 workspace-wide `cargo test`，除非用户明确要求。

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
codex-rs/codex-api/src/transport.rs
codex-rs/model-provider-info/src/lib.rs
codex-rs/model-provider/src/provider.rs
codex-rs/core/src/client.rs
codex-rs/core/src/client_tests.rs
```

---

## 10. 交接检查清单

接手后先确认：

- [ ] `git status` 干净；
- [ ] 当前分支是 `myXCode-features`；
- [ ] 没有执行任何上游同步；
- [ ] 阅读 `docs/mycode-wire-adapter-spec.md`；
- [ ] 先做 `WireAdapter` 策略化；
- [ ] 再做 `ResponsesFunctionOnly`；
- [ ] 再做 response-side tool name decoding；
- [ ] 最后补 conformance；
- [ ] Anthropic 和最终配置层放到后面；
- [ ] 所有改动继续按小提交拆分，保持 cherry-pick 友好。
