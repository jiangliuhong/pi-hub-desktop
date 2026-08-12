# AGENTS.md

本文件是 `pi-hub-desktop` 仓库内所有 AI 编码代理和开发者的最高优先级项目规则。开始修改代码前，必须先阅读本文件以及与任务对应的文档：

- `docs/requirements-v1.md`
- `docs/design-v1.md`
- `docs/requirements-v2.md`
- `docs/design-v2.md`
- `docs/requirements-v3.md`
- `docs/pi-and-pi-hub-package-management-design.md`

V1 文档定义远程 Pi Hub 连接能力；V2 文档定义 macOS 本机 Pi Hub 管理与 Pi 环境诊断；V3 文档定义 macOS 本机 Pi / Pi Hub 组件的检测、版本检查、受管安装与更新。实现与文档冲突时，先修正文档或明确记录设计决策，不得静默扩大范围。

---

## 1. 项目定义

仓库名称为 `pi-hub-desktop`，产品显示名称为 **Pi Hub Client**。

本项目不是 Pi Hub 的重写，而是 Pi Hub 的 macOS / iOS 客户端：

### V1

1. 保存多个 Pi Hub 服务配置。
2. 直接打开可访问的 Pi Hub URL。
3. 自动建立 SSH Local Port Forward 后打开 Pi Hub。
4. 在 App 内使用现有 Pi Hub WebUI。

### V2

1. 在 macOS 检测本机 Node.js 与 Pi Hub 安装。
2. 检测本机 Pi Hub 的真实运行状态。
3. 启动、停止和重启由当前 Desktop 实例管理的 Pi Hub。
4. 检查 Pi Hub 内嵌 Pi Runtime、Agent 目录、Session、认证和模型基础状态。
5. 按用户设置在打开 Desktop 时自动启动本机 Pi Hub。（当前版本未实现启动时自动检测/自动启动，改为手动模型，见 §10。）

### V3

1. 分别检测本机 Pi 与 Pi Hub 的安装、来源、版本与更新状态。
2. 联网检查 stable 最新版本（缓存、离线降级、不降级）。
3. 只检测当前 Node.js 工具链中通过 npm 全局方式安装的 Pi / Pi Hub，并允许用户显式安装或升级（不请求 `sudo`）。
4. 安装/更新使用固定包名、固定参数、配对的绝对 Node/npm CLI、后置验证与有限脱敏日志。
5. 与 V2 Local Runtime 启停、Doctor、外部进程识别集成；iOS 不提供本机组件管理。

核心业务仍由 `pi-hub / pi-web` 提供。本仓库不得复制 Pi Hub 的会话、Agent、Scheduler、Workspace、模型或认证业务实现。

---

## 2. 固定平台与范围

### 2.1 平台

- macOS 14+
- iOS 17+

必须使用同一个 Tauri 2 工程和同一套 React / TypeScript 前端。共享逻辑优先放在 Rust 中，平台差异通过明确适配层处理。

V2 本机 Runtime 与 V3 本机组件包管理只在 macOS 生效；iOS 必须继续编译并保持 V1 / V2 功能，不得启动本地进程或显示本机 Runtime / 本机组件管理入口。

### 2.2 V1 服务类型

只允许：

- `direct_url`
- `ssh_forward`

`ssh_forward` 可以连接已经存在的 SSH 中继端点，但本仓库当前不负责创建中继端点。

### 2.3 当前版本不做

禁止未经需求变更自行加入：

