# Pi Hub Client V2 开发需求

- 仓库：`jiangliuhong/pi-hub-desktop`
- 产品名称：Pi Hub Client
- 文档版本：2.0
- 目标版本：V2 / `0.2.x`
- 目标平台：macOS 14+
- iOS 影响：保留 V1 客户端能力，不提供本机进程管理
- 状态：规划基线

---

## 1. 版本背景

V1 已经建立 Pi Hub Client 的基础连接能力：

- 配置 Direct URL 服务；
- 配置 SSH Forward 服务；
- 在受控 Service WebView 中使用现有 Pi Hub WebUI；
- 在 macOS 和 iOS 之间共享服务模型、连接核心和主要 UI。

但在本机使用场景中，macOS 用户仍需要先打开终端并手动执行：

```bash
pi-hub
```

然后再回到 Pi Hub Client 打开 `http://127.0.0.1:30142`。当 Pi Hub、Node.js 或 Pi 环境存在问题时，用户也缺少统一的诊断入口。

V2 增加两项能力：

1. **Local Pi Hub Manager**：检测本机 Pi Hub 的安装和运行状态，并在 Desktop 内启动、停止、重启和打开服务。
2. **Pi Environment Doctor**：检查 Pi Hub 运行所需的 Node.js、Pi Hub 安装、Pi 运行环境、数据目录、认证与模型基础状态。

---

## 2. 核心目标

V2 目标流程：

```text
打开 Pi Hub Client
        ↓
检测本机 Pi Hub 与 Pi 环境
        ↓
显示可操作的诊断结果
        ↓
按设置自动启动，或由用户点击启动
        ↓
等待本机 Pi Hub Ready
        ↓
直接在 App 内打开 Pi Hub
```

用户不需要：

- 手动打开终端；
- 手动执行 `pi-hub`；
- 自己查找 Node.js 或 Pi Hub 的安装路径；
- 通过 `lsof`、`ps` 或 Activity Monitor 判断服务状态；
- 根据一段模糊的启动错误自行排查 Pi 环境。

---

## 3. 重要产品定义

### 3.1 Pi Hub 与外部 Pi CLI 的关系

Pi Hub 当前直接依赖并调用 `@earendil-works/pi-coding-agent`、`pi-agent-core` 等 Pi 包。因此：

> 全局 `pi` CLI 不是启动 Pi Hub 的绝对硬依赖。

V2 必须区分：

- **Pi Hub 内嵌 Pi Runtime**：Pi Hub 安装中携带的 Pi 依赖，是本机 Pi Hub 工作的核心依赖。
- **External Pi CLI**：用户额外安装的 `pi` 命令，用于终端交互；缺失时默认只显示提示，不阻止 Pi Hub 启动。
- **Pi User Environment**：默认位于 `~/.pi/agent`，或由 `PI_CODING_AGENT_DIR` 指定，包含认证、设置、模型与会话数据。

禁止仅因为 `command -v pi` 失败，就把本机 Pi Hub 判定为不可用。

### 3.2 自动启动的含义

> **设计决策（手动检测模型）**：当前版本未实现「打开 App 时自动检测 / 自动启动」。本机 Runtime 的检测（`scan_local_installations`）、启动、停止、重启全部由用户在「This Mac」卡片手动触发；App 启动和窗口聚焦不再自动 refresh 或 auto-start。本节其余条文保留为原始 V2 需求基线，`auto_start_on_app_launch` 设置项与崩溃循环保护代码保留向前兼容；若未来恢复自动启动，以本节为需求基线。

V2 中“自动启动”专指：

> 打开 Pi Hub Client macOS App 时，自动拉起本机 Pi Hub。

V2 不实现：

- macOS 登录后自动运行；
- 系统开机时启动；
- Desktop 退出后继续常驻；
- LaunchAgent / LaunchDaemon；
- 无人值守 Relay Agent。

### 3.3 进程所有权

V2 必须区分：

- **Managed Process**：由当前 Pi Hub Client 进程启动，可以停止和重启。
- **External Process**：由终端、launchd 或其他程序启动，只能检测和打开，默认不得停止。

不能因为某个进程占用了 30142 就直接执行 Kill。

---

## 4. 平台范围

### 4.1 macOS

macOS 支持全部 V2 功能：

- 安装检测；
- Pi 环境检查；
- 本机状态检测；
- 启动、停止和重启；
- 打开 App 时自动启动；
- 本地日志与诊断；
- 在现有 Viewer 中打开本机 Pi Hub。

