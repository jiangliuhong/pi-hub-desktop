# 远程 Pi Hub 加载性能优化方案

- 适用项目：`Pi Hub Client`
- 方案状态：实施中（§5.4–§5.6 连接可靠性已实现；§4 性能可观测性、§5.2 SSH channel 并发优化未开始）
- 目标平台：macOS 14+、iOS 17+
- 关联设计：`docs/design-v1.md`、`docs/design-v2.md`

## 实施记录（§5.4–§5.6 连接可靠性）

已实现范围与对应代码：

- **§5.4 会话存活与失效检测**：`Handler::disconnected` 覆盖 + 新增
  `src-tauri/src/ssh/health.rs`（`HealthMonitor`/`HealthHandle`，基于
  `tokio::watch`，first-reason-wins）。`HealthHandle` 通过 `ConnectionResources`
  透传到连接层。keepalive（25s × max 3）保持不变，已有但原本无人观测。
- **§5.5 自动重连与 Viewer 恢复**：`ConnectionManager` 新增 generation
  (`AtomicU64`)、reconnect supervisor（`Supervisor { cancel, join }`，复刻
  `LocalForward` 模式）、`1s→2s→4s→8s→15s→30s` 退避、不可重试错误立即 `Error`。
  重连仅对 SSH Forward 生效；direct URL 不重连。新增 `ConnectionBroadcaster` DI
  模式（`NoopBroadcaster`/`TauriBroadcaster`，复刻 V2 `StatusBroadcaster`），
  在 `.setup` 注入 AppHandle 后发射 `connection://state-changed`。前端
  `ViewerToolbar` 订阅该事件并在 effective URL 变化时 reload iframe（§5.5.4），
  `ConnectionPage` 反映 `reconnecting` 状态（§5.5.1）。
- **§5.5 iOS 生命周期**：`AppLifecycle`（`#[repr(u8)]`，Foreground=0/Background=1）
  存于 `ManagerInner.lifecycle`（`AtomicU8`），重连循环在 Background 时暂停、
  Foreground 时恢复；通过 Tauri v2 `WindowEvent::Resumed`/`Suspended`（mobile-only）
  驱动 `set_lifecycle`。
- **§5.6 稳定错误码**：新增 `SshKeepaliveTimeout`/`SshTransportClosed`/
  `SshChannelOpenFailed`/`NetworkPathChanged`/`ForegroundSessionInvalid`/
  `ViewerReloadFailed`；`SshError::SessionClosed { SessionCloseReason }` 把
  mid-session 关闭映射到可重试码（keepalive timeout 独立区分）。
- **App 退出**：`ConnectionManager::on_app_exit` 在 `RunEvent::ExitRequested` 中
  bounded（4s）teardown 所有连接。

未实现范围（后续阶段）：

- **§4 性能可观测性埋点**（客户端/Service View 计时、基线矩阵）。
- **§5.2 SSH channel 并发锁优化**（`forward.rs` 互斥锁范围缩减）——方案要求先有 §4
  数据再做。
- **网络路径变化（`NetworkPathChanged`）作为主动重连触发器**：当前以 health 失效
  为主触发；iOS 可达性监听留作后续增强。

已执行的检查：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、
`cargo test --lib`（177 通过 / 2 预存环境相关 V2 detector 失败，与本次无关）、
`cargo test --test ssh_forward`（5 通过）、`npm run lint`、`npm run typecheck`、
`npm test`（66 通过）、`npm run build` 均通过。


## 1. 目标与结论

继续采用“远程 Pi Hub + 本地可信 App Shell”的架构，不把 Node.js、Next.js 或 Pi Hub Runtime 打包进 iOS App。

目标是降低以下两类延迟，并解决一类连接可靠性问题：

1. SSH Forward 场景下，Pi Hub 页面首次加载及静态资源加载延迟；
2. 页面已经打开后，API 请求、SSE/WebSocket 和交互响应延迟。
3. SSH Forward 已连接后，空闲或持续使用一段时间发生无提示断开，且 Viewer 不能自动恢复。

第 3 项当前已在 iOS 观察到；macOS 尚未完成复现，必须使用同一套 Rust SSH 核心一起排查和修复。iOS 进入后台后仍不承诺持续在线，本方案要求的是前台稳定存活，以及从后台回到前台后的确定性恢复。

Direct URL 和 SSH Forward 继续共用现有连接模型。远程 Pi Hub 页面仍被视为不可信 Service View，不获得 Tauri、Keychain、文件系统或本地 Runtime 权限。

## 2. 当前链路

