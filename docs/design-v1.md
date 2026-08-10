# Pi Hub Client V1 技术设计

- 对应需求：`docs/requirements-v1.md`
- 技术状态：设计基线，Phase 0 验证通过后进入正式实现
- 目标平台：macOS 14+、iOS 17+

---

## 1. 设计目标

V1 用同一个跨端工程实现 macOS 和 iOS 客户端，并支持：

```text
Direct URL
    └── App WebView → Pi Hub URL

SSH Forward
    └── App loopback → SSH direct-tcpip → Pi Hub / existing relay target
```

设计重点不是重新实现 Pi Hub，而是建立可靠、安全、可扩展的连接层。

核心原则：

1. Pi Hub 业务与客户端连接能力解耦。
2. React UI 不直接持有 SSH Session。
3. Rust 侧是连接状态与资源生命周期的唯一真实来源。
4. 远端 Pi Hub 页面按不可信 Web 内容处理。
5. macOS 与 iOS 共享领域逻辑，平台差异集中在适配层。
6. V1 不提前开发专用 Relay Protocol。

---

## 2. 技术选型

| 层 | 选型 | 原因 |
|---|---|---|
| 跨端容器 | Tauri 2 | 同一工程支持 macOS / iOS，使用系统 WKWebView |
| UI | React + TypeScript + Vite | 适合配置型 UI，开发和测试成本低 |
| 连接核心 | Rust + Tokio | 异步网络、生命周期与资源管理清晰 |
| SSH | `russh` | 支持异步 SSH Client 和 `direct-tcpip` |
| 非敏感配置 | `tauri-plugin-store`，仅由可信 App Shell / Rust 使用 | macOS / iOS 均支持，适合 V1 配置量 |
| 敏感凭据 | Apple Keychain，通过 Rust `security-framework` 抽象 | 同时支持 macOS / iOS，不落明文文件 |
| 日志 | Rust `tracing` + 脱敏层 | 结构化诊断，不记录秘密和业务内容 |
| 测试 | Vitest / React Testing Library + Rust unit/integration test | 覆盖 UI、状态机和网络核心 |

依赖版本在项目初始化时固定到当时稳定兼容版本，并提交 `package-lock.json` 与 `Cargo.lock`。

---

## 3. 系统上下文

```text
┌─────────────────────────────────────────────────────────┐
│ Pi Hub Client                                           │
│                                                         │
│  ┌─────────────────┐       ┌─────────────────────────┐ │
│  │ Trusted App UI  │       │ Untrusted Service View  │ │
│  │ Services/Config │       │ Existing Pi Hub WebUI   │ │
│  └────────┬────────┘       └────────────┬────────────┘ │
│           │ Tauri commands              │ HTTP(S)       │
│  ┌────────▼─────────────────────────────▼────────────┐  │
│  │ Rust Core                                         │  │
│  │ ProfileStore / CredentialStore / ConnectionManager│  │
│  └───────────────┬──────────────────────┬────────────┘  │
└──────────────────┼──────────────────────┼───────────────┘
                   │                      │
             Direct HTTPS          SSH direct-tcpip
                   │                      │
                   ▼                      ▼
            Public/LAN Pi Hub       SSH Server / Relay
                                          │
                                          ▼
                                  target_host:target_port
```

---

## 4. 运行时组件

### 4.1 Trusted App Shell

本地打包的 React 页面，负责：

- 服务列表
- 服务配置
- 凭据录入
- Host Key 确认
- 连接进度和诊断
- 打开、关闭和恢复 Service View

只有 App Shell 可以调用经过最小授权的 Tauri commands。

### 4.2 Rust Core

Rust Core 负责：

- Service Profile 校验和持久化
- Apple Keychain 访问
- SSH Host Key 校验
- SSH Session 和 Local Forward
- 连接状态机
- 重连和取消
- Service View 生命周期控制
- 结构化事件和错误

### 4.3 Service View