### 4.2 iOS

iOS 继续支持 V1 的 Direct URL 和 SSH Forward。

iOS：

- 不显示“本机 Pi Hub”管理卡片；
- 不扫描本机 CLI 或 Node.js；
- 不启动子进程；
- 不暴露本机 Runtime Tauri Command；
- 不因 V2 代码引入而破坏现有构建和连接功能。

---

## 5. V2 功能范围

### 5.1 本版本实现

- 本机 Pi Hub 固定卡片；
- Node.js 与 Pi Hub 安装发现；
- Pi 基础依赖检查；
- 本机端口和服务身份检查；
- 启动、停止、重启 Desktop 管理的 Pi Hub；
- 检测外部启动的 Pi Hub；
- 端口冲突识别；
- 打开 Desktop 时自动启动；
- 退出 Desktop 时停止本次管理的 Pi Hub，默认开启；
- 最近启动日志和脱敏诊断；
- 手动选择 Node.js / Pi Hub 安装；
- 复用 V1 Viewer 打开本机 Pi Hub。

### 5.2 本版本不实现

> 注：V2 仍不实现 Pi / Pi Hub 的自动安装/升级；该能力已在 V3 纳入范围（见 `docs/requirements-v3.md`）。以下“自动安装/升级 Node.js”“任意 Shell 命令配置”“强制停止外部 Pi Hub”等在 V2 与 V3 均不实现。

- 自动安装或升级 Node.js（V2 / V3 均不实现）；
- 原地执行 `npm install -g` 修改用户外部全局安装（V2 不实现；V3 按 `requirements-v3.md` §0.1 只管理当前 Node 工具链的 npm 全局 Pi / Pi Hub）；
- 自动安装、升级或卸载 Pi（V2 不实现；V3 受管安装/更新见 `docs/requirements-v3.md`）；
- 自动安装、升级或卸载 Pi Hub（V2 不实现；V3 受管安装/更新见 `docs/requirements-v3.md`）；
- 任意 Shell 命令配置；
- 强制停止外部启动的 Pi Hub；
- macOS 登录项或 LaunchAgent；
- Desktop 退出后后台常驻；
- Relay Server、Reverse Tunnel 或家庭电脑中继 Agent；
- Terminal、PTY、SFTP；
- Mac App Store 沙盒适配；
- iOS 本机 Runtime / 本机组件管理。

---

## 6. 用户场景

### 6.1 已安装但未运行

```text
This Mac
Pi Hub 0.0.x
已停止

[启动] [检查环境] [设置]
```

用户点击“启动”，Desktop 自动校验依赖、拉起服务、等待 Ready，并更新为“正在运行”。

### 6.2 已由 Desktop 启动

```text
This Mac
Pi Hub 0.0.x
正在运行 · 由 Pi Hub Client 管理

[连接] [停止] [重启]
```

### 6.3 由终端启动

```text
This Mac
Pi Hub 0.0.x
正在运行 · 外部启动

[连接]
```

默认不显示可直接执行的“停止”按钮，并说明应在原启动位置停止。

### 6.4 环境存在问题

```text
This Mac
需要处理 2 个问题

✕ Node.js 版本过低
! 尚未配置可用模型认证

[查看检查结果]
```

阻断问题修复前禁止自动启动；警告问题允许启动，但必须明确说明可能无法完成 Agent 任务。

### 6.5 打开 App 时自动启动

用户启用：

```text
打开 Pi Hub Client 时自动启动本机 Pi Hub
```

下次启动 App 时：

1. 后台执行依赖检查；
2. 已运行则复用；
3. 未运行且没有阻断项则启动；
4. 端口冲突或检查失败则保持主界面可用并显示通知；
5. 不自动进入 Pi Hub Viewer，除非未来增加独立设置。

---

## 7. 状态模型

### 7.1 安装状态

```text
unknown
not_found
invalid
incompatible
ready
```

### 7.2 运行状态

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

### 7.3 Pi 环境总体状态

```text
ready       所有阻断检查通过，基础 Agent 能力可用
degraded    服务可以启动，但存在认证、模型或可选 CLI 警告
blocked     Node、Pi Hub、内嵌 Pi Runtime 或数据目录等硬依赖不满足
unknown     尚未检查或检查结果过期
```

禁止使用一个简单的 `installed: true/false` 或 `online: true/false` 表达全部状态。

---