```text
App Shell
  └─ Rust ConnectionManager
      ├─ Direct URL → 远程 Pi Hub HTTPS/HTTP
      └─ SSH Forward
          ├─ SSH Session
          ├─ 127.0.0.1:随机端口
          └─ SSH direct-tcpip → target_host:target_port

Service View
  └─ iframe 加载 effective_url
      ├─ SSH 场景：127.0.0.1:随机端口
      └─ Direct 场景：远程 Pi Hub URL
```

手机使用 5G 不会消除 SSH 握手、DNS、TLS、远端服务处理和多资源 TCP 建连产生的往返延迟。SSH 场景下，页面的每个资源和接口请求都可能经过同一条 SSH 隧道。

## 3. 性能假设

需要先用指标验证，不直接假设是 5G 带宽问题：

| 假设 | 现象 | 验证方式 |
|---|---|---|
| SSH channel 建立串行 | 首次加载资源较多的页面明显变慢 | 统计 channel open 次数和耗时 |
| 远端静态资源未命中缓存 | 每次新连接都重新下载 JS/CSS | 查看资源传输大小、Cache-Control 和 ETag |
| Pi Hub API 或 SSE 响应慢 | 页面框架出现但数据和交互延迟 | 统计 API 首字节和完整响应耗时 |
| WebView 主线程繁忙 | 网络请求已结束但点击、滚动仍卡顿 | 使用 Safari Web Inspector 查看 Long Task |
| SSH 服务器或目标服务处理慢 | Direct URL 快、SSH Forward 慢 | 同一设备对比两种连接方式 |
| SSH transport 空闲后被 NAT、运营商或 SSH Server 回收 | 前台静置一段时间后，下一次请求失败 | 记录最后一次成功 keepalive、transport close 原因和服务端空闲策略 |
| keepalive 已配置但失效未进入状态机 | 本地 Listener 仍存在、UI 仍显示已连接，但新 channel 无法打开 | 对比 SSH session、Listener、channel open 和 ConnectionManager 状态时间线 |
| iOS 挂起或网络切换后沿用失效 session | 回前台或 Wi-Fi/蜂窝切换后页面卡住 | 记录前后台、网络变化、session generation 和重连结果 |
| 重连后 loopback 端口变化但 Viewer 仍使用旧 URL | Rust 已重连，页面仍访问旧端口 | 对比 reconnect 前后 effective URL、Viewer reload 和首个成功请求 |

## 4. 第一阶段：可观测性

### 4.1 客户端计时

在 Trusted App Shell 和 Rust 连接核心增加非敏感性能事件，仅记录：

- 连接类型：`direct_url` / `ssh_forward`；
- 阶段：DNS、SSH connect、host key、authentication、forward、viewer load；
- 开始时间、结束时间、耗时；
- 失败错误码；
- SSH channel 建立次数和失败次数。
- keepalive 发送、确认、连续失败次数和最后成功时间；
- SSH transport 关闭来源：远端关闭、本地取消、超时、网络错误或未知；
- connection generation、重连次数、退避耗时和重连后首个 channel 成功时间；
- App 前台/后台切换时间和网络路径变化时间。

禁止记录 URL 查询参数、凭据、Cookie、Authorization、页面正文和对话内容。

### 4.2 Service View 计时

在 `ViewerToolbar` 记录：

- iframe 开始加载时间；
- `load` 事件时间；
- 可选的页面端 `DOMContentLoaded` 与首个可交互标记。

计时只用于本地诊断，不上传外部服务。

### 4.3 验收基线

至少采集以下矩阵：

| 设备 | 网络 | 连接方式 | 页面类型 |
|---|---|---|---|
| iPhone 14 Pro | 5G | SSH Forward | 首页 |
| iPhone 14 Pro | Wi-Fi | SSH Forward | 首页 |
| iPhone 14 Pro | 5G | Direct HTTPS | 首页 |
| macOS | Wi-Fi | SSH Forward | 首页 |

每种组合至少执行 5 次冷启动和 5 次热启动，记录 p50/p95。

长连接另建稳定性基线，不与首屏 p50/p95 混在一起：

| 平台 | 网络与生命周期 | 场景 | 最低观察时长 |
|---|---|---|---:|
| iOS 真机 | Wi-Fi、前台 | 页面空闲后继续请求 | 60 分钟 |
| iOS 真机 | 蜂窝网络、前台 | 页面空闲后继续请求 | 60 分钟 |
| iOS 真机 | Wi-Fi ↔ 蜂窝 | 页面打开时切换网络 | 每方向至少 5 次 |
| iOS 真机 | 前台 → 后台 → 前台 | 分别停留 1、5、15 分钟 | 每档至少 3 次 |
| macOS 真机 | Wi-Fi、App 保持运行 | 页面空闲与持续交互 | 4 小时 |

