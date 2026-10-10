# 项目目录结构

本文用于定位实现和确定新增文件的归属。路径均相对仓库根目录；目录树只列维护入口，不穷举全部文件。进程分工与数据流见[架构说明](architecture.md)，环境准备见[构建与验证](build.md)。

## 顶层分工

| 路径 | 内容与维护边界 |
| --- | --- |
| `Cargo.toml`、`Cargo.lock` | 主 workspace 成员、共享依赖和依赖锁定 |
| `rust-toolchain.toml`、`.cargo/` | Rust 工具链与 Cargo 构建配置 |
| `cli/`、`skills/` | 控制台命令、离线帮助和随包分发的 Agent 操作指南 |
| `app/` | 主程序、窗口交互、内嵌资源及应用测试 |
| `crates/` | 领域模型、存储与 Windows 平台能力 |
| `installer/` | Inno EXE 脚本、MSI 定义、安装检查、安装标记与许可文本 |
| `tools/` | 常用打包与工具链入口；辅助脚本按 `packaging`、`validation`、`release`、`assets`、`analysis` 分类，见[脚本导航](../../tools/README.md) |
| `docs/` | 使用说明、开发指南、品牌说明与设计素材 |
| `screenshot/` | README 等文档使用的截图 |
| `.github/` | 构建、发布说明及发布刷新工作流与配置 |
| `target/` | 编译产物、打包输出、测试报告与本地实验数据；不作为源码目录 |

根目录的双语 README 面向使用者，CONTRIBUTING 面向贡献者，CHANGELOG 保存发布历史，PRIVACY 说明隐私政策。专题实现文档通过[开发文档导航](README.md)进入，避免在根目录重复维护技术说明。

主 workspace 包含 `app`、`cli` 和九个 `luciddesk-*` 库 crate。部分工具有自己的 `Cargo.toml`、`Cargo.lock` 和独立 workspace，例如 `tools/windows-bindings/`；根目录构建不会自动构建这些工具。

CLI 的 `main.rs` 负责参数和输出，`shortcuts.rs` 复用预览/提交流程，`next_step.rs` 生成结构化后续指引，`help.rs` 与 `help/topics.rs` 负责离线发现，`skill.rs` 内嵌完整技能包。`skills/luciddesk-control/SKILL.md` 是入口，详细规则按任务放入 `references/`；维护与分发约束见 [CLI 与 Agent](cli-agent.md)。

## 应用功能目录

```text
app/
├── Cargo.toml
├── build.rs                   # 应用构建脚本
├── assets/                    # 内嵌图标等资源与资源说明
├── locales/                   # 内嵌 Fluent 翻译资源
├── examples/                  # 应用相关探针与示例
├── tests/
│   └── canvas_compat.rs        # Canvas 兼容性集成测试
└── src/
    ├── main.rs                # 参数、单实例、身份和数据目录
    ├── desktop_component.rs   # 包标记与桌面 DLL 部署
    ├── app_icon.rs            # 应用图标资源访问
    ├── system_info.rs         # 系统信息与诊断报告
    ├── clipboard.rs           # Unicode 文本剪贴板
    ├── i18n.rs                # 语言解析、资源与格式化
    ├── tray.rs                # 托盘入口与生命周期
    ├── window_visibility.rs   # 窗口可见性辅助
    ├── updates/               # 应用更新查询与 HTTP 支持
    └── pane/
        ├── mod.rs             # 应用状态与功能入口
        ├── model.rs           # 窗口模型
        ├── events/mod.rs      # 操作分派
        ├── hybrid/            # mod.rs 运行入口；清单、图标、审计与改名事务
        ├── runtime.rs         # 通知、截止时间与故障重连
        ├── display_layout.rs  # 显示器布局
        ├── recovery.rs        # 备份、导入和恢复
        ├── folder/            # mod.rs 文件夹入口；导航、视图偏好、图像及测试
        ├── search/            # 搜索窗口、Everything 与快捷键
        ├── drag_drop/         # OLE 拖放、预览与临时描述
        ├── settings/          # mod.rs 入口；host、painter、布局、Agent 设置及测试
        ├── window/            # mod.rs 窗口入口；输入、菜单、调度及测试
        ├── control/           # CLI 控制、计划、布局与回执
        ├── render/            # mod.rs 面板绘制；tests.rs、bench.rs
        ├── canvas.rs          # 绘制作用域、文字与离屏读回
        ├── canvas/brushes.rs  # 固定数量的 GPU 画刷复用
        ├── composition.rs     # Surface、交换链与呈现背压
        ├── composition/recovery.rs # 窗口绘制失败的有限重试
        ├── native_graphics.rs # 图形绑定转换与生命周期
        ├── scaled_icons.rs    # CPU 缩放图像缓存
        ├── assets.rs          # 图像资源入口
        ├── assets/            # 图像资源辅助模块
        ├── acrylic/           # mod.rs 材质入口；运行时、画刷回退与兼容层
        └── tests.rs           # 面板集成回归
```

`pane/` 还包含主题、字体、菜单、重命名、动画和标签等共享模块。按功能查询文件比把所有文件堆入目录树更方便：

```powershell
rg --files app/src/pane
rg -n 'mod |pub.*use ' app/src/pane/mod.rs
```

### 按任务定位

