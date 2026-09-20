# MyCode Wire Adapter V2 Spec

Status: **partially implemented; ResponsesFunctionOnly and Anthropic are not implemented**

Written: 2026-09-21

Branch context: `myXCode-features`

This document defines the next stage of the fork's MultiAdapt work. It is an
internal engineering spec, not user-facing product documentation.

The short version:

> Codex history and agent state remain canonical. Provider-specific request,
> response, streaming, and tool semantics are adapted at the provider boundary.
> The agent core must not learn Chat Completions or Anthropic message shapes.

---

## 1. Goals

The fork must support multiple model-provider wire protocols without requiring
an external translating proxy.

V2 must provide:

- a provider-neutral request, response, streaming, and tool boundary;
- a conservative `ResponsesFunctionOnly` adapter for endpoints that implement
  ordinary Responses functions but not namespaces, custom tools, Responses Lite,
  tool search, or built-in tools;
- a Chat Completions function-only adapter for the broad OpenAI-compatible
  provider ecosystem;
- reversible tool identity mapping, including namespace flattening;
- response-side decoding of wire tool names before tool routing;
- provider-selected adapter policy that is explicit, testable, and fail-closed;
- a path to add Anthropic Messages later without changing the agent loop again.

The agent core must continue to store and process canonical `ResponseItem`
history. Wire formats are encoding and decoding layers, not internal state.

---

## 2. Non-goals

V2 does not include:

- restoring the historical `wire_api = "chat"` configuration semantics;
- full Chat Completions feature parity;
- native namespace emulation on Chat Completions;
- Chat Completions custom/freeform tool support;
- Chat Completions Responses Lite support;
- built-in web search or tool search for Chat providers;
- automatic migration of old wire-specific session history;
- an Anthropic Messages implementation in the first V2 milestone;
- a general provider SDK abstraction.

Anthropic is a follow-up adapter. It must fit the boundary defined here, but it
must not drive the first implementation stage.

---

## 3. Current State

The following pieces already exist:

### 3.1 Provider transport boundary

`codex-api` defines a provider-owned `ResponseTransport` trait. The default
implementation is the existing Responses HTTP client.

Relevant paths:

```text
codex-rs/codex-api/src/transport.rs
codex-rs/codex-api/src/endpoint/responses.rs
codex-rs/model-provider/src/provider.rs
```

This lets a provider return a different transport without teaching the agent
loop about another wire format.

### 3.2 Pure model-wire crate

The fork has an independent crate:

```text
codex-rs/ext/mycode/model-wire/
```

Crate name:

```text
codex-mycode-model-wire
```

It owns:

- `CanonicalRequest`
- `CanonicalItem`
- `CanonicalToolCall`
- `CanonicalToolResult`
- `CanonicalEvent`
- `WireCapabilities`
- `WireAdapter`
- `ToolPlan`
- Chat request construction

It has no dependency on `codex-core`, sessions, auth, HTTP, or the TUI.

### 3.3 Chat Completions adapter

The fork has a second independent crate:

```text
codex-rs/ext/mycode/chat-adapter/
```

Crate name:

```text
codex-mycode-chat-adapter
```

It owns:

- Chat Completions HTTP POST to `chat/completions`
- reuse of `codex_api::EndpointSession`
- auth, retry, request telemetry, and SSE telemetry
- Chat SSE parsing
- tool-call argument delta merging
- normalization into `ResponseEvent`
- `[DONE]` completion handling
- context/quota/overload error normalization

### 3.4 Provider selection

`ProviderCapabilities` currently exposes a transitional `ResponseAdapter`:

```rust
pub enum ResponseAdapter {
    Responses,
    ChatCompletions,
}
```

The configured provider reads the selected adapter from a reserved local
header:

```toml
[model_providers.example.http_headers]
x-codex-response-adapter = "chat_completions"
```

The header is consumed locally and is never sent to the provider.

This is a temporary configuration bridge. It exists to avoid adding a field to
upstream `ModelProviderInfo` and rewriting roughly one hundred upstream struct
literals.

### 3.5 Core dispatch

`ModelClientSession::stream` dispatches on the provider capability:

```text
Responses        -> existing WebSocket/HTTP Responses path
ChatCompletions  -> Chat Completions adapter
```

