# Pi Hub Client V3 开发需求

- 仓库：`jiangliuhong/pi-hub-desktop`
- 产品名称：Pi Hub Client
- 文档版本：3.0
- 目标版本：V3 / `0.3.x`
- 目标平台：macOS 14+
- iOS 影响：保留 V1 / V2 客户端能力，不提供本机包管理
- 关联设计：`docs/pi-and-pi-hub-package-management-design.md`
- 状态：规划基线

---

## 1. 版本背景

V1 建立远程 Pi Hub 连接能力；V2 建立本机 Pi Hub 的检测、Doctor、进程启停和环境检查能力。V2 明确把自动安装与升级排除在范围之外（`requirements-v2.md` §5.2、`AGENTS.md` §2.3）。

但在实际使用中，macOS 用户仍会碰到：

- 本机完全没有 Pi 或 Pi Hub，需要先去终端执行 `npm install -g`；
- 已有外部安装但版本过旧，且用户不知道如何安全更新；
- 同时使用 Homebrew、NVM、Volta 等多套 Node 环境，原地升级风险高；
- Desktop 无法判断是否存在可用更新。

V3 引入“本机组件管理”能力，把检测、版本检查、初次安装和更新统一收进设置页，并在不修改用户外部环境、不请求 `sudo` 的前提下提供 Desktop 受管的软件副本。

> **范围前提**：V3 把 V2 的“不自动安装/升级”约束升级为“按本文档规则受管安装/升级”。V2 原有的检测、Doctor、进程管理和 Runtime 契约继续生效；V3 不重复 V2 的进程管理实现，而是复用并扩展现有模块。

---

## 2. 核心目标

V3 目标流程：

```text
打开 设置 -> 本机组件
        ↓
分别检测 Pi 与 Pi Hub 的本机安装与远端更新
        ↓
展示安装状态、版本、来源、路径和可用操作
        ↓
按需执行 Desktop 受管的初次安装或更新（用户显式点击）
        ↓
安装/验证/切换原子完成；失败保留旧版本
        ↓
Pi Hub 激活复用 Local Runtime 启停与 Doctor 契约
```

用户不需要：

- 手动打开终端执行 `npm install -g`；
- 自行处理 Homebrew / NVM / Volta 原地升级风险；
- 在缺少任一产品时离开 App；
- 猜测是否存在可用更新。

---

## 3. 重要产品定义

### 3.1 Pi 与 Pi Hub 是两个独立产品

- **Pi**：`package: @earendil-works/pi-coding-agent`，`binary: pi`，用于终端交互。
- **Pi Hub**：`package: @jarome/pi-hub`，`binary: pi-hub`，本机 Web 服务。

Pi Hub 安装包内部依赖 Pi Runtime，但两者不是同一安装对象：

- 更新外部 Pi，不改变 Pi Hub 内嵌 Pi Runtime；
- 更新 Pi Hub，可能随 Pi Hub 包版本一起更新内嵌 Pi Runtime；
- 外部 Pi 未安装不应阻止 Pi Hub 启动（继承 V2 §3.1）；
- 设置页必须明确展示这种关系，避免用户误以为两个版本必须一致。

### 3.2 受管副本模型（Managed Copy）

V3 的安装或更新目标始终是 Pi Hub Client 自己的用户级受管目录，不是原地修改用户的外部安装：

```text
~/Library/Application Support/Pi Hub Client/packages/
├── manifest.json
├── pi/        (versions/<v>/node_modules, staging, logs)
└── pi-hub/    (versions/<v>/node_modules, staging, logs)
```

- 不执行 `npm update -g`、`brew upgrade`、`volta install`，不修改 NVM 当前版本的全局包；
- 外部安装（Homebrew / NVM / Volta / 手动路径）继续展示，但更新动作是“安装受管版本”，不能暗示外部环境已被修改；
- 受管副本支持原子切换和回滚；
- 第一阶段不把受管 Pi 写入终端 PATH。

