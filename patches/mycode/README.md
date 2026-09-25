# MyCode v2 patch series

上游基线: `75e0e0aad97a86138b8b1ec87d9b544b4a35ecbf`

依次应用 0001 → 0002 → 0003 可完整回放 fork 状态（wire 适配 + rwe 文件工具 + 遥测阻断 + 版本号）。

## 用法

```bash
git checkout 75e0e0aad97a86138b8b1ec87d9b544b4a35ecbf
git apply -3 patches/mycode/0001-mycode-crates.patch        # 纯新增路径，零冲突
git apply -3 patches/mycode/0002-mycode-seam.patch          # wire/tools 的 seam，风险集中在此
git apply -3 patches/mycode/0003-mycode-commercial.patch    # 遥测阻断
cd codex-rs && cargo metadata --format-version 1 >/dev/null # 刷新 Cargo.lock
just write-config-schema
just write-app-server-schema && just write-app-server-schema --experimental
```

## 分层

| 层 | 内容 | 文件数 | 冲突风险 |
| --- | --- | --- | --- |
| 0001 | 新增 crate / 新模块 / 设计文档 / 验证脚本 | 34 | 零（上游无这些路径） |
| 0002 | wire 适配 + rwe 文件工具 + 版本号（改上游文件） | 86 | 高（seam 都在这里） |
| 0003 | 遥测阻断（改上游文件） | 22 | 中 |

## 变更来源提交

- 3a7bf9658 feat(mycode): replay wire adapter and file tools onto upstream main
- 4ccacf79b chore(mycode): bump fork version to 26.157.0
- 34d4c62d2 feat(mycode): add fork telemetry policy crate
- f3d751e18 fix(mycode): restore codex-analytics test compilation
- 6a57c67d9 feat(mycode): block commercial telemetry at its egress points
- 4749c4ac5 test(mycode): adapt feedback tests to the blocked upload path