## 8. Pi 基础依赖检查

### 8.1 检查等级

每个检查必须包含：

- 稳定检查 ID；
- 分类；
- 状态；
- 严重级别；
- 面向用户的信息；
- 可执行的修复建议；
- 非敏感诊断详情。

严重级别：

```text
required     失败时阻止 Desktop 启动本机 Pi Hub
recommended  失败时允许启动，但功能可能不完整
informational 仅展示信息，不影响运行
```

### 8.2 必需检查

#### DEP-NODE-001 Node.js 可用

- 找到可执行 Node.js；
- 使用绝对路径执行版本检查；
- 版本满足 Pi Hub 当前要求，基线为 `>=22.19.0`；
- Node 路径可执行；
- 不依赖 Finder 启动时可能缺失的交互式 Shell PATH。

#### DEP-PIHUB-001 Pi Hub 安装有效

- 找到 `@jarome/pi-hub`；
- 包名、CLI 入口和版本可验证；
- 生产构建产物存在；
- Node.js 与 Pi Hub 入口形成有效安装组合；
- 安装路径经 canonicalize 后仍位于用户明确选择或可信发现路径中。

#### DEP-PI-EMBEDDED-001 内嵌 Pi Runtime 可用

- Pi Hub 的 `@earendil-works/pi-coding-agent` 等核心依赖可以加载；
- Pi Hub 与内嵌 Pi 包版本满足其自身兼容约束；
- 检查通过 Pi Hub 提供的机器可读 Doctor 契约完成，不由 Desktop 猜测 `node_modules` 内部结构。

#### DEP-PI-DIR-001 Pi Agent 数据目录可用

检查实际使用的目录：

```text
PI_CODING_AGENT_DIR
或
~/.pi/agent
```

要求：

- 路径可解析；
- 已存在时可读；
- 需要创建时父目录可写；
- Session、Settings 等正常操作所需目录可以创建；
- 不把目录不存在本身当成致命错误，只要可以安全创建。

#### DEP-SESSION-001 Session 目录可用

- Session 目录可读写或可创建；
- 不读取和上传具体会话内容作为检查的一部分；
- 权限错误属于阻断项。

### 8.3 建议检查

#### DEP-PI-AUTH-001 Provider 认证基础状态

- 只判断是否存在可用的 Provider 认证状态；
- 不输出 API Key、Bearer Token、Cookie 或 OAuth Refresh Token；
- 不把秘密传回前端；
- 基础检查默认不调用模型 API，不产生费用；
- 未配置认证时状态为 `degraded`，允许 Pi Hub 启动。

#### DEP-PI-MODEL-001 模型基础状态

- Pi Model Runtime 可以初始化；
- 至少存在一个可见模型时标记为通过；
- 默认模型缺失属于警告；
- 配置解析错误返回稳定错误码，不返回配置文件全文。

### 8.4 信息检查

#### DEP-PI-CLI-001 外部 Pi CLI

- 尝试发现独立 `pi` 命令；
- 找到后读取 `pi --version`；
- 支持 npm 安装和官方 standalone binary；
- 缺失时只显示“可选 CLI 未安装”，不得阻止 Pi Hub 启动；
- 不自动运行交互式 `pi`；
- 不自动调用会刷新凭据的命令。

### 8.5 检查结果时效

- App 启动时执行一次；
- 用户点击“重新检查”时立即执行；
- 安装路径或 Agent 目录设置变化后自动失效；
- 启动 Pi Hub 前若结果超过 60 秒，应重新执行阻断检查；
- 诊断结果不得永久缓存为真相。

---

## 9. 功能需求

### V2-FR-001 本机 Pi Hub 卡片

macOS 服务列表顶部显示固定的 `This Mac` 卡片，至少包含：

- Pi Hub 安装版本；
- Node.js 版本；
- 运行状态；
- 管理来源；
- 本地地址；
- 依赖检查摘要；
- 当前可执行操作。

本机卡片不是普通 `DirectUrlProfile`，不得与用户远程服务配置混为一体。

### V2-FR-002 自动发现安装

Desktop 按确定顺序发现 Node.js、Pi Hub 和可选 Pi CLI。

必须支持至少：

- 当前进程 PATH；
- `/opt/homebrew/bin`；
- `/usr/local/bin`；
- NVM；
- Volta；
- FNM；
- ASDF / Mise shim；
- 用户此前保存的绝对路径。

发现失败时允许用户手动选择安装。

### V2-FR-003 手动选择与验证