- Relay Server
- 家庭电脑常驻 Relay Agent
- SSH Reverse Forward
- ProxyJump / 多级跳板
- SSH Terminal、PTY、SFTP
- VPN、系统代理或通用端口转发器
- Android、Windows、Linux
- 用户账号、云同步、团队协作
- Pi Hub WebUI 重写
- 后台永久维持 iOS Tunnel
- macOS 登录项、LaunchAgent 或 LaunchDaemon
- Desktop 退出后长期常驻 Pi Hub
- 自动安装或升级 Node.js（V3 仍是非目标；Pi / Pi Hub 的受管安装与更新已纳入 V3，必须按 `docs/requirements-v3.md` 与本文件 §9 规则执行）
- 任意 Shell 启动命令或环境变量编辑器
- Mac App Store 沙盒适配
- 后台静默更新、beta / 预发布通道、自动提权或写入当前 npm 全局 prefix 之外的目录（V3 非目标）

发现这些需求时，应记录到后续版本，不得顺手实现。V3 范围内的安装/更新能力必须严格遵守本文件 §9 与 `docs/requirements-v3.md`，不得静默扩大范围。

---

## 3. 技术栈

除非经过明确设计变更，否则使用：

- Tauri 2
- React
- TypeScript strict
- Vite
- Rust stable
- Tokio
- `russh`：SSH 客户端和 `direct-tcpip`
- Apple Security Framework / Keychain：敏感凭据
- Rust 侧版本化 Store：非敏感配置

不要仅为方便增加 Electron、Flutter、第二套 UI 工程或通用 Node sidecar。

依赖必须固定兼容版本并提交 lockfile。新增依赖前必须说明：

1. 解决的问题。
2. 为什么标准库或现有依赖不能解决。
3. 是否支持 macOS、`aarch64-apple-ios` 和 iOS Simulator。
4. 安全与维护风险。

---

## 4. 目录边界

优先保持：

```text
pi-hub-desktop/
├── src/
│   ├── app/
│   ├── features/
│   │   ├── services/
│   │   ├── connection/
│   │   ├── viewer/
│   │   ├── local-runtime/       # V2 macOS UI
│   │   └── package-management/  # V3 macOS UI
│   ├── components/
│   ├── lib/
│   └── types/
├── src-tauri/
│   ├── capabilities/
│   ├── src/
│   │   ├── commands/
│   │   ├── connection/
│   │   ├── credential/
│   │   ├── profile/
│   │   ├── ssh/
│   │   ├── viewer/
│   │   ├── local_runtime/       # V2 本机 Runtime 领域
│   │   ├── package_management/  # V3 本机组件包管理领域
│   │   ├── platform/
│   │   ├── error.rs
│   │   └── lib.rs
│   └── tauri.conf.json
├── docs/
├── AGENTS.md
└── README.md
```

不提前拆复杂 monorepo 或大量 crates。只有模块边界稳定且存在真实复用需求时才能拆 crate。

---

## 5. 架构规则

### 5.1 远程连接 Provider

UI 不得直接操作 SSH：

```text
ConnectionProvider
├── DirectUrlProvider
└── SshForwardProvider
```

未来新增 Relay 时应增加 Provider，而不是修改 UI 核心流程。

### 5.2 本机 Runtime 独立领域

本机 Pi Hub 不是普通 `DirectUrlProfile`。

```text
LocalRuntimeManager
├── InstallationDetector
├── PiEnvironmentDoctor
├── LocalServiceProbe
├── ProcessSupervisor
├── LocalRuntimeSettingsStore
└── RuntimeLogBuffer
```

不得把安装扫描、进程管理或 Doctor 逻辑塞进 V1 `ConnectionManager`。

### 5.3 Command 必须薄

Tauri command 只负责：

- 输入校验
- 调用领域服务
- 转换错误
- 返回可序列化 DTO

禁止在 command 中堆积 SSH、进程、存储、Doctor 或生命周期逻辑。

### 5.4 Rust 保存真实状态

SSH Session、本地 Listener、Managed Child、运行状态和操作 generation 的唯一真实状态必须在 Rust 侧。

前端只展示 Rust 返回或事件推送的状态，不得仅靠 React state 判断 Tunnel 或进程是否存在。

### 5.5 跨仓库契约

V2 允许因明确需求修改 `jiangliuhong/pi-hub`，但必须先实现并测试稳定协议：