| 要修改的行为 | 优先查看 | 专题说明 |
| --- | --- | --- |
| 桌面收纳与清单一致性 | `pane/hybrid/mod.rs`、`hybrid/inventory.rs`、`hybrid/audit.rs` | [成员过滤](hybrid-desktop.md) |
| 图标加载、刷新与回收 | `hybrid/icons.rs`、`hybrid/icon_changes.rs`、`hybrid/image_retention.rs` | [图标内存管理](memory-optimization.md) |
| 文件夹导航与视图偏好 | `folder/mod.rs`、`folder/entry_mode.rs`、`folder/preferences.rs` | [使用说明](../usage.md) |
| 搜索与全局快捷键 | `search/mod.rs`、`search/everything.rs`、`search/everything_settings.rs`、`search/hotkey.rs` | [后台调度](event-driven-runtime.md) |
| 设置布局与多语言 | `settings/layout.rs`、`i18n.rs`、`app/locales/` | [本地化指南](localization.md) |
| 绘制、材质和图形资源 | `render/mod.rs`、`native_graphics.rs`、`acrylic/` | [绘图与绑定](rendering.md) |
| MSIX 桌面 DLL 路径 | `app/src/desktop_component.rs`、`tools/package-msix.ps1` | [MSIX 打包](../msix.md) |

表中未写完整前缀的面板模块均位于 `app/src/pane/`；`i18n.rs` 位于 `app/src/`。搜索配置和快捷键保留在 `search/`，拖放描述保留在 `drag_drop/`；多个功能共享的窗口、模型和绘图能力留在共同父模块。

## 库与生成文件

| Crate | 实现入口与内部组织 |
| --- | --- |
| `luciddesk-api` | 本地控制协议、操作和响应模型；供主程序与 CLI 共用 |
| `luciddesk-diagnostics` | 日志等级、单行格式、限流、轮转与显式渲染追踪 |
| `luciddesk-core` | `src/lib.rs` 重导出领域类型；身份、坐标、外观、项目、面板与工作区分别维护 |
| `luciddesk-storage` | `src/store/` 管理配置、数据库、编解码、标签、显示器布局与恢复；公共错误位于 `src/error.rs` |
| `luciddesk-shell` | `src/` 按身份、桌面查询、通知、激活、COM/OLE、文件操作和菜单划分 |
| `luciddesk-explorer` | `src/discovery.rs` 负责发现与冲突检测；`filter.rs`、`filter/` 包含控制端、IPC 和 Explorer 端实现；`filter/menu/` 管理菜单宿主 |
| `luciddesk-menu` | `theme.rs`、`frame.rs` 提供应用与 Explorer 共用的原生菜单外观 |
| `luciddesk-graphics` | `src/layer.rs` 管理合成层；`src/bindings/` 保存生成的 DWM/DComp 绑定 |
| `luciddesk-window` | `src/lib.rs` 与 `src/monitors.rs` 提供错误提示及显示器能力 |

各库公共入口与依赖边界见 [crates 导航](../../crates/README.md)。`luciddesk-explorer` 同时包含控制端和 DLL 侧代码，定位问题时应先确认执行进程与线程，不能只按 crate 名判断运行位置。

生成绑定由 `tools/windows-bindings/` 维护，修改时同步生成器、筛选清单与输出。图标生成与验证分别由 `tools/assets/generate-app-icon.ps1`、`tools/assets/verify-app-icon.ps1` 负责；资源规则见[应用资源说明](../../app/assets/README.md)。

## 打包、测试与诊断工具

| 入口 | 用途 |
| --- | --- |
| `tools/package.ps1` | 普通包、便携包及 EXE / MSI 安装包 |
| `tools/package-msix.ps1` | 从普通生产包生成 MSIX |
| `installer/LucidDesk.iss`、`shortcut-cleanup.iss`、`installed` | 安装逻辑、快捷方式清理及安装标记 |
| `tools/ensure-inno.ps1`、`ensure-wix.ps1`、`build-msi.ps1`、`use-windows-toolchain.ps1` | 安装器编译器和 Windows 构建工具链准备 |
| `tools/validation/check-locales.py` | 翻译资源与调用检查 |
| `tools/validation/test-installer.ps1`、`test-package-lifecycle.py` | 安装器及真实应用生命周期回归 |
| `tools/render-diagnostics/` | 渲染比较、崩溃转储配置和收集 |
| `tools/release/release-notes.py`、`refresh-release.py` | 发布说明与发布内容维护 |

单元测试可内联在实现中，较大的测试模块拆到对应功能目录；例如 `app/src/pane/hybrid/icons/tests.rs`。跨模块集成测试位于各包的 `tests/`，独立探针通常位于 `examples/`。真实桌面和安装测试可能改变运行状态，执行要求见[验证与兼容边界](validation.md)。

`target/production`、`target/packages`、`target/portable`、`target/installers`、`target/msix` 等保存构建或打包输出。本地一次性探针与报告放在 `target/`；需要长期复用的验证工具应整理进 `tools/`，明确参数、作用范围和清理行为。

## 新增文件约定

1. 先按职责选择 `app`、对应 crate、`installer` 或 `tools`，再决定子目录。UI 交互留在应用层，领域模型不依赖窗口或数据库。
2. 功能专用实现放入所属功能目录；只有多个功能需要时才提升到共同父模块，并限制可见性，不为共享一个帮助函数扩大公共 API。
3. 使用常规 `mod` 规则。`name.rs` 配合 `name/` 和 `name/mod.rs` 均可，不为形式统一搬动现有模块。示例的 `#[path]` 和生成绑定的 `include!` 按实际用途维护。
4. 测试靠近被验证的行为，避免另建与源码平行但缺乏归属的测试树。生成文件修改同时更新生成来源。
5. 移动文件时同步 `mod`、相对路径、资源嵌入、构建脚本、工具调用和文档链接；修改独立工具还需检查它自己的 manifest。
6. 源码移动后执行全目标编译和受影响测试；纯文档变更检查路径与链接即可。运行机制写入架构或专题文档，本页只维护定位入口与目录规则。