用户可以选择 Node.js 或 Pi Hub 入口。

保存前必须验证：

- 文件存在且可执行或可由 Node 执行；
- Pi Hub 包身份正确；
- 版本可读取；
- 入口和 package root 一致；
- 不是任意用户脚本。

禁止提供“自定义启动命令”文本框。

### V2-FR-004 执行依赖检查

用户可以打开完整 Doctor 页面并重新检查。

页面按以下分类展示：

- Runtime；
- Pi Hub；
- Pi Environment；
- Authentication & Models；
- Optional Tools。

检查失败必须提供具体下一步，不能只显示 `Dependency check failed`。

### V2-FR-005 检测运行状态

Desktop 必须同时检查：

1. 目标 loopback 端口是否可连接；
2. 目标服务是否通过稳定协议表明自己是 Pi Hub；
3. 是否存在当前 Desktop 持有的 Managed Process Handle。

由此区分：

- stopped；
- running_managed；
- running_external；
- port_conflict。

### V2-FR-006 启动本机 Pi Hub

用户点击启动时：

1. 重新确认阻断检查；
2. 检查端口；
3. 使用绝对 Node.js 和 Pi Hub 入口路径；
4. 以固定参数启动，并禁止自动打开浏览器；
5. 捕获 stdout / stderr；
6. 等待稳定健康接口；
7. 在超时或提前退出时返回可操作错误；
8. 保存当前进程为 Managed Process。

默认启动地址：

```text
http://127.0.0.1:30142
```

### V2-FR-007 停止服务

只有 `running_managed` 状态提供普通停止操作。

停止流程：

- 发送优雅退出信号；
- 等待限定时间；
- 超时后才执行强制终止；
- 终止完整受管进程组；
- 确认端口释放；
- 清理日志任务和状态。

### V2-FR-008 重启服务

重启仅适用于 Managed Process，等价于完整停止后重新检查并启动。

不得在旧进程未释放端口时启动第二个实例。

### V2-FR-009 外部服务处理

当 Pi Hub 已由外部程序启动：

- 显示 `running_external`；
- 允许直接打开；
- 默认不允许停止或重启；
- 不根据端口反查 PID 后直接 Kill；
- 提示用户在原启动位置管理服务。

### V2-FR-010 端口冲突

如果端口可连接但目标不是 Pi Hub：

- 状态为 `port_conflict`；
- 禁止启动；
- 不自动 Kill；
- 显示端口和建议；
- 允许用户修改本机 Pi Hub 端口。

### V2-FR-011 连接本机 Pi Hub

运行状态有效后，“连接”必须先通过 Rust 重新探测本机服务身份与运行状态，
然后使用 V1 已有 Viewer 在应用内打开本机 Pi Hub；不得交给系统浏览器。

必须继续保证：

- Service WebView 无 Tauri 原生能力；
- 本机 Pi Hub 页面不能调用 Process、Shell、Keychain 或本机 Runtime Commands；
- Pi Hub HTTP Basic Auth 凭据仍由 Keychain 管理。

### V2-FR-012 打开 App 时自动启动

设置默认关闭。

启用后：

- 在主窗口可用后异步执行；
- 不阻塞 App Shell 启动；
- 已运行时复用；
- 存在阻断项或端口冲突时不启动；
- 失败时显示一次可操作通知；
- 避免崩溃循环和无限重试。

### V2-FR-013 App 退出行为

新增设置：

```text
退出 Pi Hub Client 时停止由本次 App 启动的 Pi Hub
```

默认开启。

- 关闭窗口但 App 仍运行时不停止；
- `Cmd + Q` 或真正退出时执行；
- 外部启动的服务不受影响；
- 异常崩溃后遗留进程在下次启动时按 External Process 处理。

### V2-FR-014 日志与诊断

保存最近一次受管启动的有限日志：

- 启动时间；
- Node / Pi Hub 版本；
- 启动阶段；
- Ready 耗时；
- 退出码或信号；
- 最近固定行数 stdout / stderr。

日志必须有大小上限和轮转，默认不记录 Pi Hub HTTP 业务内容。

### V2-FR-015 设置

至少支持：

- 本机端口，默认 30142；
- 自动启动；
- 退出时停止；
- Node.js 绝对路径；
- Pi Hub 入口绝对路径；
- 可选 Pi Agent 数据目录覆盖；
- 可选 Pi Hub HTTP Credential 引用。

### V2-FR-016 状态刷新

