# 远程 Pi Hub 加载性能优化方案

- 适用项目：`Pi Hub Client`
- 方案状态：提案
- 目标平台：macOS 14+、iOS 17+
- 关联设计：`docs/design-v1.md`、`docs/design-v2.md`

## 1. 目标与结论

继续采用“远程 Pi Hub + 本地可信 App Shell”的架构，不把 Node.js、Next.js 或 Pi Hub Runtime 打包进 iOS App。

目标是降低以下两类延迟：

1. SSH Forward 场景下，Pi Hub 页面首次加载及静态资源加载延迟；
2. 页面已经打开后，API 请求、SSE/WebSocket 和交互响应延迟。

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

## 4. 第一阶段：可观测性

### 4.1 客户端计时

在 Trusted App Shell 和 Rust 连接核心增加非敏感性能事件，仅记录：

- 连接类型：`direct_url` / `ssh_forward`；
- 阶段：DNS、SSH connect、host key、authentication、forward、viewer load；
- 开始时间、结束时间、耗时；
- 失败错误码；
- SSH channel 建立次数和失败次数。

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
- 确认主要瓶颈位于 SSH、远端 Pi Hub 还是 WebView。

### Phase 1：低风险优化

- 修正明显的重复请求；
- 优化 Pi Hub 静态资源缓存和压缩；
- 验证 HTTP Keep-Alive；
- 优化移动端首屏资源。

### Phase 2：SSH 优化

- 在保持安全约束的前提下优化 channel 并发和排队；
- 增加连接异常、取消、重连回归测试；
- 重新运行真机性能矩阵。

### Phase 3：验收

- SSH Forward 5G 首屏 p95 较基线下降至少 30%；
- Direct HTTPS 不出现性能回归；
- 首次加载和热加载均不泄露敏感信息；
- iOS 17+ 与 macOS 14+ 编译、测试和真机回归通过。

## 9. 交付物

- 本文档对应的性能基线记录；
- 客户端脱敏性能诊断；
- SSH Forward 优化及测试；
- `pi-hub` 静态资源和接口性能优化；
- iPhone 5G/Wi-Fi 与 macOS 回归记录。
