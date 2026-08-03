# G04 Desktop Host Lifecycle v1

本契约冻结 S07 的 Windows 桌面宿主边界。Tauri 只承载窗口、托盘、单实例、自启动、签名升级和白名单 command；Rust Core 仍是保险库与密钥的唯一持有者。

## 1. 宿主状态

```text
STARTING
  -> RECOVERING      打开保险库，恢复上次未闭合宿主会话并记录空缺
  -> BACKGROUND      `--background` 自启动；窗口隐藏，Core 继续运行
  -> FOREGROUND      手动启动；显示并聚焦主窗口

BACKGROUND <-> FOREGROUND
  WindowCloseRequested -> BACKGROUND（阻止退出并隐藏）
  TrayOpen | SecondInstance -> FOREGROUND（显示、取消最小化并聚焦）

BACKGROUND | FOREGROUND
  -> EXITING_EXPLICIT 显式退出
  -> EXITING_UPDATE   已下载并验签的升级准备安装
  -> FAILED_CLOSED    Core 安全关闭后升级安装失败且重新打开失败

EXITING_*
  -> 记录宿主会话终点与原因
  -> checkpoint WAL
  -> close SQLCipher
  -> zeroize Vault Key
  -> release writer lock
  -> STOPPED | updater install + relaunch
```

关闭步骤必须全部尝试并按执行顺序返回完整错误集合；窗口隐藏、WebView 卸载或前端异常不能触发 Core 关闭。

## 2. 单实例与自启动

- `tauri-plugin-single-instance` 必须是首个注册插件。第二实例不得打开保险库或创建第二个 Core；它只通知首实例显示并聚焦主窗口后退出。
- `tauri-plugin-autostart` 只注册当前用户，参数固定为 `--background`。WebView 只能调用 `get_autostart` 与 `set_autostart(enabled)` 白名单 command，不能获得插件通用权限。
- 手动启动默认进入 `FOREGROUND`；登录自启动默认进入 `BACKGROUND`。两者使用同一非提权可执行文件，不部署 Windows Service 或独立 `core-host.exe`。

## 3. 签名升级

- 只使用 `tauri-plugin-updater` 的 HTTPS endpoint、内嵌公钥和强制签名验证；私钥不进入仓库、应用配置或运行时。
- 检查和下载升级时 Core 保持运行。下载完成后，本人明确安装才进入 `EXITING_UPDATE`；Core 安全关闭后才安装并重启。
- 安装失败时先尝试用 DPAPI 重新打开 Core 并建立新的宿主会话；重新打开也失败则进入 `FAILED_CLOSED`，不得以无保险库状态伪装正常运行。
- 升级前后的宿主空缺标记为 `UPDATE`，不得伪造该区间的采集活动。

## 4. 崩溃与空缺

```text
begin_host_session(now, launch_mode):
  previous := latest_host_session()
  if previous exists and previous.ended_at is null:
    append HostRuntimeGap(previous.last_seen_at, now, CRASH)
  else if previous exists and previous.ended_at < now:
    append HostRuntimeGap(previous.ended_at, now, previous.end_reason)
  append HostSession(started_at=now, last_seen_at=now, launch_mode)

heartbeat(session_id, now):
  require session is current and open
  update last_seen_at=now

finish_host_session(session_id, now, reason):
  require session is current and open
  update last_seen_at=now, ended_at=now, end_reason=reason
```

宿主每 30 秒提交一次心跳。崩溃空缺从最后一次已提交心跳开始，因此上界误差不超过一个心跳周期；若系统时钟回退，空缺区间收敛为零长度且记录时钟异常，不生成负区间。

## 5. WebView 能力

允许的 S07 command 只有：

```text
get_host_status() -> HostStatusView
list_conversation() -> ConversationTurnView[]
send_message(verbatim) -> ConversationTurnResult
get_autostart() -> bool
set_autostart(enabled) -> bool
check_update() -> UpdateAvailability
install_update() -> never | UpdateInstallError
exit_application() -> never | ShutdownError
```

S28 只在同一白名单边界追加：

```text
get_capture_status() -> CaptureStatusView
list_activity_timeline() -> ActivityTimelineEntryView[]
set_capture_paused(paused) -> CaptureStatusView
```

采集查询只返回有界活动元数据和有原因空缺；暂停/恢复不能获得 Win32、数据库或密钥句柄。

命令参数和返回值只能是有界结构化数据。WebView 不接收 Vault Key、Recovery Key、数据库或 repository 句柄、模型 bearer token、任意路径、通用文件 API、shell、HTTP 或进程能力。

## 6. Windows 测试方案

| 场景 | 确定性判据 |
| --- | --- |
| 关闭窗口 | `CloseRequested` 被阻止，窗口隐藏，宿主会话保持开放且后续心跳成功。 |
| 第二实例 | 第二实例不取得 writer lock；首实例收到激活事件并显示、聚焦现有窗口。 |
| 显式退出 | 即使 checkpoint 或 close 报错，后续清零与释放仍执行；最终不再接受 command。 |
| 崩溃恢复 | 未闭合会话在重启时产生一条 `CRASH` 空缺，起点为最后心跳，且只产生一次。 |
| 升级 | 只有已签名下载和本人确认可进入 `EXITING_UPDATE`；重启后产生 `UPDATE` 空缺。 |
| 对话重启 | 双方逐字发言重启后仍可读取；普通问答不自动产生任何账本 Claim。 |
| WebView 边界 | capability 与 invoke handler 均不存在通用文件、shell、HTTP、数据库、密钥或运行时凭据入口。 |
| 活动采集 | 连续同活动合并；空闲、窗口切换、暂停、锁屏、源不可用和退出产生确定性边界，且只返回最小元数据。 |
| 采集恢复 | 崩溃后活动只保留至最后观测点，之后为 `CRASH` 空缺；关窗隐藏期间采集线程与宿主会话继续。 |

纯生命周期转换和 SQLCipher 重启/故障注入由 Rust 自动化测试覆盖；Tauri 窗口、托盘、单实例与自启动在 Windows 上使用打包后的 smoke harness 验证，前端使用类型检查与组件测试验证 command 调用和错误呈现。