The Chat path builds a canonical request, applies `ToolPlan`, and passes the
plan to the Chat SSE parser so flattened tool names can be decoded back into
canonical names.

### 3.6 Tests

Current focused coverage includes:

- model-wire translation and tool-plan tests;
- Chat SSE tool argument merging;
- Chat flattened-name decoding;
- configured-provider adapter selection;
- core Chat request/stream tests;
- canonical Chat tool history encoding.

---

## 4. Architecture

### 4.1 Layers

The intended layering is:

```text
canonical agent state
  ResponseItem history
  ToolSpec tool definitions
  ResponseEvent streaming events
        |
        v
wire policy
  WireAdapter
  WireCapabilities
  ToolPlan
        |
        v
wire codec
  canonical request -> wire request
  wire event -> canonical event
  wire tool name -> canonical ToolName
        |
        v
transport
  Responses HTTP/WebSocket
  Chat Completions HTTP/SSE
  future Anthropic HTTP/SSE
```

The agent core owns:

- agent loop;
- prompt and history construction;
- tool execution;
- approvals;
- session state;
- persistence.

The wire layer owns:

- provider capability policy;
- request translation;
- response translation;
- streaming translation;
- tool surface adaptation;
- reversible tool identity mapping.

The transport layer owns:

- HTTP and WebSocket details;
- auth headers;
- retries and timeouts;
- compression;
- SSE byte transport;
- request telemetry.

### 4.2 Dependency Direction

Intended dependency direction:

```text
codex-protocol
  <- codex-tools
  <- codex-mycode-model-wire
  <- codex-mycode-chat-adapter
  <- codex-core

codex-protocol
  <- codex-api
  <- codex-model-provider
  <- codex-core
```

`codex-mycode-model-wire` must remain pure translation. It must not depend on:

- `codex-core`;
- sessions;
- rollouts;
- auth;
- HTTP clients;
- TUI;
- tool execution handlers.

The Chat adapter may depend on `codex-api` and `codex-client` for transport,
but it must not depend on `codex-core`.

### 4.3 Enum Dispatch First

Use enum dispatch for V2.

Do not introduce `async_trait` or `#[allow(async_fn_in_trait)]` just to make
the adapter boundary look extensible. The known variants are small and the
transport boundary already handles the async part.

A future trait may be justified if:

- a third-party adapter must be loaded dynamically;
- the variant count becomes large enough to hurt maintainability;
- adapter construction needs provider-owned state that cannot be represented
  by the enum.

Until then, an enum is simpler and easier to test.

---

## 5. Canonical Boundary

The canonical boundary is intentionally conservative.

### 5.1 Canonical Request

Minimum request concepts:

```text
model
instructions
items
tools
tool_choice
parallel_tool_calls
stream
provider_metadata
```

The canonical item list remains `Vec<ResponseItem>` for V2. This avoids a
second history representation and keeps existing persistence compatible.

### 5.2 Canonical Streaming Events

Minimum streaming concepts:

```text
TextDelta
ReasoningDelta
ToolCallStarted
ToolCallArgumentsDelta
ToolCallCompleted
Usage
Completed
Error
```

For the current implementation, the adapter ultimately emits the existing
`ResponseEvent` shape. `CanonicalEvent` remains the pure model-wire vocabulary
for future adapters.

### 5.3 Canonical Tool Identity

Tool identity is:

```rust
ToolName {
    namespace: Option<String>,
    name: String,
}
```

The wire may flatten this identity, but the canonical identity must survive a
round trip.

The following must always remain associated:

- tool call ID;
- tool result ID;
- tool name;
- tool namespace;
- function arguments;
- function output;
- message order.

---

## 6. ToolPlan

`ToolPlan` is the provider-neutral model-visible tool surface plus the mapping
needed to decode model calls back into canonical tool identities.

It is not an execution plan shown to the user.

Relevant path:

```text
codex-rs/ext/mycode/model-wire/src/tool_plan.rs
```

### 6.1 Responsibilities

`ToolPlan::new(tools, capabilities)` must:

- hide tools that the selected wire cannot encode;
- preserve ordinary function tools;
- flatten namespaced tools when namespace tools are unsupported;
- detect name collisions;
- retain a reversible wire-name to canonical-name mapping;
- expose the final model-visible tool list;
- adapt history items to the selected wire;
- decode model-returned wire names back to canonical names.

