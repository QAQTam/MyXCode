# MyCode v2 patch series

上游基线: `75e0e0aad97a86138b8b1ec87d9b544b4a35ecbf`

应用 0001 + 0002 到该基线上，可完整回放 fork 状态（wire 适配 + rwe 文件工具 + 版本号）。

## 用法

```bash
git checkout 75e0e0aad97a86138b8b1ec87d9b544b4a35ecbf
git apply -3 patches/mycode/0001-mycode-crates.patch   # 纯新增路径，零冲突
git apply -3 patches/mycode/0002-mycode-seam.patch     # 唯一需要解冲突的一层
cd codex-rs && cargo metadata --format-version 1 >/dev/null   # 刷新 Cargo.lock
just write-config-schema
just write-app-server-schema && just write-app-server-schema --experimental
```

## 分层说明

| 层 | 内容 | 文件数 | 冲突风险 |
| --- | --- | --- | --- |
| 0001 | 新增 crate / 新模块 / 设计文档 | 31 | 零（上游无这些路径） |
| 0002 | 上游已有文件的 seam 改动 | 86 | 全部风险集中在此 |

## 变更来源提交

- 3a7bf9658 feat(mycode): replay wire adapter and file tools onto upstream main
- 4ccacf79b chore(mycode): bump fork version to 26.157.0