```text
pi-hub --version --json
pi-hub doctor --json --offline
GET /api/client-info
SIGTERM / SIGINT 子进程转发
```

Desktop 不得以解析 `Ready` 文本、扫描 Pi Hub `node_modules` 内部结构或猜测端口内容作为长期方案。

V3 受管安装复用上述 Pi Hub CLI 契约（`--version --json`、Doctor）做后置验证；并依赖 npm registry `dist-tags.latest` 与包发布物在 `--ignore-scripts` 下可运行（契约确认与开放前提见 `docs/requirements-v3.md` §17）。

### 5.6 本机组件包管理领域（V3）

Pi / Pi Hub 的检测、版本检查、受管安装与更新是独立领域，不与 V2 Runtime 启停混在一起：

```text
PackageManagementManager
├── GlobalNpmInstallationDetector
├── ReleaseClient
├── NpmToolchainDetector
├── PackageInstaller
├── PostInstallVerifier
└── OperationLogStore
```

包管理器不复制 Pi Hub 进程管理逻辑；Pi Hub 激活、停止和重启通过最小适配接口委托给 `LocalRuntimeManager`。

---

## 6. 安全硬规则

以下规则没有“先做出来再说”的例外。

### 6.1 凭据

以下数据只能存入 Apple Keychain：

- SSH Password
- SSH Private Key
- Private Key Passphrase
- Pi Hub HTTP 凭据

服务和 Runtime 设置只能保存 `credential_id`。禁止秘密出现在：

- JSON / Store / SQLite 明文字段
- Git 和测试快照
- panic 和日志
- URL query / user-info
- Tauri event payload
- Doctor JSON
- 完整环境变量转储

### 6.2 SSH Host Key

- 首次连接显示算法和 SHA-256 Fingerprint。
- Host Key 按 `host + port` 绑定。
- Host Key 变化时阻断连接。
- 禁止自动接受变化后的 Host Key。
- 禁止任何等价于 `StrictHostKeyChecking=no` 的逻辑。

### 6.3 SSH 本地映射

- 只绑定 `127.0.0.1`。
- 使用系统临时端口。
- 失败、断开、页面关闭和 App 退出时释放 Listener。
- 每个连接有独立生命周期。

### 6.4 Service WebView

Pi Hub WebView 按不可信内容处理：

- 不配置任何 remote capability。
- 不启用 `dangerousRemoteDomainIpcAccess`。
- 不得读取 Keychain、Store、文件或 Runtime 状态。
- 不得调用 Shell、Process、Dialog 或任何 Tauri Command。
- 外部链接默认交给系统浏览器。
- 只允许当前服务 origin 和经过验证的内部导航。

新增 V2 Runtime 或 V3 包管理 Commands 后，也不得授予 Service WebView 调用权限。

### 6.5 HTTP / TLS

- Direct URL 的 HTTP 必须显示明文警告。
- HTTPS 使用系统证书校验。
- 不提供忽略证书错误。
- 不全局关闭 ATS，只配置必要的最小 loopback 例外。
- Desktop 管理的本机 Pi Hub 强制绑定 `127.0.0.1`，不提供 `0.0.0.0` 设置。

### 6.6 本机进程

- 禁止 `sh -c`、`zsh -c` 和字符串拼接命令。
- 禁止向前端暴露通用 `execute`、`spawn`、`kill`。
- 使用经过验证的绝对 Node.js 与 Pi Hub 入口路径。
- 启动参数由 Rust 固定构造。
- 只终止当前 App 内存中持有 Child Handle 的进程组。
- 不根据端口、进程名或 PID 猜测进程所有权。

V3 的 npm 安装子进程同样适用本节：只能执行验证过的绝对 Node/npm CLI，参数由 Rust 固定构造，只终止 manager 持有 Handle 的 npm 进程组，不按 PID/端口/进程名猜测所有权。

---