### 3.3 Node.js 仍是前置条件，不是受管对象

- 第一阶段不自动安装或升级 Node.js（V3 非目标）；
- Node.js 缺失或版本不兼容时，安装按钮显示阻断原因和修复建议；
- “零依赖首次安装”需要另行设计受管 Node Runtime，不在本阶段。

### 3.4 进程所有权（继承 V2 §3.3）

V3 必须延续 V2 的进程所有权规则：

- `running_managed`：当前 App 持有 Child Handle，可停止/重启；
- `running_external`：服务存在但无 Handle，只能打开；
- Pi Hub 更新必须尊重所有权：受管可“更新并重启”，外部只能下载受管副本后等待用户稍后激活；
- npm 安装子进程本身也是受管子进程，只能终止 Desktop 持有 Handle 的进程组。

---

## 4. 平台范围

### 4.1 macOS

macOS 支持 V3 全部功能：

- Pi 与 Pi Hub 的独立检测与状态展示；
- 联网检查 stable 最新版本；
- Desktop 受管的初次安装与更新；
- 安装事务、取消、回滚和有限脱敏日志；
- 与 V2 Local Runtime 启停、Doctor、外部进程识别集成。

### 4.2 iOS

iOS 继续支持 V1 / V2 既有能力。

iOS：

- 不显示“本机组件 / 包管理”入口；
- 不扫描本机 CLI、Node.js 或 npm；
- 不启动子进程或安装事务；
- 包管理 Tauri Command 返回稳定的 `unsupported_platform` 错误；
- 不因 V3 代码引入而破坏现有构建和连接功能。

---

## 5. V3 功能范围

### 5.1 本版本实现

- 独立的 Pi 与 Pi Hub 安装状态模型（区分未安装 / 已安装 / 损坏 / 不兼容 / 未知）；
- 强化外部 Pi CLI 检测（package identity、bin、版本、engine、来源分类）；
- 联网查询 stable `latest` 版本、缓存、离线降级；
- Desktop 受管的初次安装（分别安装与一键安装）；
- Desktop 受管的更新（含 Pi Hub 的“更新并重启”）；
- npm toolchain 检测（绝对 Node + 绝对 `npm-cli.js`）；
- staging 安装、严格后置验证、原子激活与旧版本保留；
- 安装/更新事务、操作锁、取消与 generation 防覆盖；
- 安装失败清理、回滚与重启恢复；
- 独立的设置页“本机组件”入口与卡片；
- 有限、脱敏的安装操作日志；
- Service WebView 零能力不变；
- iOS 门控（不显示入口，命令返回 `unsupported_platform`）。

### 5.2 本版本不实现

- 自动安装或升级 Node.js；
- 自动更新 Desktop App 自身；
- 卸载或覆盖用户的外部全局安装；
- 执行 `sudo` 或写入 `/usr/local`、`/opt/homebrew`；
- 修改 Shell 配置或 PATH；
- 把受管 Pi 默认写入终端 PATH；
- 支持任意 npm package、registry URL 或安装参数；
- 在 iOS 上管理本机 Pi / Pi Hub；
- 后台静默更新；
- 强制停止不是当前 Desktop 实例启动的 Pi Hub；
- beta / 预发布通道；
- Mac App Store 沙盒适配。

### 5.3 与 V2 范围的关系

V2 的检测、Doctor、端口探测、进程管理和退出清理行为不变。V3 新增的包管理是独立领域，不把安装、npm 或事务逻辑塞进 `LocalRuntimeManager`；V3 只通过最小适配接口与 Local Runtime 协调 Pi Hub 激活。

---

## 6. 用户场景

### 6.1 两者均未安装，前置条件满足

设置 -> 本机组件：

```text
Node.js 前置条件：满足（Node 22.x · npm 10.x）

Pi
未安装

Pi Hub
未安装

[一键安装 Pi 与 Pi Hub]
```

