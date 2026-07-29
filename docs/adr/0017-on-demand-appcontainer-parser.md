---
status: superseded by ADR-0023
---

# 文档由按需 AppContainer 进程解析

首版使用 Rust `parser-host.exe` 逐文件按需解析：Core 在原件归档后通过匿名管道传入内容，解析器在无网络能力的 Windows AppContainer 中运行并由 Job Object 限制资源，返回受限的结构化结果后退出。解析器不获得保险库路径、密钥、数据库句柄或来源路径。

## Considered Options

- Core 进程内解析：部署最简单，但解析漏洞会直接进入持钥进程，违反恶意文档威胁边界。
- Apache Tika/Java 进程：格式覆盖广，但首版引入大型运行时和更宽依赖面。
- WebAssembly 解析器：能力隔离清晰，但会显著限制首批原生解析库选择。
- Windows 原生按需沙箱：兼顾隔离、Rust 解析库和首版平台范围，采纳。

## Consequences

AppContainer 创建、Job Object 约束或管道握手失败时必须拒绝解析，不得回退到 Core 内执行。解析超时、崩溃和资源超限只改变解析状态，已归档原件继续保留；明文内容不写入临时文件或运行日志。