- App 获得焦点时刷新；
- 启动、停止和设置变化后立即刷新；
- 前台运行时使用合理间隔健康检查；
- 不以高频端口轮询持续消耗资源；
- UI 只展示 Rust Runtime Manager 的状态。

### V2-FR-017 iOS 兼容

V2 合并后必须继续通过 iOS 编译，且 V1 Direct URL / SSH Forward 的行为和测试不退化。

---

## 10. Pi Hub 跨仓库集成契约

在实现 Desktop V2 前，`jiangliuhong/pi-hub` 需要提供稳定、机器可读的集成能力。

### 10.1 版本命令

```bash
pi-hub --version --json
```

返回非敏感 JSON，并直接退出。

### 10.2 Doctor 命令

```bash
pi-hub doctor --json --offline
```

要求：

- 不启动 HTTP Server；
- 不进入交互模式；
- 默认不刷新 OAuth；
- 不请求模型 API；
- 不输出任何凭据；
- 返回带 `schemaVersion` 的稳定 JSON；
- 使用退出码区分 ready、degraded、blocked 和内部错误。

### 10.3 服务身份接口

```http
GET /api/client-info
```

只返回：

```json
{
  "service": "pi-hub",
  "version": "0.0.x",
  "protocolVersion": 1
}
```

该接口：

- 不返回配置、会话或凭据；
- 保留现有 Host / Request Security 检查；
- 使用 `Cache-Control: no-store`；
- 用于区分 Pi Hub 与普通端口占用程序。

### 10.4 退出信号

Pi Hub CLI 必须将 `SIGTERM` / `SIGINT` 正确传递给 Next.js 子进程，并等待清理，避免 Desktop 停止外层进程后遗留服务。

---

## 11. 安全需求

### V2-SR-001 禁止通用 Shell

- 不使用 `sh -c`、`zsh -c` 或 shell 拼接命令；
- 不向前端暴露 `execute(command)`、`spawn(path,args)` 或 `kill(pid)`；
- 所有可执行文件使用经过验证的绝对路径；
- 参数由 Rust 固定构造。

### V2-SR-002 进程终止边界

- 只终止当前 Runtime Manager 持有 Handle 的进程组；
- 不根据进程名、端口或 PID 猜测所有权；
- App 重启后无法证明所有权的服务按外部进程处理。

### V2-SR-003 Pi 凭据

- Desktop 不读取或返回 `auth.json` 中的秘密；
- Doctor 结果只包含状态和非敏感 Provider 标识；
- 禁止调用 `pi auth ... --credentials`；
- 禁止把环境变量完整转储到日志。

### V2-SR-004 日志脱敏

日志不得包含：

- API Key；
- OAuth Token；
- Authorization；
- Cookie；
- Telegram Token；
- Private Key；
- Passphrase；
- 完整进程环境变量；
- 用户对话或文件内容。

### V2-SR-005 WebView 隔离

新增 Runtime Commands 只允许 Trusted App Shell 调用。Pi Hub Service WebView 继续保持零 Tauri Capability。

### V2-SR-006 本地监听

Desktop 管理的 Pi Hub 默认只能绑定 `127.0.0.1`。V2 不在本机管理页面提供 `0.0.0.0` 选项。

---

## 12. 非功能需求

### V2-NFR-001 启动性能

- 主窗口不等待完整 Doctor 才显示；
- 本机状态卡片可先显示 `checking`；
- 普通检查目标在 3 秒内完成；
- 单个外部命令必须有超时。

### V2-NFR-002 稳定性

- 启动超时默认 30 秒；
- 优雅停止等待默认 5 秒；
- 连续自动启动失败必须触发崩溃循环保护；
- 状态更新必须避免旧任务覆盖新结果。

### V2-NFR-003 可测试性

Detector、Doctor、HealthProbe 和 ProcessSupervisor 必须有独立接口，允许使用临时目录、假进程和 mock HTTP Server 测试。

### V2-NFR-004 向后兼容

V2 不迁移或破坏 V1 服务 Profile。Local Runtime Settings 使用独立 schema 和存储命名空间。

### V2-NFR-005 分发方式

V2 macOS 首选 Developer ID 签名、公证和 DMG 分发。因为需要运行用户安装在 App Bundle 外部的 Node.js / Pi Hub，Mac App Store 沙盒适配不属于本版本。

---

## 13. 页面需求

### 13.1 This Mac 卡片