### 6.2 Namespace Flattening

Canonical tool:

```text
namespace = "files"
name = "read"
```

Function-only wire name:

```text
files__read
```

Mapping:

```text
"files__read" -> ToolName {
    namespace: Some("files"),
    name: "read",
}
```

Rules:

- The separator is `__`.
- The default function namespace remains bare.
- The mapping must be reversible using the active `ToolPlan`.
- Name collisions must fail before request construction.
- The wire name must be validated against provider name limits.
- The tool router must never see a wire-flattened name that has a known
  canonical mapping.

### 6.3 Capability Rules

`WireCapabilities::function_only()` means:

```text
namespace_tools: false
custom_tools: false
responses_lite: false
tool_search: false
built_in_tools: false
parallel_tool_calls: true
```

When a capability is false:

- hide the corresponding tool from the model-visible tool list;
- do not serialize a partial or empty native tool definition;
- do not silently convert a built-in tool into a shell command;
- do not emit an input item that requires the unsupported feature;
- adapt or omit history items that cannot be represented.

### 6.4 History Adaptation

History adaptation must be applied only to the request copy.

It must not mutate persisted history.

Examples:

- namespaced `FunctionCall` becomes a flattened function call;
- plaintext `AgentMessage` becomes a user message for function-only wires;
- encrypted `AgentMessage` becomes a bounded undeliverable notice for
  function-only wires;
- unsupported custom/built-in calls are hidden or represented only when the
  wire can preserve their semantics.

---

## 7. WireAdapter Policy

The current `ResponseAdapter` is too narrow. It selects transport, but it does
not own request/response semantics.

V2 should make `WireAdapter` the single provider-owned policy object.

Proposed shape:

```rust
pub enum WireAdapter {
    ResponsesNative,
    ResponsesFunctionOnly,
    ChatCompletionsFunctionOnly,
}
```

Reserved future variant:

```rust
AnthropicMessages
```

### 7.1 Responsibilities

`WireAdapter` should provide:

```rust
fn capabilities(self) -> WireCapabilities;
fn tool_plan(self, tools: &[ToolSpec]) -> Result<ToolPlan, ToolPlanError>;
fn adapt_history(self, items: Vec<ResponseItem>) -> Vec<ResponseItem>;
fn decode_tool_name(self, plan: &ToolPlan, wire_name: &str) -> ToolName;
fn transport_kind(self) -> ResponseTransportKind;
```

The exact Rust signatures may differ. The important point is that all wire
policy decisions have one owner.

### 7.2 Variant Behavior

#### ResponsesNative

- preserve namespaces;
- preserve custom/freeform tools;
- allow Responses Lite when the model and provider support it;
- allow tool search and built-in tools when supported;
- do not flatten tool names;
- do not adapt canonical history for function-only compatibility.

#### ResponsesFunctionOnly

- keep ordinary functions;
- flatten namespaces;
- hide custom/freeform tools;
- disable Responses Lite;
- hide tool search and built-in tools;
- decode flattened function names on response;
- adapt history before sending.

#### ChatCompletionsFunctionOnly

- same tool policy as ResponsesFunctionOnly;
- translate canonical messages to Chat messages;
- translate function calls and outputs to Chat tool calls and tool messages;
- parse Chat SSE;
- decode flattened function names on response.

#### AnthropicMessages

Reserved. It must reuse:

- `CanonicalRequest`;
- `CanonicalEvent`;
- `ToolPlan`;
- `WireCapabilities`.

It must not introduce Anthropic-specific types into core session state.

---

## 8. Request Flow

### 8.1 ResponsesNative

```text
Prompt + ResponseItem history
  -> existing Responses request builder
  -> Responses transport
  -> existing ResponseEvent stream
  -> session/tool router
```

This path must remain behavior-preserving.

### 8.2 ResponsesFunctionOnly

```text
Prompt + ResponseItem history
  -> WireAdapter::ResponsesFunctionOnly
  -> ToolPlan::new(..., function_only())
  -> adapt history copy
  -> build Responses request with flattened tools
  -> Responses transport
  -> decode flattened FunctionCall names
  -> canonical ResponseEvent stream
  -> session/tool router
```

### 8.3 ChatCompletionsFunctionOnly