Service View 加载远端或 loopback Pi Hub WebUI。

它不属于可信前端，不能获得 Tauri API、Keychain、Store、Shell 或文件系统权限。

### 4.4 Existing SSH Relay

V1 把 SSH Relay 当作普通 SSH Server 使用：

```text
ssh_host:ssh_port
  └── target_host:target_port
```

目标可以是：

- SSH Server 自己的 `127.0.0.1:30142`
- SSH Server 局域网内的 Pi Hub
- 已由 Reverse Tunnel 暴露在 SSH Server 上的端口

V1 不管理 Reverse Tunnel 的创建、认证、分配和保活。

---

## 5. 推荐仓库结构

```text
pi-hub-desktop/
├── src/
│   ├── app/
│   │   ├── App.tsx
│   │   ├── router.tsx
│   │   └── providers.tsx
│   ├── features/
│   │   ├── services/
│   │   │   ├── api.ts
│   │   │   ├── model.ts
│   │   │   ├── ServiceListPage.tsx
│   │   │   └── ServiceFormPage.tsx
│   │   ├── connection/
│   │   │   ├── model.ts
│   │   │   ├── ConnectionPage.tsx
│   │   │   └── HostKeyDialog.tsx
│   │   └── viewer/
│   │       ├── api.ts
│   │       └── ViewerToolbar.tsx
│   ├── components/
│   ├── lib/
│   │   ├── tauri.ts
│   │   └── validation.ts
│   └── types/
├── src-tauri/
│   ├── capabilities/
│   │   ├── shell.json
│   │   ├── macos.json
│   │   └── ios.json
│   ├── src/
│   │   ├── commands/
│   │   │   ├── profiles.rs
│   │   │   ├── credentials.rs
│   │   │   ├── connections.rs
│   │   │   └── viewer.rs
│   │   ├── connection/
│   │   │   ├── manager.rs
│   │   │   ├── provider.rs
│   │   │   ├── direct.rs
│   │   │   ├── ssh_forward.rs
│   │   │   ├── state.rs
│   │   │   └── diagnostics.rs
│   │   ├── credential/
│   │   │   ├── mod.rs
│   │   │   └── apple_keychain.rs
│   │   ├── profile/
│   │   │   ├── model.rs
│   │   │   ├── repository.rs
│   │   │   └── migration.rs
│   │   ├── ssh/
│   │   │   ├── client.rs
│   │   │   ├── host_key.rs
│   │   │   ├── key_loader.rs
│   │   │   └── forward.rs
│   │   ├── viewer/
│   │   │   ├── manager.rs
│   │   │   ├── navigation.rs
│   │   │   └── auth.rs
│   │   ├── platform/
│   │   │   ├── mod.rs
│   │   │   ├── macos.rs
│   │   │   └── ios.rs
│   │   ├── error.rs
│   │   ├── event.rs
│   │   └── lib.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── docs/
├── AGENTS.md
├── package.json
└── README.md
```

V1 先保持单个 Rust crate。模块形成稳定边界后再考虑 workspace crates。

---

## 6. 领域模型

### 6.1 ServiceProfile

