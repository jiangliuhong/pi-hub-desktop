# Pi Hub Client V1 开发需求

- 仓库：`jiangliuhong/pi-hub-desktop`
- 产品名称：Pi Hub Client
- 文档版本：1.0
- 目标平台：macOS 14+、iOS 17+
- 状态：开发基线

---

## 1. 背景

Pi Hub 以 Web 服务运行，常见地址为：

```text
http://127.0.0.1:30142
```

用户可能将 Pi Hub 部署在：

1. 可直接访问的公网或局域网地址。
2. 只能通过 SSH Local Port Forward 访问的服务器。
3. 已经通过 VPS / SSH Reverse Tunnel 暴露到某个 SSH 中继端点的家庭电脑。

当前在 iPhone 上访问 SSH 后面的 Pi Hub，通常需要：

```text
Termius → 建立端口映射 → Safari → localhost → Pi Hub
```

该流程需要用户理解 SSH、端口映射和本地端口，并在多个 App 之间切换。

V1 的目标是将服务配置、连接建立和 Pi Hub WebUI 集成到同一个 macOS / iOS 客户端中。

---

## 2. 产品目标

用户只需要：

```text
打开 App → 选择 Pi Hub 服务 → 自动连接 → 开始使用
```

用户不需要：

- 手动执行 SSH 命令。
- 手动选择或维护本地映射端口。
- 使用 Termius 创建 Tunnel。
- 再打开 Safari 或 Chrome。
- 理解 `ssh -L` 的具体参数。

V1 成功的核心标准是：

> 同一个服务配置可以在 macOS 和 iOS 上以一致方式使用；直连服务直接打开，SSH 服务自动完成 Local Port Forward 后打开。

---

## 3. 产品定位

Pi Hub Client 是 Pi Hub 的连接与展示客户端，不是 Pi Hub 的重新实现。

```text
Pi Hub Client
├── 服务配置
├── Direct URL
├── SSH Local Port Forward
├── 凭据与 Host Key 管理
└── Pi Hub Service WebView

Pi Hub / pi-web
├── Agent
├── Session
├── Workspace
├── Scheduler
└── WebUI
```

现有 Pi Hub 页面直接加载到 App 中。V1 不复制、不重写 Pi Hub WebUI。

---

## 4. 用户与使用场景

V1 面向管理自己 Pi Hub 实例的单用户。

### 4.1 场景 A：直接地址

Pi Hub 已通过 HTTPS、局域网或可信网络提供地址：

```text
https://pi.example.com
```

用户添加 Direct URL 服务，点击后直接在 App 中打开。

### 4.2 场景 B：SSH 访问 VPS 上的 Pi Hub

```text
Client
  ↓ SSH
VPS:22
  ↓ direct-tcpip
VPS 127.0.0.1:30142
```

客户端自动创建本地随机端口并加载：

```text
http://127.0.0.1:<random-port>
```

### 4.3 场景 C：通过已存在的 SSH 中继端点访问

家庭电脑已经通过其他服务或 Reverse Tunnel 接入 VPS。Pi Hub Client 只消费该中继结果：

```text
Client
  ↓ SSH
Relay VPS
  ↓ relay-local target
Home Pi Hub
```

用户配置 SSH Server 以及从 SSH Server 视角可访问的目标 `host:port`。V1 不负责创建家庭电脑到 VPS 的 Reverse Tunnel。

---

## 5. V1 范围

### 5.1 平台

- macOS 14+
- iOS 17+

两个平台使用同一套服务数据模型、连接核心和主要 UI。

### 5.2 连接类型

V1 只支持：

1. `direct_url`
2. `ssh_forward`

### 5.3 SSH 认证

V1 支持：

- SSH Password
- OpenSSH Ed25519 Private Key
- OpenSSH RSA Private Key
- 加密私钥与 Passphrase

### 5.4 Pi Hub Web 认证

V1 应兼容 Pi Hub 当前使用的 HTTP Basic Authentication：