## 7. SSH 实现规则

V1 支持：

- Password authentication
- OpenSSH Ed25519 / RSA private key
- 加密私钥和 Passphrase
- SSH keepalive
- `direct-tcpip` Local Port Forward

不支持：

- SSH Agent
- FIDO2 / YubiKey
- PKCS#11
- Remote Forward
- Dynamic Forward / SOCKS
- Shell / PTY

每个本地 TCP 连接对应一个 `direct-tcpip` channel。转发必须双向异步、支持 EOF、取消和资源释放；不得记录业务数据。禁止固定 sleep 判断 ready。

---

## 8. V2 本机 Runtime 规则

### 8.1 Pi 依赖定义

Pi Hub 内嵌 `@earendil-works/pi-coding-agent` 等 Pi Runtime。独立的全局 `pi` CLI 不是 Pi Hub 启动硬依赖。

- 内嵌 Pi Runtime 加载失败：required / blocked。
- 外部 `pi` CLI 未安装：informational，不阻止启动。
- 禁止仅凭 `command -v pi` 失败判定 Pi Hub 不可用。

### 8.2 安装发现

至少支持：

- 已保存绝对路径
- App PATH
- Homebrew
- `/usr/local`
- NVM
- Volta
- FNM
- ASDF / Mise shim

候选必须 canonicalize，并验证 package name、bin entry、version、Node engine 和生产构建。Finder 启动时不能依赖 `.zshrc`。

### 8.3 Doctor

`pi-hub doctor --json --offline` 必须：

- 不启动 HTTP Server。
- 不进入交互模式。
- 不调用模型 API。
- 不刷新 OAuth。
- 不输出 Credential。
- 不调用 `pi auth ... --credentials`。
- 返回带 schema version 的稳定 JSON 和退出码。

Desktop 只解析白名单字段，不把 stdout 原文直接暴露给 UI。

### 8.4 服务身份

判断本机端口时必须调用 `/api/client-info` 并校验 `service` 与 `protocolVersion`。

- TCP 不通：stopped。
- 返回有效 Pi Hub：running managed/external。
- 端口可用但不是 Pi Hub：port conflict。

不得仅以端口可连接或 stdout 包含 `Ready` 作为成功依据。

### 8.5 启动

实际等价于：

```text
<absolute-node>
<absolute-pi-hub-entrypoint>
--hostname 127.0.0.1
--port <configured-port>
--no-open
```

- 设置 package root 为 cwd。
- Node 目录置于 PATH 首位。
- 不执行用户 shell 配置。
- 可选 `PI_CODING_AGENT_DIR` 来自受控设置。
- 可选 `PI_HUB_PASSWORD` 从 Keychain 注入。
- 不记录完整环境。

### 8.6 进程所有权

- `running_managed`：当前 App 持有 Child Handle，可停止和重启。
- `running_external`：服务存在但无 Handle，只能打开。
- App 崩溃或重启后无法证明所有权的进程按 external 处理。
- 禁止端口反查 PID 后直接 Kill。

### 8.7 自动启动

> **当前版本设计决策**：本机 Runtime 检测与启停已改为完全手动模型（见 §10）。App 启动和窗口聚焦不再自动 refresh 或 auto-start；以下「自动启动」条文保留为原始 V2 需求，`auto_start_on_app_launch` 设置项与崩溃循环保护代码保留向前兼容，但当前版本无启动调用点。若未来恢复自动启动，以本节为需求基线。

“自动启动”只表示打开 Desktop 时拉起本机 Pi Hub，不等于开机启动。

- 设置默认关闭。
- 不阻塞主窗口显示。
- 已运行时不重复启动。
- blocked / port conflict 时不启动。
- 连续失败必须有崩溃循环保护。
- 自动启动失败不得影响远程服务使用。

### 8.8 退出

设置允许在真正退出 App 时停止本次受管服务，默认开启。