Rust 使用 tagged enum，避免大量可空字段：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "connection_type", rename_all = "snake_case")]
pub enum ServiceProfile {
    DirectUrl(DirectUrlProfile),
    SshForward(SshForwardProfile),
}
```

公共元数据：

```rust
pub struct ProfileMetadata {
    pub id: Uuid,
    pub schema_version: u32,
    pub name: String,
    pub pi_hub_credential_id: Option<CredentialId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Direct URL：

```rust
pub struct DirectUrlProfile {
    pub metadata: ProfileMetadata,
    pub base_url: Url,
}
```

SSH Forward：

```rust
pub struct SshForwardProfile {
    pub metadata: ProfileMetadata,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_username: String,
    pub ssh_auth_type: SshAuthType,
    pub ssh_credential_id: CredentialId,
    pub target_host: String,
    pub target_port: u16,
    pub service_scheme: ServiceScheme,
    pub service_base_path: String,
}
```

### 6.2 CredentialRecord

Profile 只保存引用：

```rust
pub struct CredentialId(String);
```

Keychain item 的 service 建议固定为：

```text
top.jiangliuhong.pihubclient
```

account 使用：

```text
credential/<uuid>/<kind>
```

kind 示例：

```text
ssh-password
ssh-private-key
ssh-key-passphrase
pi-hub-password
```

### 6.3 KnownHostRecord

```rust
pub struct KnownHostRecord {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub public_key: Vec<u8>,
    pub sha256_fingerprint: String,
    pub trusted_at: DateTime<Utc>,
}
```

Known Host 不是秘密，可以存入非敏感 Store，但必须防止无提示覆盖。

### 6.4 ConnectionRecord

```rust
pub struct ConnectionRecord {
    pub id: ConnectionId,
    pub service_id: Uuid,
    pub state: ConnectionState,
    pub effective_url: Option<Url>,
    pub diagnostics: ConnectionDiagnostics,
}
```

活动资源不序列化，只存在 Rust 内存中。

---

## 7. Connection Provider

### 7.1 接口

```rust
#[async_trait]
pub trait ConnectionProvider: Send + Sync {
    async fn connect(
        &self,
        profile: &ServiceProfile,
        context: ConnectContext,
    ) -> Result<EstablishedConnection, ConnectError>;
}
```

`EstablishedConnection` 至少包含：

```rust
pub struct EstablishedConnection {
    pub effective_url: Url,
    pub resources: ConnectionResources,
}
```

资源通过 RAII + CancellationToken 管理。Drop 只能作为最后保障，正常路径必须显式异步关闭。

### 7.2 DirectUrlProvider

流程：

```text
validate URL
  ↓
validate scheme and TLS policy
  ↓
optional lightweight reachability check
  ↓
return original URL
```

约束：

- `https://` 完整支持。
- `http://127.0.0.1`、`http://localhost` 和本地网络地址可以按平台 ATS 策略验证。
- 公网 Direct URL 应要求 HTTPS；V1 不通过全局 ATS 例外支持任意公网 HTTP。
- TLS 错误直接失败，不允许忽略。

轻量探测不是连接成功的唯一依据。部分 Pi Hub 会返回 401，401 应视为“服务可达但需要认证”。

### 7.3 SshForwardProvider

流程：

```text
load profile
  ↓
load credential from Keychain
  ↓
connect ssh_host:ssh_port
  ↓
verify host key
  ↓
authenticate
  ↓
bind 127.0.0.1:0
  ↓
start accept loop
  ↓
probe effective URL
  ↓
return local URL
```

本地 URL：

```text
{service_scheme}://127.0.0.1:{allocated_port}{service_base_path}
```

如果 `service_scheme=https`，TLS 证书必须对客户端实际访问的 host 有效。由于 loopback hostname 与远端证书通常不匹配，V1 默认 SSH Forward 使用 `http`；SSH 已保护传输到 SSH Server 的链路。高级 HTTPS 模式只有在证书和 hostname 模型明确验证后才开放。

---

## 8. SSH Local Forward 设计

### 8.1 Listener

```rust
let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
let local_addr = listener.local_addr()?;
```

禁止传入用户配置的监听地址。

### 8.2 Accept Loop

概念流程：

```rust
loop {
    tokio::select! {
        _ = cancellation.cancelled() => break,
        accepted = listener.accept() => {
            let (local_stream, peer_addr) = accepted?;
            spawn_forward_channel(local_stream, peer_addr, ssh_handle.clone(), target.clone());
        }
    }
}
```

### 8.3 direct-tcpip Channel

每个本地 TCP 连接：

```text
local TcpStream
  ↕ copy_bidirectional
russh ChannelStream
  ↕ SSH
remote target_host:target_port
```

使用 `channel_open_direct_tcpip`，originator 信息使用本地 peer 地址，不伪造敏感来源。

### 8.4 关闭语义

必须处理：

- 任一侧 EOF
- half-close
- channel open failure
- SSH Session 断开
- App cancellation
- Viewer 关闭
- 网络切换

每个 channel 独立结束。单个 HTTP 请求失败不得直接 panic 或破坏其他 channel。

### 8.5 Keepalive

SSH 连接建立后启动 keepalive task：

- 建议周期：20～30 秒，由实现阶段验证。
- keepalive 失败转为 reconnect 状态。
- App 主动断开时取消 keepalive。
- iOS 进入后台后不依赖 keepalive 保证存活。

---

## 9. SSH Host Key 流程

### 9.1 首次连接

SSH Handler 收到服务器公钥后：

1. 计算算法和 SHA-256 Fingerprint。
2. 查询 `(host, port)` 的 KnownHostRecord。
3. 未找到时创建 `HostKeyChallenge`。
4. 发送事件给 Trusted App Shell。
5. 等待用户确认或拒绝，设置超时。
6. 用户确认后保存完整公钥和 Fingerprint。
7. 继续握手。

事件不包含任何秘密：

```json
{
  "challenge_id": "uuid",
  "host": "vps.example.com",
  "port": 22,
  "algorithm": "ssh-ed25519",
  "fingerprint": "SHA256:..."
}
```

### 9.2 Host Key 变化

发现已有记录但公钥不同时：

- Handler 返回 `host_key_changed`。
- 当前连接终止。
- 普通连接弹窗不提供“继续”按钮。
- 用户进入服务安全设置，执行独立的替换流程。

替换流程必须再次显示旧、新 Fingerprint。

---

## 10. 凭据设计

### 10.1 CredentialStore 接口

```rust
#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn put(&self, id: &CredentialId, value: SecretValue) -> Result<()>;
    async fn get(&self, id: &CredentialId) -> Result<SecretValue>;
    async fn delete(&self, id: &CredentialId) -> Result<()>;
}
```

Apple 实现使用 Security Framework Keychain。`security-framework` 的 password APIs 可用于 macOS 和 iOS。

### 10.2 Private Key 导入

流程：

```text
Trusted App Shell 请求系统文件选择器
  ↓
Rust 读取选中文件
  ↓
校验大小和 OpenSSH 格式
  ↓
必要时请求 Passphrase
  ↓
验证可解密
  ↓
完整内容写入 Keychain
  ↓
立即清理临时内存和文件句柄
```

限制：

- 不复制到普通 app data 文件。
- 不在前端状态中长期保存完整私钥。
- 前端传递秘密后应立即清空输入状态。
- 日志只记录 credential id 和类型。

### 10.3 删除引用

删除或更新 Profile 时：

1. 计算旧、新 credential references。
2. 保留仍被其他 Profile 使用的凭据。
3. 删除引用计数为零的 Keychain item。
4. 失败时返回可恢复错误，不留下 Profile 已删但凭据状态不明的静默情况。

---

## 11. Profile Store

V1 使用一个版本化 Store 文件保存：

```json
{
  "schema_version": 1,
  "profiles": [],
  "known_hosts": [],
  "last_opened_service_id": null
}
```

Store 仅由 Rust repository 操作；不要把完整 Store 权限直接开放给远端 WebView。

更新采用：

1. 读取并验证当前版本。
2. 在内存应用修改。
3. 原子保存。
4. 保存失败时不修改运行时真实状态。

每次 schema 变化增加显式 migration。

---

## 12. ConnectionManager

### 12.1 结构

```rust
pub struct ConnectionManager {
    connections: RwLock<HashMap<ConnectionId, Arc<ManagedConnection>>>,
    by_service: RwLock<HashMap<Uuid, ConnectionId>>,
}
```

`ManagedConnection`：

```rust
pub struct ManagedConnection {
    id: ConnectionId,
    service_id: Uuid,
    state: watch::Sender<ConnectionState>,
    cancellation: CancellationToken,
    tasks: Mutex<JoinSet<()>>,
    resources: Mutex<Option<ConnectionResources>>,
    diagnostics: RwLock<ConnectionDiagnostics>,
}
```

### 12.2 去重

同一个 service 默认只允许一个活动连接：

- 已连接：返回现有 connection。
- 正在连接：订阅现有进度。
- error / disconnected：创建新连接。

不同 service 可以拥有独立连接；iOS UI V1 默认只展示一个 Viewer，但底层不得依赖这个假设泄漏资源。

### 12.3 状态机

允许的主要转换：

```text
idle
  → validating
  → connecting_ssh        # ssh only
  → verifying_host_key    # ssh only
  → authenticating        # ssh only
  → opening_forward       # ssh only
  → checking_service
  → connected

connected
  → reconnecting
  → connected | error | disconnected

any active state
  → disconnecting
  → disconnected
```

非法状态转换在开发模式报错，并由单元测试覆盖。

### 12.4 Reconnect Policy

可重试：

- 短暂网络中断
- SSH transport reset
- keepalive timeout
- iOS 前台恢复后 session 已失效

不可自动重试：

- authentication_failed
- host_key_changed
- private_key_invalid
- invalid_profile
- TLS certificate error

退避建议：

```text
1s → 2s → 4s → 8s → 15s → 30s 上限
```

回到前台时可以立即执行一次显式重连，不必等待旧退避。

---

## 13. Tauri Commands 与事件

### 13.1 Commands

```text
list_services
get_service
create_service
update_service
delete_service
put_credential
delete_credential
connect_service
disconnect_connection
get_connection_status
get_connection_diagnostics
respond_host_key_challenge
replace_known_host
open_service_view
close_service_view
```

所有 command 使用 DTO，不直接暴露内部资源句柄。

### 13.2 Events

```text
connection://state-changed
connection://diagnostics-updated
ssh://host-key-challenge
viewer://closed
app://foregrounded
app://backgrounded
```

事件 payload 不包含凭据和页面内容。

---

## 14. Service View 安全架构

### 14.1 首选方案

创建独立、标签明确的 Service WebView / WebviewWindow：

```text
main                     Trusted App Shell
service-<service-id>     Untrusted Pi Hub content
```

Capability 只绑定 `main`，不为 `service-*` 定义 remote URL capability。

不得配置：

```text
dangerousRemoteDomainIpcAccess
remote.urls for Pi Hub origins
wildcard window capabilities
```

### 14.2 iOS 技术验证

Tauri 2 当前提供移动端多窗口 / scene 能力，但 V1 必须在 iPhone 真机验证：

- 创建 Service View 的 UX 是否符合单页客户端体验。
- 关闭 Service View 后能否可靠回到 App Shell。
- 键盘、文件选择、Cookie 和前后台恢复是否正常。
- Service View 是否确实无法访问 Tauri IPC。

### 14.3 iOS 备选方案

如果 Tauri 的独立移动 WebviewWindow 不能满足 UX 或安全要求，使用一个最小 Darwin 平台插件：

```text
Tauri Trusted Shell
  ↓ open viewer command
Native WKWebView container
  ├── native toolbar / close
  ├── navigation policy
  ├── auth challenge
  └── isolated WKWebsiteDataStore
```

连接核心、Profile 和 Keychain 仍保留在 Rust，不因此重写为第二套客户端。

禁止的替代方案：

- 在可信 React 页面中用 iframe 嵌入 Pi Hub。
- 给远端 origin 开放 Tauri commands。
- 直接让远端页面共享 Shell 的高权限 capability。

### 14.4 Cookie 隔离

HTTP Cookie 不按端口隔离。多个 SSH Profile 都可能使用 `127.0.0.1:<random-port>`，因此必须避免不同服务共享同一个 Cookie Store。

macOS 14 / iOS 17 上优先为每个 service 使用稳定的 `data_store_identifier`：

```text
hash(app_bundle_id + service_id) → 16-byte identifier
```

这样即使端点都是 `127.0.0.1`，Cookie、LocalStorage 和 IndexedDB 仍按 service 隔离。

删除服务时应提供清理对应 WebView data store 的路径。

### 14.5 Navigation Policy

允许：

- 当前 effective origin
- 当前 origin 的相对 URL
- Pi Hub 同源 WebSocket / SSE / fetch

外部 origin：

- `http/https` 默认交给系统浏览器。
- `mailto` 等交给系统 handler。
- 未知 scheme 拒绝并提示。

页面新开窗口请求使用同一策略，默认不在 Service View 中创建任意第三方窗口。

---

## 15. Pi Hub HTTP Basic Authentication

当前 Pi Hub 可通过 HTTP Basic Auth 保护页面。V1 设计：

1. Profile 可引用 `pi-hub-password` Keychain item。
2. 用户名默认 `pi`。
3. Service View 收到认证 challenge 时，由可信 native / Rust bridge 查询对应 credential。
4. 只对当前 profile 的 effective origin 提供凭据。
5. 不把 Authorization 放入 URL、日志或 Tauri event。
6. 用户未保存密码或认证失败时，显示可信 Shell 提示并允许重新输入。

Phase 0 必须验证 Tauri / Wry 在 macOS 与 iOS 上是否暴露足够的 authentication challenge hook。

若默认 API 不足，优先在 Darwin WKWebView 容器中实现 `WKNavigationDelegate` challenge 处理；不使用在 URL 中嵌入凭据的降级方案。

---

## 16. ATS、HTTP 与 TLS

### 16.1 SSH Loopback

SSH Forward 默认生成：

```text
http://127.0.0.1:<random-port>
```

配置 Apple 平台所需的最小 local networking 例外，不使用全局 `NSAllowsArbitraryLoads`。

### 16.2 Direct URL

- 公网 Direct URL：要求 HTTPS。
- 局域网 / loopback HTTP：允许，但显示明文提示，并受系统 local networking 权限与 ATS 约束。
- 不设置 `NSAllowsArbitraryLoadsInWebContent=true` 来支持任意公网 HTTP，避免扩大攻击面和 App Review 风险。

### 16.3 TLS

- 使用系统证书验证。
- 不支持自签名证书忽略。
- 不实现用户可关闭的 TLS verification。
- SSH Forward 高级 HTTPS 模式只有在证书 hostname 与访问 hostname 一致时可用；否则提示改用 tunnel 内 HTTP。

---

## 17. 服务可达性检查

### 17.1 目标

在打开 Service View 前区分：

- 网络不可达
- SSH 成功但目标端口不可达
- HTTP 服务可达
- 服务要求认证
- TLS 失败

### 17.2 策略

Direct URL：

- 可选执行短超时 GET。
- 2xx、3xx、401 视为 HTTP 服务可达。
- 其他状态不一定阻止打开，但记录诊断。

SSH Forward：

1. Listener 启动后请求 effective URL。
2. 该请求会真实经过 `direct-tcpip`。
3. 成功或 401 后再打开 Service View。
4. TCP 失败映射为 `target_unreachable`。

V1 不强制判断目标一定是 Pi Hub，因为当前没有稳定 Client Protocol 探测接口。后续可以在 Pi Hub 增加 `/api/client-info` 后升级。

---

## 18. 平台生命周期

### 18.1 macOS

- 关闭 Service View：默认断开该 service，后续可增加“保持连接”。
- 关闭主窗口但 App 未退出：产品实现阶段决定是否保留活动连接，默认保持与 UI 语义一致。
- `applicationWillTerminate`：取消全部 connection token，并等待有限时间完成关闭。
- 不注册 LaunchAgent。

### 18.2 iOS

前台：

```text
active → keep connection
```

进入后台：

```text
persist viewer/service id
stop aggressive reconnect
allow OS to suspend socket
```

回到前台：

```text
check session/listener
  ├── valid → restore viewer
  └── invalid → immediate reconnect → restore viewer
```

不声明与产品无关的后台模式，不尝试绕过 iOS 生命周期限制。

---

## 19. 错误模型

Rust 内部：

```rust
pub enum AppError {
    Profile(ProfileError),
    Credential(CredentialError),
    Ssh(SshError),
    Forward(ForwardError),
    Service(ServiceError),
    Viewer(ViewerError),
    Platform(PlatformError),
    Cancelled,
}
```

Tauri 边界返回稳定 DTO：

```json
{
  "code": "authentication_failed",
  "message": "SSH 认证失败",
  "retryable": false,
  "stage": "authenticating",
  "details": {
    "host": "vps.example.com",
    "port": 22
  }
}
```

`details` 必须经过白名单序列化，不直接暴露底层错误字符串。

---

## 20. 日志与诊断

日志等级：

```text
ERROR  无法继续的连接或存储错误
WARN   重连、异常断开、安全提示
INFO   状态转换和资源生命周期
DEBUG  非敏感协议阶段，仅开发构建
TRACE  默认禁用，不记录 payload
```

统一 Redaction：

- Password → `[REDACTED]`
- Private Key → 从不进入格式化参数
- Authorization / Cookie → 从不记录
- URL query → 默认移除
- Host / Port 可记录
- 本地随机端口仅诊断页可见

生产构建默认不启用远端日志上传。

---

## 21. Capability 设计

`main` 只申请真实需要的最小权限，例如：

- 调用自定义 profile / connection commands
- 系统文件选择器，用于导入私钥
- 系统 opener，用于外部链接
- 必要的窗口控制

不默认开放：

- Shell execute
- 任意文件系统读写
- 任意 HTTP client 给前端
- Process control
- Store wildcard

`service-*` 不匹配任何 capability 文件，不定义 remote URL 权限。

平台专属 capability 使用 `platforms` 分离，不把 macOS-only 权限带到 iOS。

---

## 22. 测试设计

### 22.1 Rust 单元测试

- ServiceProfile 校验
- migration
- ConnectionState 转换
- reconnect policy
- Host Key fingerprint 与比对
- credential reference 计算
- error DTO 脱敏
- navigation allowlist

### 22.2 SSH 集成测试

测试环境启动受控 SSH Server 和 HTTP echo / mock Pi Hub：

- Password auth
- Ed25519 auth
- encrypted key
- unknown Host Key
- changed Host Key
- wrong password
- target unreachable
- multi-channel HTTP
- cancellation
- listener cleanup

CI 中不使用真实生产 SSH Key。

### 22.3 前端测试

- 两种 Profile 表单
- 条件字段
- 校验错误
- Host Key Dialog
- connection state UI
- destructive confirmation
- external navigation behavior

### 22.4 平台测试

macOS 真机：

- Direct HTTPS
- SSH Forward
- Keychain
- WebView Cookie isolation
- 文件上传 / 下载
- App 退出释放

iPhone 真机：

- Direct HTTPS
- SSH Forward
- Host Key challenge
- Basic Auth
- 键盘输入
- 文件选择
- 前后台恢复
- 网络切换 Wi-Fi / Cellular

---

## 23. 构建与 CI

### 23.1 本地工具链

项目初始化时提交：

- `.nvmrc` 或等价 Node 版本文件
- `rust-toolchain.toml`
- `package-lock.json`
- `Cargo.lock`

### 23.2 GitHub Actions

基础 CI：

```text
frontend format/lint/typecheck/test
cargo fmt/clippy/test
macOS debug/release compile
security-sensitive unit tests
```

iOS：

- 无签名 Simulator build 可进入 CI。
- 真机签名和 TestFlight 使用受保护 secrets 和手动 release workflow。
- 不在 PR 日志输出证书或 provisioning 内容。

---

## 24. Phase 0 技术 Spike

必须产出一份可复现结果，而不是只证明“能编译”。

### Spike A：Tauri 跨端

- macOS 运行 React Shell。
- iPhone 真机运行同一工程。
- 验证最低系统版本设置。

### Spike B：russh on iOS

- Password auth。
- Ed25519 auth。
- 加密 Key。
- `channel_open_direct_tcpip`。
- 网络切换和取消。

### Spike C：Service View Security

- 加载 Direct URL。
- 加载 loopback URL。
- 尝试从远端页面调用 Tauri API，必须失败。
- 验证外部链接策略。
- 验证每个 service 的 Cookie Store 隔离。

### Spike D：Basic Auth

- macOS challenge。
- iOS challenge。
- 凭据从 Keychain 读取。
- 错误密码更新。

### Spike E：Pi Hub Compatibility

- 流式消息。
- 长会话滚动。
- iOS 键盘。
- 代码复制。
- 文件上传。
- 文件下载。
- 页面刷新和 Cookie。

任一 Spike 失败时，先记录：

```text
复现步骤
环境版本
失败原因
可选方案
对范围和风险的影响
```

再决定是否使用 Darwin WKWebView plugin。不要直接更换整个技术栈。

---

## 25. 已知风险与处理

| 风险 | 影响 | V1 处理 |
|---|---|---|
| Tauri 移动 Service Window UX 不符合预期 | iOS Viewer 无法按设计呈现 | Phase 0 验证；必要时使用最小 WKWebView 插件 |
| Wry 未直接暴露 Basic Auth challenge | Pi Hub Password 无法自动处理 | Darwin native delegate 插件 |
| `russh` iOS crypto backend 或 key parsing 问题 | SSH 功能阻塞 | Phase 0 真机验证；保留替换 SSH 实现的 Provider 边界 |
| 多个 127.0.0.1 Profile 共享 Cookie | 会话串用 | 每 service 独立 WKWebsiteDataStore identifier |
| iOS 后台挂起 SSH | 返回时连接失效 | 前台自动恢复，不承诺后台常驻 |
| Public HTTP 被 ATS 拦截 | Direct HTTP 不可用 | 公网要求 HTTPS；只开放最小 local networking |
| Pi Hub 移动布局不足 | App 内体验差 | 在 pi-hub 修复响应式，不复制 WebUI |
| Relay 语义不一致 | 用户配置复杂 | V1 只暴露 SSH target host/port，高级设置说明清楚 |

---

## 26. 后续扩展点

V1 完成后可按优先级增加：

1. `RelayProvider`：专用 Client ↔ Relay ↔ Agent 协议。
2. macOS Agent：把本机 Pi Hub 主动接入 Relay。
3. 服务配置加密同步。
4. Windows / Android。
5. 多窗口同时使用多个 Pi Hub。
6. `/api/client-info` 协议与版本兼容检查。
7. 原生通知和深链接。

这些能力不能反向污染 V1 的 `DirectUrlProvider` 和 `SshForwardProvider`。

---

## 27. 关键设计决策摘要

| 决策 | 结论 |
|---|---|
| 是否重写 Pi Hub UI | 否，直接加载现有 WebUI |
| 是否使用 Electron | 否，V1 使用 Tauri 2 |
| 是否同时支持 macOS / iOS | 是，同仓、共享连接核心 |
| SSH 如何实现 | Rust `russh` + `direct-tcpip` |
| 是否建设 Relay Server | 否，V1 只消费已有 SSH 中继端点 |
| 敏感数据存储 | Apple Keychain |
| 本地映射监听 | `127.0.0.1:0`，系统随机端口 |
| Host Key 策略 | 首次明确确认，变化强制阻断 |
| 远端 WebView 权限 | 零 Tauri remote capability |
| 公网 Direct HTTP | 不支持，要求 HTTPS |
| iOS 后台 Tunnel | 不承诺；回前台自动恢复 |
| Cookie 隔离 | 每 service 独立 WebView data store |