- 用户名默认 `pi`。
- Pi Hub Password 为可选配置。
- Password 必须存入 Apple Keychain。
- 不允许将用户名和密码拼入 URL。
- 未配置密码时，允许服务自行完成认证流程或返回明确的 401 提示。

该能力必须在正式实现前通过 macOS 和 iPhone 真机技术验证。

---

## 6. 服务配置模型

### 6.1 公共字段

| 字段 | 必填 | 说明 |
|---|---:|---|
| `id` | 是 | 稳定 UUID |
| `schema_version` | 是 | 配置结构版本 |
| `name` | 是 | 用户可识别名称 |
| `connection_type` | 是 | `direct_url` 或 `ssh_forward` |
| `pi_hub_credential_id` | 否 | Pi Hub HTTP 凭据 Keychain 引用 |
| `created_at` | 是 | 创建时间 |
| `updated_at` | 是 | 更新时间 |

### 6.2 Direct URL 字段

| 字段 | 必填 | 默认值 | 说明 |
|---|---:|---|---|
| `base_url` | 是 | - | 完整 HTTP/HTTPS URL |

示例：

```json
{
  "id": "uuid",
  "schema_version": 1,
  "name": "Cloud Pi Hub",
  "connection_type": "direct_url",
  "base_url": "https://pi.example.com"
}
```

### 6.3 SSH Forward 字段

| 字段 | 必填 | 默认值 | 说明 |
|---|---:|---|---|
| `ssh_host` | 是 | - | SSH Server 域名或 IP |
| `ssh_port` | 是 | `22` | SSH 端口 |
| `ssh_username` | 是 | - | SSH 用户名 |
| `ssh_auth_type` | 是 | - | `password` 或 `private_key` |
| `ssh_credential_id` | 是 | - | Keychain 凭据引用 |
| `target_host` | 是 | `127.0.0.1` | 从 SSH Server 视角访问的目标 |
| `target_port` | 是 | `30142` | Pi Hub 或中继目标端口 |
| `service_scheme` | 是 | `http` | `http` 或 `https` |
| `service_base_path` | 否 | `/` | Pi Hub 基础路径 |

示例：

```json
{
  "id": "uuid",
  "schema_version": 1,
  "name": "VPS Pi Hub",
  "connection_type": "ssh_forward",
  "ssh_host": "vps.example.com",
  "ssh_port": 22,
  "ssh_username": "ubuntu",
  "ssh_auth_type": "private_key",
  "ssh_credential_id": "cred_uuid",
  "target_host": "127.0.0.1",
  "target_port": 30142,
  "service_scheme": "http",
  "service_base_path": "/"
}
```

服务配置不得包含 Password、Private Key、Passphrase 或 Pi Hub Password 明文。

---

## 7. 功能需求

### FR-001 服务列表

客户端首页显示已保存的 Pi Hub 服务。

每个服务至少显示：

- 名称
- 连接类型
- 当前状态
- 最近一次连接结果或时间

未主动检测的服务不得虚构显示为 `Online`。默认状态应为“未连接”。

### FR-002 新增服务

用户可以新增 Direct URL 或 SSH Forward 服务。

表单必须根据连接类型只显示相关字段，并在保存前完成格式校验。

### FR-003 编辑服务

用户可以编辑服务名称和连接参数。

修改 SSH `host` 或 `port` 后，原 Host Key 信任记录不得自动沿用到新的端点。

### FR-004 删除服务

用户可以删除服务。

删除时必须：

1. 断开该服务的活动连接。
2. 删除服务配置。
3. 删除不再被其他服务引用的 Keychain 凭据。
4. 删除对应 Host Key 信任记录。

删除操作需要二次确认。

### FR-005 Direct URL 连接

点击 Direct URL 服务后：

1. 校验 URL。
2. 对 HTTP 地址显示一次明确安全提示；用户确认后可继续。
3. 使用 App 内 Service WebView 打开 URL。
4. HTTPS 使用系统证书校验。
5. 失败时显示可操作的错误原因。