点击后串行安装并验证两者；任一失败保留已成功项并显示“部分完成”。

### 6.2 已安装外部旧版本，有更新

```text
Pi
已安装 · 0.84.0 · Homebrew
有更新（最新 0.85.0）

[检查更新] [安装受管版本 0.85.0]
```

不修改 Homebrew 安装，安装新的 Desktop 受管副本。

### 6.3 Pi Hub 由 Desktop 管理，有更新

```text
Pi Hub
正在运行 · 由 Client 管理 · 0.0.x
有更新（最新 0.0.y）

[检查更新] [更新并重启]
```

点击后先在 staging 下载验证，再提示确认重启；激活后 Doctor 与 service identity 校验，失败自动回滚。

### 6.4 Pi Hub 由外部启动

```text
Pi Hub
正在运行 · 外部启动
有更新（最新 0.0.y）

[安装受管版本 0.0.y]（稍后激活）
```

不停止外部进程；激活需要用户稍后在原位置停止外部服务。

### 6.5 Node/npm 阻断

```text
Node.js 前置条件：不满足
✕ npm 不可用（无法执行受管安装）

Pi / Pi Hub 安装与更新按钮置灰，显示修复建议。
```

### 6.6 离线

本机安装状态照常展示，更新状态标记为 `unknown` 并显示最后成功检查时间，不清空已检测的安装信息。

---

## 7. 状态模型

### 7.1 单产品安装状态

```text
unknown        无法验证（不等于未安装）
not_installed  未发现可用安装
installed      已安装且有效
invalid        安装损坏（身份/版本/bin 不符）
incompatible   版本或 Node engine 不满足
```

### 7.2 更新状态

```text
unknown            未检查、离线或失败
checking           正在查询远端
up_to_date         已是最新 stable
available          有可用 stable 更新
newer_than_latest  当前版本高于 latest（禁止降级）
unavailable        无法获取版本（区分于 unknown 语义时使用）
```

### 7.3 操作类型与阶段

```text
操作类型: install / update / repair / activate

阶段: preparing
      fetching_metadata
      installing
      verifying
      awaiting_restart_confirmation
      activating
      restarting
      rolling_back
      completed
      cancelled
      failed
```

### 7.4 安装来源与所有权

来源（扩展 V2 `InstallationSource`，新增受管标记）：

```text
persisted / path / homebrew / nvm / volta / fnm / asdf / mise / manual
+ desktop_managed   Desktop 受管副本（V3 新增）
```

所有权：

```text
desktop_managed   Desktop active manifest 指向的受管安装
external          其他来源
```

禁止使用一个布尔值表达全部状态；前端不得自行推导权限，所有 `allowed_actions` 必须由 Rust 计算。

---

## 8. 安装与前置检查

### 8.1 检查等级（继承 V2 §8.1）

每个检查必须包含稳定 ID、分类、状态、严重级别、面向用户信息和可执行修复建议。严重级别沿用 V2。

### 8.2 Node.js 前置检查

- 找到可执行 Node.js（继承 V2 `DEP-NODE-001`）；
- 版本满足 Pi Hub 基线 `>=22.19.0`；
- 在 Node 可用但 npm 不可用时，检测仍可完成，但安装/更新被阻断。

### 8.3 npm toolchain 检查（V3 新增）

- 定位与 Node 配套的 npm CLI（优先同前缀 `npm-cli.js`）；
- 使用绝对 Node 执行绝对 `npm-cli.js --version` 校验；
- 短超时、stdin null、输出有界；
- 不执行 Shell，不用 `which` / `command -v`；
- 失败时给出修复建议，不清空已检测安装。

### 8.4 Pi 检测强化（V3）

在 V2 仅 `pi --version` 基础上增加：