每次记录断开前最后一次成功活动、是否仍有 Listener、SSH session 是否关闭、失败 channel 数、状态转换、恢复耗时和 Viewer 是否重新加载。不得只以页面仍显示或 loopback 端口仍在监听判定连接存活。

## 5. 第二阶段：SSH Forward 优化

### 5.1 保持安全边界

- 监听地址仍固定为 `127.0.0.1`；
- 端口仍由系统分配；
- 不监听 `0.0.0.0`；
- 不把 SSH Session、凭据或 Tauri API 暴露给 Service View；
- Host Key 校验和连接取消行为不改变。

### 5.2 优化 channel 建立

当前 `src-tauri/src/ssh/forward.rs` 在打开 `direct-tcpip` channel 时使用共享 SSH handle 的互斥锁。下一步应：

1. 先用计时确认该锁是否造成明显排队；
2. 在 russh API 允许的前提下缩短锁持有范围或支持安全并发打开 channel；
3. 对并发 channel 设置上限，避免页面资源突发请求耗尽 SSH Server；
4. 保留单个 channel 的独立失败和取消语义。

不能通过简单移除锁来换取速度，必须验证 russh handle 的并发安全和 SSH Server 的兼容性。

### 5.3 连接复用

优先依赖浏览器和远端 HTTP 的 Keep-Alive。不要在客户端自行缓存包含认证信息的业务响应。

如果实测页面仍大量创建短连接，再评估：

- 远端 Pi Hub 是否启用 HTTP Keep-Alive；
- SSH Server 是否支持足够的并发 channel；
- 是否需要面向 Pi Hub 的受控连接池。

### 5.4 SSH 会话存活与失效检测

当前 Rust SSH 配置已经设置约 25 秒 keepalive 和连续失败阈值，但方案不能把“配置了 keepalive”等同于“ConnectionManager 能感知并恢复断线”。实现阶段必须确认 `russh` 的实际语义，并补齐：

1. SSH transport 正常关闭、异常关闭、keepalive 超阈值和新 channel 打开失败都能通知 `ConnectionManager`；
2. 本地 Listener 存在不代表 SSH 可用，状态不得长期错误停留在 `connected`；
3. 单个 forwarding channel 失败先按 channel 级错误处理；只有 transport 已失效或连续探测失败时才触发 session 级重连；
4. 活跃 HTTP、SSE 或 WebSocket 流量与空闲页面都必须覆盖，不能依靠业务请求充当唯一保活；
5. keepalive task、transport monitor 和 reconnect task 归属同一个 connection generation，主动断开、Viewer 关闭或 App 退出后必须全部取消。

keepalive 周期和失败阈值先以现有值为基线，根据 iOS 5G/Wi-Fi 与 macOS 实测调整。不得为了掩盖断线使用高频保活造成明显耗电或移动数据消耗。

### 5.5 自动重连与 Viewer 恢复

SSH session 判定失效后：

1. Rust 状态必须从 `connected` 进入 `reconnecting`，前端不得继续显示已连接；
2. 前台按 `1s → 2s → 4s → 8s → 15s → 30s` 上限退避，认证失败、Host Key 变化和配置错误不得无限重试；
3. 每次重连创建新的 connection generation，先释放旧 SSH session、Listener 和 channel，再发布新的 effective URL；
4. 随机 loopback 端口变化后，Viewer 必须切换到新 URL 并重新加载，不能继续请求旧端口；
5. 重连成功必须以新 SSH session 上真实打开 `direct-tcpip` 并完成 Pi Hub 服务探测为准，不能只以 SSH authentication 成功为准；
6. 重连期间用户主动断开、返回列表或删除服务时立即停止退避，不得稍后复活连接；
7. 同一服务始终最多一个有效 SSH session 和一个 reconnect loop，旧 generation 的异步结果不得覆盖新状态。

iOS 生命周期规则：进入后台后停止激进重连，不承诺 SSH socket 存活；回到前台立即检查 transport 和 Listener，并在失效时发起一次不等待旧退避的重连。macOS 在 App 仍运行且 Viewer 打开时执行同样的异常断线恢复，但不引入 LaunchAgent 或退出后常驻。

### 5.6 稳定错误与诊断

至少区分以下非敏感结果，具体枚举名在实现设计中固定：