```text
Prompt + ResponseItem history
  -> WireAdapter::ChatCompletionsFunctionOnly
  -> ToolPlan::new(..., function_only())
  -> adapt history copy
  -> CanonicalRequest
  -> ChatCompletionsClient
  -> Chat HTTP/SSE
  -> decode flattened FunctionCall names
  -> canonical ResponseEvent stream
  -> session/tool router
```

### 8.4 AnthropicMessages

```text
Prompt + ResponseItem history
  -> WireAdapter::AnthropicMessages
  -> ToolPlan::new(..., anthropic_capabilities())
  -> CanonicalRequest
  -> Anthropic adapter
  -> Anthropic HTTP/SSE
  -> canonical ResponseEvent stream
  -> session/tool router
```

---

## 9. Response Flow and Tool Name Decoding

Request-side adaptation is only half of the design. Response-side decoding is
required for correct tool routing.

The preferred insertion point is the canonical response stream mapping path in
`codex-rs/core/src/client.rs`, before events reach the session/tool router.

Conceptually:

```text
provider raw event
  -> ResponseEvent
  -> if FunctionCall:
       plan.decode_flat_name(wire_name)
  -> canonical ResponseItem
  -> session/tool router
```

Rules:

- `ResponsesNative` passes no decoder and preserves names as-is.
- `ResponsesFunctionOnly` passes the active `ToolPlan`.
- `ChatCompletionsFunctionOnly` passes the active `ToolPlan`.
- Unknown wire names fall back to a plain `ToolName`.
- Known wire names must decode before any tool routing decision.
- Tool result IDs must remain unchanged.

This keeps the tool router and session logic wire-agnostic.

---

## 10. Configuration

### 10.1 Transitional Configuration

Current transitional form:

```toml
[model_providers.example.http_headers]
x-codex-response-adapter = "chat_completions"
```

Properties:

- consumed locally;
- stripped before building provider HTTP headers;
- validated by `ModelProviderInfo::validate`;
- does not overload `wire_api`;
- does not restore `wire_api = "chat"`.

### 10.2 Target Configuration

After `WireAdapter` stabilizes, introduce a fork extension configuration layer.

Preferred shape:

```toml
[model_providers.example.extensions]
wire_adapter = "responses_function_only"
```

The fork config layer maps this to:

```rust
ProviderCapabilities {
    wire_adapter: WireAdapter::ResponsesFunctionOnly,
    ..
}
```

Do not add a new field directly to upstream `ModelProviderInfo` until the
extension shape is proven. Doing so creates a large mechanical diff and makes
future upstream cherry-picks harder.

### 10.3 Defaults

Recommended defaults:

| Provider | Adapter |
| --- | --- |
| OpenAI Responses | `ResponsesNative` |
| Azure Responses | `ResponsesNative` |
| Generic Responses | `ResponsesFunctionOnly` |
| Chat Completions | `ChatCompletionsFunctionOnly` |
| Anthropic | not selectable in V2 first milestone |

The default must be fail-closed. Unknown adapter values must be rejected.

---

## 11. History and Wire-Switching Invariants

The following invariants are mandatory:

1. Internal history is always canonical `ResponseItem`.
2. Wire-specific history shapes never become persisted canonical history.
3. Switching wire between turns preserves message order.
4. Switching wire preserves `FunctionCall.call_id`.
5. Switching wire preserves `FunctionCallOutput.call_id`.
6. Switching wire preserves tool result association.
7. Namespace identity survives flatten/decode round trips.
8. Chat `tool_calls` never leak into a Responses request.
9. Responses namespace shapes never leak into a function-only wire request.
10. Unsupported tools fail closed or are hidden; they are not silently
    converted into unrelated tools.
11. Request adaptation operates on a copy and does not rewrite stored history.
12. Subagent and `AgentMessage` delivery must survive function-only wires.

---

## 12. Error Handling

Adapters must fail closed.

Examples:

- unsupported custom tool and no fallback: hide the tool or return a structured
  adapter error;
- flattened-name collision: fail tool-plan construction;
- unknown adapter name: reject provider validation;
- invalid Chat request shape: return an invalid-request error before transport;
- malformed SSE event: skip only when the event is clearly irrelevant; do not
  silently continue past terminal protocol errors;
- context-length and quota errors: normalize into existing `ApiError` variants;
- auth errors: preserve the existing provider recovery path.