1. canonicalize executable；
2. 定位 package root；
3. 验证 `package.json.name === "@earendil-works/pi-coding-agent"`；
4. 验证 `bin.pi`；
5. 解析严格 semver；
6. 验证 Node engine；
7. 区分 npm / standalone / desktop_managed / unknown；
8. standalone 只检测；更新时安装受管 npm 副本，不覆盖 binary。

### 8.5 Pi Hub 检测（继承 V2）

继承 V2 的候选验证、`.next` 生产构建和 `/api/client-info` 服务身份校验；V3 增加 Desktop active manifest 来源识别。

### 8.6 检查时机

- App 启动只执行本机快速扫描，不阻塞窗口；
- 打开设置立即返回缓存并触发刷新；
- “重新扫描”强制本机扫描；
- 安装/更新成功后强制扫描与 Doctor；
- 网络版本查询独立，失败不清空本机结果。

---

## 9. 远端版本检查

### 9.1 固定数据源

```text
Pi     -> @earendil-works/pi-coding-agent -> dist-tag latest
Pi Hub -> @jarome/pi-hub                   -> dist-tag latest
```

只解析 `dist-tags.latest`、对应 `version`、`engines.node`、`dist.integrity` 和可选发布时间。不得把 registry 原文传给前端或日志。

### 9.2 缓存策略

- 成功 TTL 6 小时；
- 用户手动检查忽略 TTL，但使用 ETag；
- 失败不覆盖最后一次成功结果；
- 失败后最短重试间隔 30 秒；
- 离线时 `update_status = unknown` 并显示最后成功时间。

### 9.3 版本策略

- 只使用 `latest` stable dist-tag；
- 不自动选择 prerelease；
- semver 比较，禁止字符串比较；
- 当前版本高于 latest 显示 `newer_than_latest`，禁止降级；
- 前端提交后端生成的短期 opaque release token，不能提交任意版本/package spec。

---

## 10. npm 工具链与固定命令

### V3-FR-001 npm 工具链对象

```rust
struct NpmToolchain {
    node_executable: PathBuf,
    npm_cli_js: PathBuf,
    npm_version: String,
    source: InstallationSource,
}
```

验证要求见 §8.3。

### V3-FR-002 固定安装命令

安装命令逻辑等价于：

```text
<absolute-node> <absolute-npm-cli.js>
install
--prefix <validated-staging-dir>
--no-save
--package-lock=false
--ignore-scripts
--no-audit
--no-fund
--omit=dev
<allowlisted-package>@<exact-version>
```

- Rust 逐项构造参数；
- 包名来自 `ProductId` 固定映射，版本来自 release token，staging path 由后端生成；
- 前端禁止传 registry、package、命令、参数、环境或目标目录；
- 默认 `--ignore-scripts`；若发布物不能支持，必须先改变发布方式或提交独立安全设计（见 §17 / §16 开放项）。

---

## 11. 功能需求

### V3-FR-010 独立产品状态模型

Rust 维护 Pi 与 Pi Hub 各自的安装状态、所有权、来源、版本和更新状态，并提供稳定 DTO。前端只展示 Rust 返回或事件推送的状态。

### V3-FR-011 强化检测与来源分类

检测必须区分 `desktop_managed` 与外部来源，并在展示层保留所有有效候选。对 Pi Hub，`/api/client-info` 只证明服务身份，不反推磁盘安装所有权。

### V3-FR-012 远端版本查询与缓存

支持 §9 全部要求；离线或失败时保留本机安装事实。

### V3-FR-013 初次安装

支持各自安装与“一键安装全部”：

1. 校验平台/operation lock；
2. 刷新本机状态；
3. 校验 Node/npm；
4. 获取并冻结 stable release；
5. 创建 staging；
6. 固定 npm install；
7. 严格后置验证；
8. rename 至 `versions/<version>`；
9. 原子写 active manifest；
10. 刷新 detector/Doctor；
11. 返回完成快照。

后置验证失败删除 staging，active manifest 不变。

### V3-FR-014 一键安装编排