```text
This Mac

Pi Hub 0.0.x · Node 24.x
● 正在运行（由 Client 管理）
http://127.0.0.1:30142

[连接] [停止] [重启]

环境：Ready
```

### 13.2 本机设置页

```text
Pi Hub Installation
Node.js
Pi Agent Directory
Local Port

[ ] 打开 Client 时自动启动
[x] 退出 Client 时停止受管服务

[重新扫描] [重新检查环境]
```

### 13.3 Environment Doctor

```text
Runtime
✓ Node.js 24.x
✓ Pi Hub 0.0.x

Pi Environment
✓ Embedded Pi Runtime
✓ Agent Directory
! Provider authentication not configured
! External pi CLI not found (optional)
```

每个失败项提供说明和修复建议，但 V2 不直接自动安装。

### 13.4 启动日志

只展示脱敏后的有限日志，并提供复制诊断摘要，而不是复制完整环境。

---

## 14. 验收标准

1. 使用 Homebrew Node 安装 Pi Hub 时可以自动发现并启动。
2. 使用 NVM 时，从 Finder 启动 App 仍能找到匹配的 Node 和 Pi Hub。
3. 使用 Volta 或 FNM 时可以发现，或允许手动选择后稳定保存。
4. Node 低于 22.19.0 时 Doctor 返回 blocked，禁止自动启动。
5. 未安装外部 `pi` CLI 时最多为 informational，不错误阻断 Pi Hub。
6. Pi Hub 内嵌 Pi Runtime 无法加载时 Doctor 返回 blocked。
7. `~/.pi/agent` 不存在但可创建时检查通过并给出明确说明。
8. Pi Agent 目录不可写时检查阻断并显示路径和建议。
9. 没有 Provider 认证时服务仍可启动，但状态为 degraded。
10. 点击启动后不会打开 Safari 或 Chrome。
11. 服务 Ready 后可直接在现有 Viewer 中使用。
12. Desktop 启动的 Pi Hub 可以正常停止，Next.js 子进程不残留。
13. 外部终端启动的 Pi Hub 被识别为 running_external，Desktop 不直接停止。
14. 30142 被非 Pi Hub 服务占用时显示 port_conflict，不 Kill 进程。
15. 自动启动开启后，重开 Desktop 能自动拉起本机 Pi Hub。
16. 已经运行时自动启动不产生第二个实例。
17. 连续失败不会无限循环拉起。
18. `Cmd + Q` 时按设置停止本次受管服务。
19. 日志和错误 DTO 不出现秘密或完整环境变量。
20. Service WebView 无法调用任何 Local Runtime Command。
21. iOS 端不显示本机管理功能，V1 连接功能保持可用。
22. V2 相关单元、集成、前端测试和 macOS 真机验收均有记录。

---

## 15. 开发阶段

### Phase 0：Pi Hub 集成协议

在 Pi Hub 仓库完成：

- `--version --json`；
- `doctor --json --offline`；
- `/api/client-info`；
- SIGTERM / SIGINT 子进程转发；
- 对应自动化测试。

### Phase 1：Detector 与 Doctor

- 安装发现；
- 路径验证；
- 状态 DTO；
- Pi Environment Doctor；
- This Mac 只读状态卡片。

### Phase 2：Process Supervisor

- 启动；
- Ready 检测；
- 停止；
- 重启；
- 进程组和资源回收；
- 日志缓冲。

### Phase 3：自动启动与设置

- 独立设置存储；
- App 启动时拉起；
- 退出清理；
- 崩溃循环保护。

### Phase 4：macOS 真机验收

覆盖：

- Homebrew；
- NVM；
- Volta / FNM；
- Finder 启动；
- 外部进程；
- 端口冲突；
- 启动崩溃；
- `Cmd + Q`；
- 签名、公证后的实际 App。

---

## 16. 完成定义

V2 只有在以下条件同时满足时才算完成：

- 两项需求均形成完整用户闭环；
- Pi Hub 跨仓库契约已经实现并有版本约束；
- 只允许管理 Desktop 自己启动的进程；
- Pi CLI 缺失不会被错误当成硬阻断；
- Doctor 不泄露任何凭据；
- 自动启动不会造成重复实例或崩溃循环；
- 本机 Pi Hub 页面仍处于零权限 Service WebView；
- macOS 真机完成 NVM、Homebrew 和外部进程场景验证；
- iOS 构建和 V1 功能没有回归；
- 所有未验证项被如实记录。
