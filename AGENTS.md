# AGENTS.md

本文件是 `pi-hub-desktop` 仓库内所有 AI 编码代理和开发者的最高优先级项目规则。开始修改代码前，必须先阅读本文件以及：

- `docs/requirements-v1.md`
- `docs/design-v1.md`

如果实现与文档冲突，先修正文档或明确记录设计决策，不得静默扩大范围。

---

## 1. 项目定义

仓库名称为 `pi-hub-desktop`，产品显示名称暂定为 **Pi Hub Client**。

本项目不是 Pi Hub 的重写，而是 Pi Hub 的 macOS / iOS 客户端。V1 只负责：

1. 保存多个 Pi Hub 服务配置。
2. 直接打开可访问的 Pi Hub URL。
3. 自动建立 SSH Local Port Forward 后打开 Pi Hub。
4. 在 App 内使用现有 Pi Hub WebUI。

核心业务仍由远端或本机的 `pi-hub / pi-web` 提供。本仓库不得复制 Pi Hub 的会话、Agent、Scheduler、Workspace 等业务实现。

---

## 2. V1 固定范围

### 2.1 支持平台

- macOS 14+
- iOS 17+

必须使用同一个 Tauri 2 工程和同一套 React / TypeScript 前端。共享连接逻辑优先放在 Rust 中，平台差异必须通过明确的适配层处理。

### 2.2 服务类型

V1 只允许两种服务类型：

- `direct_url`：直接访问 HTTP/HTTPS 地址。
- `ssh_forward`：连接 SSH Server，将本地随机 loopback 端口映射到 SSH Server 可访问的目标地址和端口。

`ssh_forward` 必须支持连接已经存在的 SSH 中继端点，但 V1 不负责创建或维护中继端点。

### 2.3 V1 不做

禁止未经需求变更自行加入以下内容：

- Relay Server
- 家庭电脑常驻 Agent
- SSH Reverse Forward
- ProxyJump / 多级跳板
- SSH Terminal
- SFTP 或文件管理
- VPN、系统代理或通用端口转发器
- Android、Windows、Linux
- 用户账号、云同步、团队协作
- Pi Hub WebUI 重写
- 后台永久维持 iOS Tunnel
- macOS 开机常驻中继 Agent

发现这些需求时，应记录到后续版本，不得顺手实现。

---

## 3. 推荐技术栈

除非经过明确的设计变更，否则使用：

- Tauri 2
- React
- TypeScript，开启严格模式
- Vite
- Rust stable
- Tokio
- `russh`，用于 SSH 客户端和 `direct-tcpip`
- Apple Security Framework / Keychain，用于敏感凭据
- Tauri Store 或 Rust 侧等价持久化，用于非敏感服务配置

不要仅为方便而增加 Electron、Node sidecar、Flutter 或第二套 UI 工程。

依赖必须固定兼容版本并提交 lockfile。新增依赖前必须说明：

1. 解决的问题。
2. 为什么标准库或现有依赖不能解决。
3. 是否支持 `aarch64-apple-ios`、iOS Simulator 和 macOS。
4. 安全与维护风险。

---

## 4. 目标目录结构

项目初始化后优先保持以下边界：

```text
pi-hub-desktop/
├── src/
│   ├── app/                    # 路由、启动和全局状态
│   ├── features/
│   │   ├── services/           # 服务列表与配置
│   │   ├── connection/         # 连接状态 UI
│   │   └── viewer/             # Pi Hub 访问界面控制
│   ├── components/             # 可复用展示组件
│   ├── lib/                    # 纯 TypeScript 工具和 API client
│   └── types/                  # 前端共享类型
├── src-tauri/
│   ├── capabilities/           # 最小权限配置
│   ├── src/
│   │   ├── commands/           # Tauri command 薄适配层
│   │   ├── connection/         # ConnectionManager 与 Provider
│   │   ├── credential/         # Keychain 抽象
│   │   ├── profile/            # 服务配置持久化
│   │   ├── ssh/                # SSH session 与 forwarding
│   │   ├── viewer/             # Service WebView 生命周期
│   │   ├── platform/           # macOS / iOS 差异适配
│   │   ├── error.rs
│   │   └── lib.rs
│   └── tauri.conf.json
├── docs/
├── AGENTS.md
└── README.md
```

V1 不提前拆成复杂 monorepo 或大量 crates。只有模块已经形成稳定独立边界并存在真实复用需求时，才能拆 crate。

---

## 5. 架构规则

### 5.1 Provider 抽象

连接方式必须通过统一接口实现，UI 不得直接操作 SSH：

```text
ConnectionProvider
├── DirectUrlProvider
└── SshForwardProvider
```

统一返回：

- connection id
- service id
- effective URL
- connection state
- disconnect handle / cancellation token

未来增加 Relay 时应新增 Provider，而不是修改 UI 的核心流程。

### 5.2 Command 必须薄

Tauri command 只负责：