两者均未安装时“一键安装”是两个独立事务的串行编排：

1. 获取一次版本元数据；
2. 安装并验证 Pi；
3. 安装并验证 Pi Hub；
4. 各自成功后立即提交各自 manifest；
5. 第二项失败不回滚已成功的第一项，显示“部分完成”；
6. 不默认启动 Pi Hub，完成后询问“现在启动”。

### V3-FR-015 更新

与初次安装共用事务并增加：

- 记录 `previous_active_version`；
- 新版本验证前不改当前路径；
- Pi Hub 运行时按 §11.5 协调；
- 激活后再次执行版本命令；
- Pi Hub 执行 Doctor 与 service identity 检查；
- 失败恢复旧 manifest 和 Runtime 路径。

### V3-FR-016 原子激活

- 同文件系统写临时 manifest，`fsync` 后 rename；
- 版本目录由 staging rename；
- 禁止在 active 目录原地 npm install、先删除旧版本、让 detector 选中半成品或仅用 symlink 表达事务。

### V3-FR-017 清理

- 每产品保留 active 和最近一个成功版本；
- 失败 staging 立即尽力清理；
- 启动时清理超过 24 小时且未被 journal 引用的 staging；
- 不删除 active/rollback 引用；
- 清理失败不把可用安装标记为损坏。

### V3-FR-018 并发与操作锁

- 全局最多一个 npm 安装事务；
- 本机扫描和网络版本查询可并行；
- Local Runtime 正在 start/stop/restart 时禁止 Pi Hub activation；
- 使用单调 generation id，旧事件不能覆盖新快照；
- 重复点击返回当前 operation，不启动第二个子进程。

### V3-FR-019 取消

- 下载/安装阶段允许取消；
- manifest 原子提交开始后暂不可取消；
- 只终止 manager 持有的 npm 子进程组；
- 不按 PID、进程名或端口猜测进程；
- 取消后清理 staging，保留 active；
- App 退出请求取消并有界等待。

### V3-FR-020 重启恢复

- staging 未提交：标记中断并清理；
- version 已就位、manifest 未切换：保留 inactive；
- manifest 已切换、Pi Hub 未验证：运行 detector/Doctor，失败提示回滚；
- 不自动重启或终止 external Pi Hub。

### V3-FR-021 Pi Hub 运行时更新协调

| Runtime 状态 | 行为 |
|---|---|
| stopped/failed | 安装、验证并立即激活 |
| running_managed | 先 staging 安装验证；激活前提示“更新并重启” |
| running_external | 可下载受管新版，但不得停止外部进程；等待用户稍后激活 |
| starting/stopping/checking | 禁止更新，等待当前操作结束 |
| port_conflict | 可下载，但不得启动或宣称切换成功 |

`running_managed` 流程：

1. staging 安装和验证不影响当前服务；
2. 用户确认后调用现有 `LocalRuntimeManager.stop()`；
3. 原子切换 active manifest 和 Local Runtime 路径；
4. 调用 `LocalRuntimeManager.start()`；
5. `/api/client-info` 与 Doctor 验证通过后完成；
6. 新版本启动失败：停止新进程、恢复旧 manifest/路径并尝试恢复旧版本；
7. 明确显示回滚成功或回滚失败。

### V3-FR-022 操作日志

每次操作保存有限结构化记录（operation_id、product、kind、from/to 版本、stage、时间、result_code、脱敏 summary）。保留最近 10 次或 30 天；单次日志限制行数和字节；脱敏 Authorization/Cookie/Token/API Key/Private Key/npm token；不记录完整环境、对话、Session、模型请求或凭据。

### V3-FR-023 设置页“本机组件”入口

- 通用设置保留“外观”；
- 新增“本机组件”（macOS only），含 Node.js 前置条件、Pi、Pi Hub；
- 现有 `/local-runtime/settings` 保留为高级设置页；
- 设置页只组合导航，不直接实现包管理状态机。