- 关闭窗口但 App 仍运行时不停止。
- 外部服务不处理。
- 优雅停止超时后才强制终止受管进程组。

### 8.9 日志

- 有固定行数和文件大小上限。
- 默认只保留最近启动日志。
- 不记录对话、HTTP 业务数据或完整环境。
- 必须对 Authorization、API Key、Cookie、Token、Private Key 等脱敏。
- 不允许主动打印 Secret 后只依赖正则擦除。

---

## 9. V3 本机组件包管理规则

V3 在 V2 之外新增“本机组件管理”（Pi 与 Pi Hub 的检测、版本检查、受管安装与更新）。完整需求见 `docs/requirements-v3.md`，技术设计见 `docs/pi-and-pi-hub-package-management-design.md`。

### 9.1 范围与前置条件

- V3 把 V2 “不自动安装/升级 Pi、Pi Hub” 的约束升级为“按 V3 文档规则受管安装/升级”。
- Node.js 自动安装/升级在 V3 仍是非目标；Node.js 缺失或 npm 不可用时安装/更新被阻断。
- beta/预发布通道、后台静默更新、自动提权、写入配对 npm global prefix 之外的目录或 `sudo` 均为非目标。

### 9.2 npm 全局安装模型

- 页面只把当前选定 Node.js 配套 npm 的 global root 中、身份和入口验证通过的包视为已安装。
- PATH、Homebrew、standalone、手动路径或其他 Node 版本下的同名命令不得作为本页面的安装事实。
- 安装与升级等价于对配对 npm 的 global prefix 执行固定包名、精确版本的全局安装；不得请求 `sudo`。prefix 不可写时返回稳定错误和修复建议。
- Pi 与 Pi Hub 分别验证；Pi Hub 升级后的 Runtime 重启仍遵守进程所有权边界。

### 9.3 固定包名与命令

- 包名来自 `ProductId` 固定映射，版本来自后端短期 release token。
- 前端不得传 package、version spec、registry、命令、参数、PID、路径或环境。
- npm 安装等价于 `<absolute-node> <absolute-npm-cli.js> install --global --prefix <validated-global-prefix> --ignore-scripts --no-audit --no-fund --omit=dev <allowlisted-package>@<exact-version>`。
- 参数由 Rust 固定构造，stdin 关闭，输出有界并脱敏。

### 9.4 验证与失败处理

- npm 全局安装完成后必须验证 package identity、精确版本、bin、Node engine 与版本命令；Pi Hub 还验证生产构建和 Doctor 契约。
- npm 全局安装不具备 Desktop 私有 staging 的原子切换语义；失败时必须如实报告，不得宣称旧版本必然未变化。
- Pi Hub 由当前 Desktop 管理时，升级后通过 `LocalRuntimeManager` 重启；外部进程不得由 Desktop 停止或 Kill。

### 9.5 进程所有权与 Runtime 协调

- 包管理器不自行 Kill Pi Hub；Pi Hub 激活、停止和重启复用 `LocalRuntimeManager`。
- `running_managed` 更新需用户确认，升级后由 `LocalRuntimeManager` 重启；`running_external` 可以升级 npm 全局包，但不停止外部进程。
- npm 安装子进程本身也是受管子进程，只终止 manager 持有 Handle 的进程组。

### 9.6 版本与 Registry

- 固定官方公共 registry，HTTPS + 系统证书校验，不忽略证书错误。
- 只读 `latest` stable dist-tag；semver 比较，禁止字符串比较。
- 当前版本高于 latest 时显示 `newer_than_latest`，禁止降级。
- 不得把 registry 原文传给前端或日志。

### 9.7 文件系统与数据隔离

- 写入目标必须等于配对 npm 报告并验证的 global prefix；拒绝前端路径、`..`、跨 prefix 目标和非预期路径。
- 检测只接受 global root 内的 package；package 入口 canonicalize 后必须仍属于该 package root。
- 不写入或迁移 `~/.pi/agent` 会话、认证与 Session 数据。