- 输入校验
- 调用领域服务
- 转换错误
- 返回可序列化 DTO

禁止在 command 中堆积 SSH、存储或生命周期逻辑。

### 5.3 前端不保存连接真相

SSH Session、本地 Listener 和连接生命周期的唯一真实状态必须在 Rust 侧。前端只展示 Rust 返回或事件推送的状态。

不得仅靠 React 状态判断 Tunnel 是否存在。

### 5.4 不侵入 Pi Hub

除非需求明确要求，本仓库不得修改 `jiangliuhong/pi-hub`。客户端应兼容标准 HTTP 页面和当前 Pi Hub WebUI。

如确实需要新增稳定探测接口，先在设计文档中说明兼容策略，再单独修改 Pi Hub 仓库。

---

## 6. 安全硬规则

以下规则没有“先做出来再说”的例外。

### 6.1 凭据

以下数据只能存入 Apple Keychain：

- SSH Password
- SSH Private Key
- Private Key Passphrase
- 可选的 Pi Hub HTTP 凭据

服务配置中只能保存 `credential_id` 等引用，不得保存明文或可逆拼接值。

禁止在以下位置出现秘密：

- JSON / Store 配置
- SQLite 明文字段
- Git
- 测试快照
- panic 信息
- 日志
- URL query 或 user-info
- Tauri event payload

删除服务时必须同步删除不再被引用的凭据。

### 6.2 SSH Host Key

- 首次连接必须显示算法和 SHA-256 Fingerprint，由用户明确确认。
- 已确认 Host Key 按 `host + port` 绑定。
- Host Key 变化时必须阻断连接。
- 禁止自动接受变化后的 Host Key。
- 禁止实现等价于 `StrictHostKeyChecking=no` 的逻辑。

### 6.3 本地映射端口

- 必须绑定 `127.0.0.1`，不得绑定 `0.0.0.0`。
- 使用系统分配的临时端口，禁止固定使用 30142。
- 连接失败、断开、页面关闭和 App 退出时必须释放 Listener。
- 同时连接多个服务时，每个连接使用独立端口和生命周期。

### 6.4 远端 Web 内容

Pi Hub Service WebView 加载的是远端内容，必须按不可信内容处理：

- 不给远端 origin 配置任何 Tauri remote capability。
- 不启用 `dangerousRemoteDomainIpcAccess` 或等价配置。
- Service WebView 不得读取 Keychain、文件系统或服务配置。
- Service WebView 不得调用 Shell、Process、Store、Dialog 等原生命令。
- App Shell 与 Service WebView 必须具有清晰的安全边界。
- 外部站点链接默认交给系统浏览器，不在具有 Pi Hub 会话的 WebView 中任意导航。
- 只允许当前连接的目标 origin，以及经过明确验证的 Pi Hub 内部导航。

如果 iOS 上独立 Service WebView 的实现存在限制，必须先完成技术验证，不得通过给远端页面扩大权限来绕过。

### 6.5 HTTP / TLS

- Direct URL 支持 HTTP 和 HTTPS，但 UI 必须明确提示 HTTP 为明文传输。
- SSH Forward 的本地 endpoint 可以是 loopback HTTP；SSH 只保护 SSH 段。
- HTTPS 必须使用系统证书校验。
- V1 不提供“忽略证书错误”。
- 不得全局关闭 ATS；只允许为必要的 loopback / local networking 配置最小例外。

---

## 7. SSH 实现规则

### 7.1 V1 支持

- Password authentication
- OpenSSH Ed25519 private key
- OpenSSH RSA private key
- 加密私钥和 Passphrase
- SSH keepalive
- `direct-tcpip` Local Port Forward

### 7.2 V1 不支持

- SSH Agent
- FIDO2 / YubiKey
- PKCS#11
- keyboard-interactive，除非技术验证证明目标服务器必须依赖它
- Remote Forward
- Dynamic Forward / SOCKS
- Shell / PTY

### 7.3 数据转发

每个本地 TCP 连接对应一个 SSH `direct-tcpip` channel。数据复制必须：

- 双向异步传输
- 支持半关闭和正常 EOF
- 支持取消
- 不因单个 channel 失败而污染其他连接
- 设置合理的连接、认证和空闲超时
- 不记录业务数据内容

不要使用固定 sleep 判断 SSH 或服务是否 ready，应使用明确状态、连接结果或健康探测。

---

## 8. 生命周期规则

### 8.1 macOS

- App 前台和后台都可保持当前连接，直到用户断开或退出 App。
- 退出 App 时释放所有 SSH Session 和 Listener。
- V1 不注册 LaunchAgent，不提供无人值守中继服务。

### 8.2 iOS

- App 前台时维持连接。
- 不承诺进入后台后持续保持 SSH。
- 进入后台时记录恢复所需的非敏感状态。
- 回到前台后检测连接；失效时自动重连。
- 自动重连不得绕过 Host Key 变化确认。
- 不使用伪造音频、定位等后台模式维持 Tunnel。