### V3-FR-024 组件卡片

每个产品卡片显示状态、当前版本、最新版本、来源、位置（可展开），以及按 `allowed_actions` 计算的主操作；Pi Hub 额外显示 Runtime 状态、是否由当前 Desktop 管理、更新是否需要重启。

### V3-FR-025 进度展示

显示阶段（准备中 -> 获取版本信息 -> 下载并安装 -> 验证 -> 切换版本 -> 完成），含产品、操作类型、阶段、已耗时、取消按钮、脱敏错误和日志入口。npm 无稳定字节级进度契约，第一阶段不伪造百分比。

### V3-FR-026 iOS 兼容

V3 合并后必须继续通过 iOS 编译，V1 / V2 功能和测试不退化；包管理命令返回稳定的 `unsupported_platform`，UI 隐藏入口。

---

## 12. Tauri Commands 与事件

### V3-FR-030 Commands

```rust
get_package_management_status()
scan_managed_products()
check_product_updates(force: bool)
start_product_install(product: ProductId, release_token: String)
start_product_update(product: ProductId, release_token: String)
confirm_pi_hub_update_restart(operation_id: Uuid)
cancel_package_operation(operation_id: Uuid)
activate_managed_product(product: ProductId, installation_id: Uuid)
get_package_operation_log(operation_id: Uuid, limit: Option<u32>)
```

- Command 只校验枚举/UUID/token 并调用 manager；
- 前端不能传 package、version spec、命令、参数、PID、路径或环境；
- token 绑定 product、exact version 和元数据，短期有效；
- iOS 返回稳定 `unsupported_platform`；
- 只注册给 Trusted main window。

### V3-FR-031 Events

```text
package-management://status-changed
package-management://operation-changed
```

payload 只含 DTO，不含 registry/npm 原始输出、完整环境、凭据或未白名单路径。低频阶段事件用于 UI，不逐行广播 npm 输出。

---

## 13. 存储与迁移

### V3-FR-040 独立 store

```text
Application Support/Pi Hub Client/
├── profiles.json
├── local-runtime.json
└── package-management.json   (schema_version, 两个 active installation, Node 路径, package root, 最后检查时间)
```

不含 Secret。

### V3-FR-041 迁移规则

1. Desktop 升级时不自动复制或移动外部安装；
2. 继续读取 V2 `local-runtime.json` 的 Pi Hub 路径；
3. 首次切换受管 Pi Hub 后，原子更新 package manifest 和 Runtime paths；
4. 两份 store 更新使用可恢复 journal；
5. 切回外部安装只改变选择，不删除受管版本；
6. Secret 不进入新 store。

### V3-FR-042 ID 与 schema

- ID 使用稳定 UUID；
- 包含 `schema_version`；
- 结构变化提供向前迁移，不清空用户配置；
- 安装路径保存前 canonicalize，使用前重新验证。

---

## 14. 错误模型

所有错误必须提供稳定错误码和下一步建议。完整错误码见 `pi-and-pi-hub-package-management-design.md` §16，至少包含：

```text
package_platform_unsupported
package_operation_in_progress
package_node_unavailable
package_npm_unavailable
package_release_check_failed
package_release_invalid
package_release_token_expired
package_install_spawn_failed
package_install_failed
package_install_timeout
package_verification_failed
package_activation_failed
package_update_requires_restart
package_external_runtime_active
package_rollback_failed
package_cancelled
package_disk_space_insufficient
```

错误 DTO 只含 product、stage、版本、required bytes 等白名单字段，不直接返回 npm stderr。

---

## 15. 安全需求

### V3-SR-001 禁止通用 Shell（继承 V2-SR-001 并扩展到 npm 子进程）

- 不使用 `sh -c`、`zsh -c` 或 shell 拼接命令；
- 不向前端暴露 `execute(command)`、`spawn(path,args)` 或 `kill(pid)`；
- npm 安装使用验证过的绝对 Node/npm CLI；
- 参数由 Rust 固定构造；
- stdin 关闭，输出有界并脱敏；
- 只终止 operation 持有的 npm child handle/进程组。