---

## 13. Testing Strategy

### 13.1 Pure Model-Wire Tests

Must cover:

- function-only capability filtering;
- namespace flattening;
- collision detection;
- reversible tool identity;
- history adaptation;
- plaintext and encrypted agent messages;
- Chat request construction;
- Chat message/tool grouping.

### 13.2 Adapter Tests

Must cover:

- Chat SSE text delta;
- split tool-call argument deltas;
- multiple tool calls;
- flattened-name decoding;
- `[DONE]`;
- context/quota/overload error normalization.

### 13.3 Core Integration Tests

Must cover:

- ResponsesNative behavior remains unchanged;
- ResponsesFunctionOnly request shape;
- Chat Completions request shape;
- canonical Chat tool history encoding;
- response-side tool-name decoding;
- Chat -> ResponsesFunctionOnly resume;
- ResponsesFunctionOnly -> Chat resume;
- subagent delivery on function-only wires;
- no wire-specific history leakage.

### 13.4 Acceptance Tests

A generic function-only provider must be able to call at least:

```text
exec_command
spawn_agent
send_message
followup_task
wait_agent
list_agents
edit_file
read_file
write_file
```

A tool call made on one wire must remain usable after switching to another
wire on a later turn.

---

## 14. Migration Plan

### Stage 1: WireAdapter Policy

- promote `WireAdapter` from a tag into a policy object;
- add `transport_kind()`;
- add tool-plan and history-adaptation helpers;
- keep existing Chat behavior passing.

### Stage 2: ResponsesFunctionOnly

- add provider capability selection;
- build function-only Responses requests with `ToolPlan`;
- adapt history on the request copy;
- add request-shape tests.

### Stage 3: Response Decoding

- carry the active `ToolPlan` through the response mapping path;
- decode `FunctionCall` names before tool routing;
- add round-trip tests.

### Stage 4: Cross-Wire Conformance

- port or reimplement the old MultiAdapt conformance tests;
- cover Chat <-> ResponsesFunctionOnly;
- cover ResponsesNative <-> function-only;
- cover subagent delivery.

### Stage 5: Anthropic Messages

- add a separate adapter crate;
- reuse canonical types and `ToolPlan`;
- do not modify core session state;
- add provider selection and conformance tests.

### Stage 6: First-Class Configuration

- replace the reserved header with the fork extension config layer;
- keep the old header as a compatibility alias if needed;
- document migration from the transitional form.

---

## 15. Cherry-Pick and Upstream Discipline

This fork must keep upstream sync practical.

Rules:

1. `main` remains an upstream mirror and must not receive fork changes.
2. Do not fetch, pull, checkout, or reset `main` while the current migration is
   in progress unless the user explicitly asks.
3. Prefer new extension crates over edits to upstream modules.
4. Keep core edits small and isolated in their own commits.
5. Avoid mechanical changes to upstream structs when a fork extension layer can
   carry the same information.
6. Do not restore removed historical wire semantics.
7. When upstream changes touch an extension seam, adapt the extension rather
   than moving fork-specific behavior deeper into upstream code.

The current reserved header is a deliberate consequence of rule 5. It is not
the final configuration design.

---

## 16. Acceptance Criteria

V2 is complete when:

- `WireAdapter` is the single provider-owned wire policy;
- ResponsesNative behavior is preserved;
- ResponsesFunctionOnly works end to end;
- ChatCompletionsFunctionOnly works end to end;
- tool identity survives flatten/decode round trips;
- response-side tool names are decoded before routing;
- history remains canonical across wire switches;
- subagent delivery works on function-only wires;
- conformance tests cover both directions of wire switching;
- the transitional configuration has a documented replacement path;
- no upstream `main` sync is required to complete the migration.

---

## 17. Open Questions

1. Should `WireAdapter` live in `codex-mycode-model-wire` or a new
   `codex-mycode-wire-policy` crate?
2. Should the active `ToolPlan` be stored per request or per session?
3. Should unknown flattened names fail the turn or fall back to a plain tool
   name?
4. Should Anthropic expose a different capability set for tool-use blocks?
5. Should the reserved header remain as a compatibility alias after the
   extension config layer lands?
6. Which upstream changes, if any, can be contributed back to reduce future
   cherry-pick conflicts?