### 9.8 日志与脱敏

- 安装操作日志有固定行数和大小上限，保留最近 10 次或 30 天。
- 不得记录完整环境、对话、Session、模型请求或凭据。
- 必须脱敏 Authorization、API Key、Cookie、Token、npm token、Private Key；不允许主动打印 Secret 后只依赖正则擦除。

### 9.9 iOS 门控

- iOS 不显示“本机组件”入口，不扫描 Node/npm，不启动安装事务。
- 包管理 Tauri Command 在 iOS 返回稳定 `unsupported_platform`。

---

## 10. 生命周期规则

### macOS

- V1 SSH 连接可持续到用户断开或 App 退出。
- V2 本机 Runtime 的检测与启停由用户在「This Mac」卡片手动触发；App 启动和窗口聚焦不再自动 refresh 或 auto-start。`auto_start_on_app_launch` 设置项保留向前兼容，但当前版本无启动调用点（详见 `docs/requirements-v2.md` §3.2 设计决策）。
- V3 npm 全局安装/更新按用户显式操作执行；App 退出时取消进行中的受管 npm 子进程，不长期常驻。
- 退出时释放 SSH、Listener 和受管进程资源。
- 当前版本不注册 LaunchAgent。

### iOS

- 前台维持连接。
- 不承诺后台持续 SSH。
- 回前台检测并重连。
- 不使用伪造后台模式。
- V2 本机 Runtime 与 V3 包管理命令返回 `unsupported_platform`，UI 隐藏入口。

---

## 11. 错误与状态

所有状态必须使用明确枚举，不得散落字符串。

V1 连接状态至少包括：

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

V2 本机状态至少包括：

```text
unknown
checking
stopped
starting
running_managed
running_external
stopping
port_conflict
failed
```

V3 包管理状态必须使用明确枚举（安装状态、更新状态、操作阶段）并提供稳定错误码；完整清单见 `docs/requirements-v3.md` §7 与 §14。

错误必须提供稳定错误码和下一步建议。Rust 业务路径禁止 `unwrap()`、`expect()` 和无说明 panic；可恢复错误统一使用 typed error。

---

## 12. 前端规则

- TypeScript strict，禁止无说明 `any`。
- iOS 和 macOS 使用响应式共享 UI。
- 所有异步按钮有 loading、取消或防重复提交。
- 高风险操作必须二次确认。
- UI 不保存进程或 SSH 真相。
- 普通用户界面不展示 SSH 命令或任意 Shell 命令；V3 安装页可以只读展示由后端固定映射生成的 npm 全局安装/升级命令并允许复制，但不得编辑或把该文本作为执行输入。
- macOS 服务列表顶部使用固定 `This Mac` 卡片，不把它保存为远程 Profile。
- blocked 时禁止启动；degraded 时允许启动但明确提示。
- external 状态默认不显示停止和重启操作。
- V3：UI 不自行推导安装/更新权限，`allowed_actions` 由 Rust 计算；external Pi Hub 不显示强制切换；iOS 隐藏“本机组件”入口。

---

## 13. 数据与迁移

- ID 使用稳定 UUID。
- 所有配置包含 `schema_version`。
- 结构变化提供向前迁移，不清空用户配置。
- 敏感字段不进入序列化模型。
- V1 Profiles、V2 Local Runtime Settings 与 V3 package-management store 使用各自独立存储和迁移。
- 安装路径保存前 canonicalize，使用前重新验证。
- V3 store 不保存任何 Secret；继续读取 V2 `local-runtime.json` 路径。

---

## 14. 测试规则

### V1

继续维护：

- Profile、URL、状态机、Host Key、凭据引用测试。
- 真 SSH Server 的认证与 `direct-tcpip` 集成测试。
- 前端服务 CRUD、状态、错误和 Viewer 测试。
- macOS / iPhone 真机 SSH、Keychain、WebView 验收。