V1 不允许忽略 TLS 证书错误。

### FR-006 SSH Forward 连接

点击 SSH Forward 服务后：

1. 读取服务配置与 Keychain 凭据。
2. 连接 SSH Server。
3. 验证 SSH Host Key。
4. 完成 Password 或 Private Key 认证。
5. 在 `127.0.0.1` 上申请系统随机端口。
6. 为每个本地 TCP 连接创建 SSH `direct-tcpip` channel。
7. 检查目标服务是否可访问。
8. 在 Service WebView 中打开生成的 loopback URL。

用户不需要看到或输入本地随机端口。

### FR-007 首次 Host Key 确认

首次连接新的 `ssh_host:ssh_port` 时，必须显示：

- Host
- Port
- Host Key 算法
- SHA-256 Fingerprint

只有用户明确确认后才能保存并继续连接。

### FR-008 Host Key 变化

已保存的 Host Key 与服务器返回值不一致时：

- 立即阻断连接。
- 明确提示可能存在服务器重装或中间人攻击。
- 不允许一键静默覆盖。
- 用户必须进入显式的“替换 Host Key”流程并再次确认。

### FR-009 连接状态

客户端至少展示以下状态：

```text
未连接
校验配置
连接 SSH
等待 Host Key 确认
SSH 认证
建立映射
检查 Pi Hub
已连接
重新连接
正在断开
连接失败
```

### FR-010 断开连接

用户可以主动断开当前服务。

断开后必须关闭：

- Service WebView 或当前服务页面
- 本地 TCP Listener
- 所有 forwarding channel
- SSH Session
- keepalive / reconnect task

### FR-011 自动重连

- macOS：活动连接异常时可自动重连，直到用户断开或退出 App。
- iOS：前台连接异常时自动重连；后台不承诺保持，回到前台后自动恢复。
- 自动重连必须设置退避和最大单次等待时间。
- Host Key 变化时禁止自动重连绕过确认。
- 认证失败后不进行无限重试。

### FR-012 App 内 Pi Hub 页面

连接成功后直接显示现有 Pi Hub WebUI。

Service WebView 需要支持：

- Pi Hub 流式消息输出
- 文本输入与 iOS 键盘
- 复制文本和代码
- 页面内部正常导航
- 文件选择
- 下载或以系统方式打开下载内容
- Cookie / Session 保持
- 返回服务列表
- 刷新与重新连接

### FR-013 外部链接

Pi Hub 页面打开非当前服务 origin 的外部链接时，默认交给系统浏览器处理。

不得让 Service WebView 任意导航到不相关站点后继续保留 Pi Hub 会话上下文。

### FR-014 配置持久化

服务配置在 App 重启后保留。

非敏感配置可以存入本地 Store；所有配置必须包含 schema version 并支持迁移。

### FR-015 凭据管理

用户可以：

- 输入并保存 SSH Password。
- 从系统文件选择器导入 OpenSSH Private Key。
- 输入 Private Key Passphrase。
- 更新或删除凭据。

敏感值只存入 Apple Keychain。

### FR-016 诊断信息

连接失败页面可以显示以下非敏感信息：

- 当前阶段
- 错误码
- SSH Host / Port
- Target Host / Port
- 是否已建立 Listener
- 重试次数

不得显示秘密、Authorization、Cookie、Private Key 内容或业务数据。

---

## 8. 页面需求

### 8.1 服务列表页

```text
Pi Hub Client

Cloud Pi Hub             Direct URL
未连接                       >

VPS Pi Hub               SSH Forward
上次连接成功                 >

Home Pi Hub              SSH Forward
上次连接失败                 >

+ 添加服务
```

支持新增、编辑、删除和点击连接。

### 8.2 服务编辑页

公共字段：

- 名称
- 连接方式
- 可选 Pi Hub Password

Direct URL：

- URL

SSH Forward：

- SSH Host
- SSH Port
- Username
- Authentication Type
- Password 或 Private Key
- Target Host
- Target Port
- Service Scheme
- Base Path