- `ssh_keepalive_timeout`：keepalive 连续失败；
- `ssh_transport_closed`：SSH transport 被关闭；
- `ssh_channel_open_failed`：单个 direct-tcpip channel 无法打开；
- `network_path_changed`：网络路径变化触发重新验证；
- `foreground_session_invalid`：iOS 回前台发现旧 session 失效；
- `viewer_reload_failed`：SSH 已恢复但 Viewer 未能加载新 effective URL。

诊断只记录时间、阶段、计数、平台、网络类型和脱敏错误码；不得记录请求正文、页面内容、Cookie、Authorization、URL query 或 SSH 业务数据。

## 6. 第三阶段：远端 Pi Hub 配合

不改变 Pi Hub 的业务语义，只优化静态资源和接口响应：

- JS/CSS/字体等带版本资源启用长期缓存；
- 启用 gzip 或 Brotli；
- 减少首屏必须加载的资源；
- 将非首屏数据改为懒加载；
- 检查 API、SSE 和 WebSocket 是否存在不必要的重复请求；
- 对移动端首屏提供更小的布局和资源集合。

这些改动应在 `pi-hub` 仓库完成，并通过现有 HTTP/API 契约验证。

## 7. 不采用的方案

### 7.1 不在 iOS App 内启动完整 Pi Hub

当前 Pi Hub 依赖 Node.js、Next.js、文件系统、AgentSession 和本机运行时。将其完整打包进 iOS 会引入运行时体积、沙盒、后台执行、数据目录和升级维护问题，也违反 V2 对 iOS 不提供本机 Runtime 的约束。

### 7.2 不把 SSH Forward 改成公网监听

不能为了让 WebView 直接访问而把本地转发监听到 `0.0.0.0`，也不能把凭据放入 URL。

### 7.3 不默认复制远端 Pi Hub 数据

页面缓存不能包含会话、凭据、模型数据或对话内容。任何缓存方案都必须限定为明确版本的非敏感静态资源。

## 8. 分阶段实施

### Phase 0：测量

- 增加本地性能计时和脱敏日志；
- 完成 iPhone 14 Pro 的 5G/Wi-Fi 对比；
- 复现并记录 iOS 前台空闲断线时间，核对 SSH Server 的 `ClientAliveInterval`、`ClientAliveCountMax`、TCP keepalive 和中继/NAT 空闲策略；
- 在 macOS 使用同一服务完成长连接对照，判断问题属于共享 Rust 核心还是 iOS 生命周期/网络路径；
- 确认主要瓶颈位于 SSH、远端 Pi Hub 还是 WebView。

### Phase 1：低风险优化

- 修正明显的重复请求；
- 优化 Pi Hub 静态资源缓存和压缩；
- 验证 HTTP Keep-Alive；
- 优化移动端首屏资源。

### Phase 2：SSH 优化

- 在保持安全约束的前提下优化 channel 并发和排队；
- 打通 keepalive/transport 失效通知、状态机、自动重连和 Viewer 新 URL 恢复；
- 增加空闲超时、远端断开、黑洞连接、网络切换、前后台、取消和重连回归测试；
- 重新运行真机性能矩阵。

### Phase 3：验收

- SSH Forward 5G 首屏 p95 较基线下降至少 30%；
- Direct HTTPS 不出现性能回归；
- iOS 在稳定 Wi-Fi 和蜂窝网络前台各空闲 60 分钟后，下一次操作仍成功；若测试环境强制回收连接，则 App 必须自动恢复且不需要用户返回服务列表重连；
- iOS 从后台回到前台、Wi-Fi/蜂窝切换后，失效连接在 15 秒内进入明确状态并开始恢复；网络可用且无需 Host Key/凭据交互时，应在 30 秒内恢复 Viewer；
- macOS 稳定网络 4 小时测试中不得出现 UI 假 `connected`；远端主动断开后必须自动进入 `reconnecting` 并恢复；
- 主动断开、删除服务和 App 退出后，不得残留 Listener、SSH session、keepalive 或 reconnect task；
- 首次加载和热加载均不泄露敏感信息；
- iOS 17+ 与 macOS 14+ 编译、测试和真机回归通过。

## 9. 交付物

- 本文档对应的性能基线记录；
- 客户端脱敏性能诊断；
- iOS/macOS 长连接断线复现记录、根因结论与稳定性基线；
- SSH Forward 优化及测试；
- `pi-hub` 静态资源和接口性能优化；
- iPhone 5G/Wi-Fi 与 macOS 回归记录。
