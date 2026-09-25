# MyCode v2 patch series

基线 (upstream main): 3a7bf9658ae8d01f89983ae85c01ab918d90cde0
生成时间: 2026-09-25T02:41:50Z

## 用法

```bash
git checkout <new-upstream-main>
git apply -3 patches/mycode/0001-mycode-crates.patch   # 纯新增，永不冲突
git apply -3 patches/mycode/0002-mycode-seam.patch     # 唯一需要解冲突的一层
just write-config-schema
just write-app-server-schema && just write-app-server-schema --experimental
```

## 分层说明

| 层 | 文件 | 冲突风险 |
| --- | --- | --- |
| 0001 | 新增 crate / 新模块 / 设计文档 | 零（上游不存在这些路径） |
| 0002 | 上游已有文件的 seam 改动 | 全部风险集中在此 |