`Target Host` 和 `Target Port` 默认折叠在“高级设置”中，普通 VPS 场景默认使用 `127.0.0.1:30142`。

### 8.3 Host Key 确认页

突出显示 Fingerprint，并说明确认前应与服务器可信来源核对。

### 8.4 连接进度页

展示当前阶段和取消按钮。

连接失败时提供：

- 重试
- 编辑配置
- 查看诊断
- 返回列表

### 8.5 Pi Hub 页面

至少提供原生或可信 Shell 控制：

- 返回服务列表
- 服务名称
- 连接状态
- 刷新
- 断开 / 重连

远端 Pi Hub 内容不得取得这些控制能力对应的原生权限。

---

## 9. 安全需求

### SR-001 凭据安全

Password、Private Key、Passphrase 和 Pi Hub Password 只能保存到 Apple Keychain。

### SR-002 SSH 服务器身份

必须执行 Host Key 校验，不允许关闭或跳过。

### SR-003 Listener 暴露范围

SSH Local Forward 只能监听 `127.0.0.1`，不得监听 LAN 或公网接口。

### SR-004 WebView 权限隔离

远端 Service WebView：

- 不配置 Tauri remote capability。
- 不允许调用 Tauri command。
- 不允许读取本地 Store 或 Keychain。
- 不允许执行 Shell 或访问任意文件系统。

### SR-005 日志脱敏

日志不得包含：

- Password
- Private Key
- Passphrase
- Authorization Header
- Cookie
- 完整页面响应内容

### SR-006 网络安全

- HTTPS 使用系统信任链。
- 不提供忽略证书错误选项。
- 不全局关闭 ATS。
- Direct HTTP 需要明确提示。

### SR-007 配置验证

阻止以下配置：

- 非 HTTP/HTTPS Direct URL
- 非法端口
- 空 SSH Username
- 缺少凭据
- `0.0.0.0` 作为本地 Listener

`target_host` 可以是 SSH Server 可访问的域名、IP 或 loopback 地址。

---

## 10. 平台行为

### 10.1 macOS

- 支持窗口关闭、重新打开和 App 退出。
- App 仍运行时可以维持活动连接。
- App 退出时关闭全部连接。
- V1 不作为家庭电脑常驻 Agent，不注册 LaunchAgent。
- 可以连接并使用已经存在的 SSH 中继端点。

### 10.2 iOS

- App 前台时维持连接。
- 进入后台后不承诺 SSH 持续在线。
- 回到前台时检查连接并自动恢复。
- 不使用不相关的 Background Mode 保持连接。
- 不实现系统级 VPN 或 Network Extension。

---

## 11. 非功能需求

### NFR-001 性能

- 服务列表冷启动目标：2 秒内可交互，不包含网络连接。
- 本地空闲状态不得维持无意义的 SSH 连接。
- 同时打开一个 Pi Hub 页面时，避免重复创建同一服务的 SSH Session。

### NFR-002 可靠性

- 单个 forwarding channel 失败不得导致 App 崩溃。
- 取消连接必须能及时结束异步任务。
- App 退出或服务删除后不得残留本地 Listener。

### NFR-003 可维护性

- Direct URL 和 SSH Forward 通过统一 Provider 接口实现。
- 平台差异集中在适配层。
- 服务配置使用可判别联合类型 / tagged enum。

### NFR-004 可测试性

连接、状态机、Host Key、配置校验和凭据接口必须可在没有 UI 的情况下测试。

### NFR-005 可扩展性

后续 Relay 应作为新的 Connection Provider 接入，不改变服务列表和 Viewer 的主流程。

---

## 12. V1 明确不做

- 创建或部署 Relay Server
- 家庭电脑端常驻 Agent
- 自动建立 SSH Reverse Tunnel
- ProxyJump 或多级 SSH
- SSH Terminal
- SFTP
- 通用 Tunnel 配置
- Android / Windows / Linux
- 账号系统和云同步
- Pi Hub WebUI 重写
- iOS 永久后台连接
- 自签名 HTTPS 证书忽略
- 自动更新
- 系统托盘与开机启动