---

## 9. 错误与状态模型

连接状态必须使用明确枚举，不得散落字符串：

```text
idle
validating
connecting_ssh
verifying_host_key
authenticating
opening_forward
checking_service
connected
reconnecting
disconnecting
disconnected
error
```

错误必须区分可操作原因，至少包括：

- invalid_profile
- dns_failed
- ssh_connect_timeout
- host_key_unknown
- host_key_changed
- authentication_failed
- private_key_invalid
- private_key_passphrase_required
- target_unreachable
- local_listener_failed
- service_http_error
- tls_error
- cancelled
- unsupported_platform

用户提示必须说明下一步，不得只显示 `Connection failed`。

Rust 业务路径禁止 `unwrap()`、`expect()` 和无说明 panic。可恢复错误统一使用 typed error，并在 Tauri 边界转换为稳定错误码。

---

## 10. 前端规则

- TypeScript 开启 `strict`。
- 禁止使用 `any` 绕过类型；无法避免时必须局部说明原因。
- 服务表单根据 `connection_type` 展示字段，禁止把无关字段混在一个不可理解的大表单中。
- iOS 和 macOS 使用响应式布局，不复制两套页面。
- 所有异步按钮必须有 loading、取消或防重复提交处理。
- 删除服务、替换 Host Key、删除凭据属于高风险操作，必须二次确认。
- UI 不展示本地随机端口，除非进入诊断信息页面。
- 不在 UI 中暴露 SSH 命令给普通用户。

产品核心流程必须保持：

```text
打开 App → 选择服务 → 自动连接 → 打开 Pi Hub
```

---

## 11. 数据模型与迁移

- 服务 ID 使用稳定 UUID，不使用数组下标或名称作为主键。
- 配置模型必须包含 `schema_version`。
- 每次结构变化都提供向前迁移，不允许简单清空用户配置。
- 敏感字段不得进入 profile 序列化结构。
- `direct_url` 和 `ssh_forward` 使用可判别联合类型，不使用大量可空字段表达类型。

示意：

```ts
type ServiceProfile = DirectUrlProfile | SshForwardProfile;
```

Rust 侧使用对应的 tagged enum，并保持前后端字段一致。

---

## 12. 测试规则

每个功能至少覆盖其风险层级。

### 12.1 单元测试

- Profile 校验和迁移
- URL / host / port 校验
- 状态机转换
- Host Key 比对
- 错误映射
- 凭据引用删除逻辑

### 12.2 Rust 集成测试

- 本地 Listener 只绑定 loopback
- 临时端口分配
- 多连接隔离
- 取消与资源释放
- 模拟 SSH Server 的密码和公钥认证
- `direct-tcpip` 数据双向转发
- Host Key 首次确认与变化阻断

### 12.3 前端测试

- 新增、编辑、删除两种服务
- 字段条件显示
- 连接状态展示
- 错误提示
- 断开与返回服务列表

### 12.4 真机 / 手工验收

V1 不能只在 macOS 模拟完成。至少验证：

- 一台真实 macOS 设备
- 一台真实 iPhone
- Direct HTTPS
- Direct HTTP 警告
- SSH Password
- SSH Ed25519 Key
- 带 Passphrase 的 Key
- iOS 前后台恢复
- Pi Hub 流式响应
- 文件选择、下载和复制操作

涉及 SSH、Keychain、WebView 或生命周期的修改，必须说明在哪个平台实际验证。

---

## 13. 开发流程

1. 先确认任务属于 V1。
2. 阅读相关需求和设计章节。
3. 检查现有实现与测试。
4. 先实现最小闭环，不做无需求重构。
5. 增加或更新测试。
6. 执行格式化、类型检查、Lint 和测试。
7. 更新文档中的行为、限制或决策。
8. 在提交说明中写清验证平台和未验证项。

不得为了让检查通过而：

- 删除测试
- 降低 TypeScript 严格度
- 大范围添加 lint ignore
- 跳过 Host Key 校验
- 把秘密写入测试数据或日志
- 将真实网络失败改成固定 sleep

---

## 14. 预期检查命令

项目初始化后应提供并维护等价命令：

```bash
npm run format:check
npm run lint
npm run typecheck
npm test
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
npm run tauri ios build
```

在证书或真机环境不足时，可以无法执行最后两项，但必须明确说明，不能声称已完成验证。

---

## 15. Definition of Done

一个任务只有同时满足以下条件才算完成：

- 行为符合 V1 需求。
- macOS 和 iOS 共享逻辑未被无理由分叉。
- 安全硬规则未被破坏。
- 错误路径和取消路径得到处理。
- 资源可正确释放。
- 新行为有测试。
- 相关文档已更新。
- 所执行的检查和未执行的检查均被如实记录。

若技术验证发现 Tauri、`russh`、WKWebView 或 Keychain 在 iOS 上存在阻塞，先提交可复现结论和替代方案，不得伪造完成状态。