### V3-SR-002 进程终止边界（继承 V2-SR-002）

- 只终止当前 manager 持有 Handle 的 npm 进程组；
- 不根据进程名、端口或 PID 猜测所有权；
- App 重启后无法证明所有权的 Pi Hub 按 external 处理。

### V3-SR-003 文件系统边界

- 写入必须位于 canonicalized 受管根；
- 拒绝 `..`、跨根 symlink 和非预期路径；
- package symlink 不得逃逸受管根；
- manifest/journal 原子写入；
- 不写入 `~/.pi/agent` 会话和认证数据；
- 更新不得清空或迁移用户 Session。

### V3-SR-004 网络

- 只使用 HTTPS 和系统证书校验；
- 不允许忽略证书错误或 UI 任意下载 URL；
- registry/package 固定映射；
- npm audit 关闭，减少额外请求；
- 第一阶段只支持公开包，不读取/回传 npm token；
- 元数据和日志不得包含 Cookie/Authorization。

### V3-SR-005 凭据（继承 V2-SR-003/004）

- 包管理 store 不保存任何 Secret；
- 日志、事件、错误 DTO 不得出现 API Key、OAuth Token、Authorization、Cookie、npm token、Private Key；
- 不允许主动打印 Secret 后只依赖正则擦除。

### V3-SR-006 WebView 隔离（继承 V2-SR-005）

新增 V3 包管理 Commands 只允许 Trusted App Shell 调用。Pi Hub Service WebView 继续保持零 Tauri Capability；不得为安装功能扩大其权限。

### V3-SR-007 后置验证

- **Pi**：package name、exact version、`bin.pi`、入口 canonical path、Node engine、`pi --version`；
- **Pi Hub**：package name、exact version、`bin.pi-hub`、`.next` 生产构建、Node engine、`pi-hub --version --json` schema、离线 Doctor，以及启动后 `/api/client-info`。

---

## 16. 非功能需求

### V3-NFR-001 性能

- 设置页先返回缓存快照，后台刷新；
- 单个外部命令必须有超时；
- 不阻塞主窗口显示。

### V3-NFR-002 稳定性

- npm 安装必须有超时；
- 原子激活使用临时文件 + rename；
- 连续安装失败不进入无限循环；
- 状态更新避免旧任务覆盖新结果（generation）。

### V3-NFR-003 可测试性

- ReleaseClient、NpmToolchainDetector、PackageInstaller、PostInstallVerifier、ManagedPackageStore 必须有独立接口，可用 mock registry / mock npm / tempdir 测试；
- 不依赖真实网络或真实 npm 的测试。

### V3-NFR-004 向后兼容

- 不迁移或破坏 V1 / V2 配置；
- V3 package-management store 使用独立 schema 和命名空间；
- 继续读取 V2 `local-runtime.json`。

### V3-NFR-005 分发

继承 V2 `V2-NFR-005`（Developer ID 签名、公证、DMG；非 Mac App Store）。

---

## 17. 跨仓库契约与开放前提

进入功能实现前必须确认（对应设计文档 §22 Phase 0 / §24）：

1. `@earendil-works/pi-coding-agent` 与 `@jarome/pi-hub` 的所有发布物是否都能在 `--ignore-scripts` 下完整运行；
2. 是否接受受管 Pi 默认不写入终端 PATH（PATH 集成需独立授权设计）；
3. registry 是否固定为官方公共 registry（镜像/代理需独立信任和凭据设计）；
4. Pi Hub 更新后的数据格式是否向前兼容（若有不可逆迁移，回滚需增加数据兼容检查）；
5. 第一阶段固定 stable only；beta channel 留待后续。