### V2 单元与集成

至少覆盖：

- Homebrew / NVM / Volta / FNM 路径发现。
- symlink 与 package identity 验证。
- Node version requirement。
- Doctor schema、聚合和 Secret 脱敏。
- Agent / Session 目录权限。
- 本机状态机和 operation generation。
- mock client-info server。
- child early exit、启动超时、优雅停止、强制停止。
- duplicate start、port conflict、external refusal。
- crash-loop protection。
- 日志限制和 redaction。

### V3 单元与集成

至少覆盖：

- Pi 与 Pi Hub 包 identity、bin、version、engine。
- managed/external 分类与 not_installed/invalid/incompatible 区分。
- semver、prerelease、newer-than-latest。
- registry 白名单、ETag、TTL、离线缓存。
- release token 绑定和过期。
- npm 与 Node 配对及固定参数无 Shell。
- npm global prefix/root 一致性、package/bin symlink 路径逃逸。
- manifest 原子写、迁移、损坏恢复。
- generation、重复操作、取消只终止受管 npm child。
- 日志上限和 Secret 脱敏。
- 旧版本引用保护和清理。
- 两者均缺失一键安装、部分完成、回滚成功/失败。
- mock registry / mock npm / mock client-info 集成。

### macOS 真机

至少验证：

- Homebrew Node + npm Pi Hub。
- NVM。
- Volta 或 FNM 至少一项。
- Finder 启动签名 App。
- 外部终端 Pi Hub。
- 非 Pi Hub 端口冲突。
- Node 版本过低。
- Agent 目录不可写。
- 无认证 / 无模型。
- `Cmd + Q`。
- Developer ID 签名、公证和 DMG。
- V3：两者均无 / 单个 / 均有 npm 全局旧版本、Node 过低或 npm 缺失、无网络或 registry 5xx、global prefix 不可写、managed/external/port conflict 升级与重启、签名后 npm 全局目录权限。

### iOS 回归

V2 / V3 合并后仍需验证 iOS build、Direct URL、SSH Forward、Host Key、Viewer 和 Keychain；iOS 不显示本机组件入口，包管理命令返回稳定错误。

涉及 SSH、Keychain、WebView、进程、npm 安装或生命周期的修改，必须说明实际验证平台。

---

## 15. 开发流程

1. 确认任务属于 V1、V2 或 V3。
2. 阅读对应需求与设计章节。
3. 检查现有实现和测试。
4. 跨仓库契约先在 Pi Hub 实现，不写临时猜测逻辑。
5. 先完成最小闭环，不做无需求重构。
6. 增加或更新测试。
7. 执行格式化、类型检查、Lint 和测试。
8. 更新行为、限制和设计决策文档。
9. 提交说明写清已验证平台和未验证项。

### 15.1 macOS 客户端启动与版本确认

本仓库的 release bundle、debug bundle、`tauri dev` 进程以及可能安装到 `/Applications` 的 App 使用同一个 bundle identifier `top.jiangliuhong.pihubclient`。macOS 可能同时保留多个不同路径启动的实例；按显示名称或 bundle identifier 操作窗口时，也可能命中旧实例。另需注意：重新构建只更新磁盘产物，不会替换已经驻留内存的进程。因此曾出现以下问题：

- `target/release/bundle/macos/Pi Hub Client.app` 与 `target/debug/bundle/macos/Pi Hub Client.app` 同时运行；
- `npm run tauri -- dev` 已启动新后端，但 UI 自动化仍连接到旧 release 窗口；
- 在旧 debug 进程运行期间重新构建 debug bundle，随后打开 App 时仍复用了构建前进程；
- 使用 App 名称执行 `Cmd + Q` 只退出了其中一个实例，后续按名称读取状态又可能自动拉起旧 App；
- 仅根据窗口标题或 `tauri://localhost` 无法证明正在运行当前源码。