---

## 13. 开发阶段

### Phase 0：技术验证

正式开发前必须跑通：

1. 同一 Tauri 2 工程构建 macOS 与 iOS。
2. `russh` 在 iPhone 真机完成 SSH Password 登录。
3. `russh` 在 iPhone 真机完成 Ed25519 Key 登录。
4. `direct-tcpip` 将本地 loopback 映射到远端 Pi Hub。
5. Service WebView 在 macOS / iOS 加载 Direct URL 与 loopback URL。
6. 远端页面无法调用任何 Tauri API。
7. Apple Keychain 在两个平台正确读写。
8. Pi Hub HTTP Basic Authentication 可安全完成。
9. Pi Hub 流式输出、输入框、文件选择和 Cookie 正常。
10. iOS 前后台切换后可以恢复连接。

如果第 5 或第 8 项受 Tauri WebView API 限制，先提交可复现结论，再决定使用 Tauri 原生插件或单独的 WKWebView 容器；不得通过扩大远端页面权限解决。

### Phase 1：服务与 Direct URL

- 项目初始化
- 服务数据模型和 Store
- Keychain 抽象
- 服务列表与编辑页
- Direct URL Viewer
- 安全导航

### Phase 2：SSH Forward

- SSH Password
- Private Key / Passphrase
- Host Key 校验
- Local Listener
- `direct-tcpip`
- 连接状态和错误提示

### Phase 3：跨端完善

- macOS 完整验收
- iPhone 真机完整验收
- iOS 生命周期恢复
- 文件选择 / 下载
- HTTP Basic Authentication
- 发布构建和 TestFlight 准备

---

## 14. V1 验收标准

### 14.1 Direct URL

在 macOS 和 iPhone 上：

1. 添加 `https://` Pi Hub。
2. 点击服务。
3. 在 App 内正常使用 Pi Hub。
4. 返回服务列表后可以再次打开。

### 14.2 VPS SSH

在 macOS 和 iPhone 上：

1. 添加 SSH Forward 服务。
2. 首次连接确认 Host Key。
3. 使用 Password 或 Ed25519 Key 登录。
4. 自动映射到远端 `127.0.0.1:30142`。
5. Pi Hub 页面正常展示和流式输出。
6. 全过程无需 Termius、Safari 和手动 SSH。

### 14.3 已有中继端点

在 macOS 和 iPhone 上：

1. 配置 Relay VPS 的 SSH 地址。
2. 配置 Relay VPS 视角下的目标 `host:port`。
3. 自动建立 Local Forward。
4. 正常访问 Home Pi Hub。

V1 只验证对接已有中继端点，不验收 Reverse Tunnel 的创建与保活。

### 14.4 安全

1. 所有秘密只存在 Keychain。
2. Host Key 变化会阻断连接。
3. Listener 只绑定 `127.0.0.1`。
4. 本地端口由系统随机分配。
5. Service WebView 无 Tauri 原生权限。
6. HTTPS 证书错误不可忽略。
7. 日志不含敏感数据。

### 14.5 生命周期

1. macOS 主动断开或退出后无残留 Listener。
2. iOS 进入后台再返回后，连接可自动恢复。
3. 网络中断后显示明确状态并可重试。
4. 删除活动服务会先安全断开。

---

## 15. V1 完成定义

只有满足以下条件才能标记 V1 完成：

- Direct URL 和 SSH Forward 均在真实 macOS 与真实 iPhone 上通过。
- Password、Ed25519 Key 和带 Passphrase Key 均完成验证。
- 已有 SSH 中继端点对接通过。
- Pi Hub WebUI 的核心操作可用。
- 安全验收全部通过。
- 单元测试、Rust 集成测试、前端测试通过。
- macOS Release 构建成功。
- iOS Release / TestFlight 构建成功。
- 已知限制记录在 README 或发布说明中。