其中第 1 项与版本命令稳定 schema 确认依赖 `jiangliuhong/pi-hub` 与 `@earendil-works/pi-coding-agent` 仓库；这些必须在各自仓库验证并记录结论，不得在 Desktop 侧用临时猜测逻辑绕过。开放前提未关闭前，V3 实现应保持本需求文档定义的范围，不得擅自扩大。

---

## 18. 页面需求

### 18.1 设置 -> 本机组件

```text
设置
├── 通用
│   └── 外观
└── 本机组件            (macOS only)
    ├── Node.js 前置条件
    ├── Pi
    └── Pi Hub
```

### 18.2 产品卡片

```text
Pi
用于终端交互；不影响 Pi Hub 内嵌 Runtime

状态：已安装 / 未安装 / 有更新 / 安装损坏 / 不兼容
当前版本：0.84.0
最新版本：0.85.0
来源：Desktop 受管 / Homebrew / NVM / Volta / 手动路径
位置：~/Library/.../pi（可展开）

[检查更新] [更新]
```

### 18.3 安装进度

显示阶段、产品、操作类型、已耗时、取消、脱敏错误、日志入口。

---

## 19. 验收标准

1. 设置页分别展示 Pi / Pi Hub 安装状态、版本、来源和路径。
2. 联网可检查 stable 最新版本；离线时本机状态仍准确。
3. 任一产品缺失时可完成 Desktop 受管初次安装。
4. 两者均缺失且 Node/npm 满足时，可分别或一键安装。
5. 任一产品有更新时可更新到精确 stable 版本。
6. 外部安装更新不修改 Homebrew/NVM/Volta，而是安装受管副本。
7. 不使用 Shell、不请求 `sudo`、不接受任意 package/path/args。
8. 失败、验证失败或取消不破坏 active 版本。
9. `running_managed` Pi Hub 更新需确认并复用现有 supervisor。
10. `running_external` Pi Hub 不会被停止或 Kill。
11. 新 Pi Hub 启动失败时恢复旧 active 版本并报告结果。
12. 安装有阶段、取消、有限脱敏日志和稳定错误码。
13. Service WebView 无法调用包管理命令。
14. iOS 编译和 V1 / V2 不受影响且不显示入口。

---

## 20. 开发阶段

### Phase 0：范围与契约

- 登记 V3；
- 修订 `AGENTS.md` 中安装/升级的版本范围；
- 更新 requirements；
- 确认两个发布包支持 `--ignore-scripts`（依赖跨仓库）；
- 确认版本命令稳定 schema（依赖跨仓库）。

### Phase 1：只读状态

- 强化 Pi 检测；
- 每产品状态模型；
- release 查询和缓存；
- 设置页展示安装/版本/更新状态；
- 不执行安装写入。

### Phase 2：初次安装

- npm toolchain detector；
- managed store、staging、journal；
- 独立安装和一键安装；
- 取消、日志和后置验证。

### Phase 3：更新与回滚

- Pi 更新；
- stopped Pi Hub 更新；
- `running_managed` 更新并重启；
- external 等待切换；
- 启动失败自动回滚；
- 旧版本清理。

### Phase 4：分发验收

- 真机矩阵；
- 签名、公证、DMG；
- 网络/磁盘异常；
- iOS 回归；
- README、用户帮助和诊断文档。

---

## 21. 完成定义

V3 只有在以下条件同时满足时才算完成：

- V3 需求、设计文档与 `AGENTS.md` 版本范围一致；
- 检测、版本查询、安装、更新、取消、回滚均有 typed state 和测试；
- 外部安装和 external Pi Hub 未被越权修改或终止；
- active 切换原子，异常退出可恢复；
- 不泄露 registry/npm/环境/认证 Secret；
- Pi Hub 更新复用 Local Runtime 和 service identity 契约；
- macOS 真机矩阵完成并记录；
- iOS 构建与 V1 / V2 回归通过；
- 所有执行和未执行的检查如实记录。