启动或重启客户端时必须执行以下流程：

1. **先枚举进程**：启动前用只读进程检查确认所有 `pi-hub-desktop` 实例的 PID 与绝对可执行路径，区分 release、debug bundle、`target/debug/pi-hub-desktop` 和 `/Applications` 安装版本。
2. **只终止已确认目标**：需要切换版本时，只退出绝对路径已确认的 Pi Hub Client 进程；不得按模糊进程名批量 Kill，不得影响 Pi Hub 服务或其他进程。退出后再次检查，确认旧实例确实消失。
3. **构建后再重启**：构建前已经运行的实例一律视为旧实例。`npm run tauri -- build --debug --bundles app` 或 release build 完成后，必须退出构建前实例，再从本次构建输出的绝对 `.app` 路径启动。
4. **开发模式保持唯一实例**：使用 `npm run tauri -- dev` 时，不得同时保留 release/debug bundle 实例。必须确认唯一客户端可执行文件是当前工作区的 `target/debug/pi-hub-desktop`，并保持 Tauri dev 命令会话运行。
5. **不要用模糊 App 标识验证**：存在或可能存在多实例时，不得仅用显示名称 `Pi Hub Client` 或 bundle identifier 启动、退出或判断版本；优先使用本次产物的绝对 App 路径。避免在“确认已退出”阶段调用会自动启动 App 的 UI 状态读取接口。
6. **用功能特征验收**：进程路径正确后，还必须打开本次修改对应页面，检查至少一个只有当前代码才具备的 UI/行为特征。涉及 Rust 后端时，应实际调用对应只读命令并核对结果，不能只看窗口成功打开。
7. **如实记录运行来源**：交付说明必须写清实际启动的是 `tauri dev`、debug bundle 还是 release bundle，并记录未验证的其他产物。不得把“新产物已构建”表述为“正在运行的客户端已更新”。

不得为了让检查通过而：

- 删除测试
- 降低 TypeScript 严格度
- 大范围 lint ignore
- 跳过 Host Key
- 关闭 TLS 校验
- 给 Service WebView 增加权限
- 把 Secret 写入测试或日志
- 使用固定 sleep 伪造 ready
- Kill 不属于当前 App 的进程
- 把外部 `pi` CLI 变成错误的硬依赖
- 用 `sh -c` / shell 拼接执行 npm、前端提交任意 package/version/参数
- 原地修改用户外部安装或请求 `sudo`

---

## 16. 预期检查命令

```bash
npm run format:check
npm run lint
npm run typecheck
npm test
npm run build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
npm run tauri ios build
```

证书或真机环境不足时可以无法执行最后两项，但必须明确说明，不能声称完成验证。

涉及 `jiangliuhong/pi-hub` 的 V2 / V3 契约（Doctor、版本命令、`--ignore-scripts` 可运行性）时，还必须运行该仓库与相关 Pi 包的 typecheck、lint、test 和发布前构建检查。

---

## 17. Definition of Done

任务只有同时满足以下条件才算完成：

- 行为符合当前版本需求和技术设计。
- V1、V2 与 V3 模块边界没有被破坏。
- macOS 和 iOS 共享逻辑未被无理由分叉。
- 安全硬规则未被破坏。
- 错误、取消、超时和资源释放得到处理。
- 只有明确受管的进程可被停止（V2 Pi Hub 与 V3 npm 安装子进程）。
- Doctor 无网络副作用且不泄露 Secret；V3 安装/更新日志同样不泄露 Secret。
- 新行为有对应测试。
- 相关文档已更新。
- 所执行和未执行的检查均被如实记录。
- macOS UI 验收已确认客户端的绝对进程/产物路径，且不存在会混淆结果的旧实例。

若 Tauri、`russh`、WKWebView、Keychain、进程组或 Pi Hub Doctor 契约存在技术阻塞，先提交可复现结论和替代方案，不得伪造完成状态。
