<div align="center">

<img src="docs/images/app-icon.png" width="128" height="128" alt="LucidDesk" />

# LucidDesk

简体中文 · [English](README.en.md)

**把 Windows 桌面整理成顺手的工作区，也能交给 Agent 协助打理。**

桌面面板 · 文件夹面板 · Everything 搜索 · 空格预览 · Agent 联动

[![License: MIT](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)
[![Build CI](https://github.com/Yuch3nE/luciddesk/actions/workflows/build.yml/badge.svg)](https://github.com/Yuch3nE/luciddesk/actions/workflows/build.yml)
[![版本](https://img.shields.io/badge/version-0.22.2-087EA4?style=flat-square)](CHANGELOG.md)
[![Windows](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?style=flat-square)](#系统与兼容性)
[![架构](https://img.shields.io/badge/arch-x64-475569?style=flat-square)](docs/portable.md)

[⬇️ 下载](https://github.com/Yuch3nE/luciddesk/releases) · [📸 截图](#界面截图) · [🚀 开始使用](#开始使用) · [📖 使用指南](docs/usage.md) · [📋 更新记录](CHANGELOG.md) · [🤝 参与开发](#参与开发)

LucidDesk 是使用 Rust 开发的 Windows 桌面整理工具。把零散图标收进面板，把常用目录放到桌面，再用 Everything 搜索快速找到文件。通过 CLI 与配套 Skill，还可以让 Agent 按你的要求整理图标、调整面板布局、管理文件夹和设置，先预览改动，再应用到桌面。

![LucidDesk 功能示意：面板标签、文件夹浏览、Everything 搜索与 AI Agent 联动](docs/images/overview.svg)

<sub>功能示意图 · 实际界面见下方截图</sub>

</div>

当前版本 **0.22.2** 改善桌面隐藏与恢复，改用静默更新提示，并让设置窗口保留任务栏入口。搜索全局快捷键已移除，桌面搜索和托盘入口继续保留。已有设置与桌面布局可直接沿用，完整变化见[更新记录](CHANGELOG.md)。

<a id="界面截图"></a>

## 📸 界面截图

| 桌面面板 | 主题与材质 |
| :---: | :---: |
| <a href="screenshot/zh/pane.png"><img src="screenshot/zh/pane.png" height="220" alt="中文桌面面板" /></a> | <a href="screenshot/zh/setting.png"><img src="screenshot/zh/setting.png" height="220" alt="中文主题与材质设置" /></a> |
| 拖入图标，按需折叠与整理 | 调整深浅主题与背景材质 |
| **文件夹面板** | **关于与运行状态** |
| <a href="screenshot/zh/folder.png"><img src="screenshot/zh/folder.png" height="220" alt="中文文件夹面板列表视图" /></a> | <a href="screenshot/zh/about.png"><img src="screenshot/zh/about.png" height="220" alt="中文关于页面与运行状态" /></a> |
| 浏览常用目录，查看类型、修改时间与大小 | 查看版本、项目入口与运行状态 |

点击截图查看原图。图中为深色亚克力，实际效果随壁纸和系统设置变化；Mica 系列仅在 Windows 11 提供。

<a id="功能"></a>

## ✨ 功能

| 功能 | 你可以做什么 |
| --- | --- |
| 桌面面板 | 将文件、文件夹和快捷方式拖入面板；支持紧凑排列、保留空位的手动网格和自由坐标，按名称、类型、修改时间或大小排序 |
| 双击隐藏桌面 | 双击桌面空白处隐藏所有图标和面板，再次双击恢复；在通用设置中启用 |
| 面板标签 | 把多个桌面面板放进一个窗口，按需切换、排序、合并或分离 |
| 文件夹面板 | 在桌面浏览常用目录，按名称、类型、时间或大小排序，文件变化后自动刷新 |
| 文件搜索 | 通过 Everything 查找本机文件，直接打开或定位所在文件夹 |
| 文件预览 | 选中文件后按空格，使用 PowerToys Peek 或 QuickLook 查看内容 |
| 字体与语言 | 搜索适合当前语言的字体，切换七种界面语言，无需重启 |
| Agent 联动与 CLI | 让 Agent 通过配套 Skill 和 CLI 查询桌面、规划整理方案并应用；支持面板、图标、布局、文件夹、搜索和设置管理 |
| 备份与恢复 | 保存设置和布局，按需恢复；自动备份跳过未变化的内容 |

面板可以移动、缩放、折叠、自动收起，也可以吸附边缘、锁定或置顶。支持调整字体、圆角与颜色，选择纯色、亚克力或云母材质。文件夹与搜索面板独立显示，不参与面板标签合并。

普通面板右键菜单可切换“自动排列图标”和“对齐到网格”：关闭自动排列后允许保留空位，关闭网格对齐后可以自由放置图标；增删内容时保留其他图标的位置。排序只执行一次，再次选择同一排序字段切换升降序。“设置 → 面板布局 → 图标排列”可调整默认选项（仅影响新建面板）。Agent 也能通过 CLI 与配套 Skill 调整排列模式、定位图标并预览改动。

未收纳的图标继续由 Windows 桌面管理。遇到连接问题，可在“设置 → 关于”查看状态、重新连接或复制诊断信息。近期改进见[更新记录](CHANGELOG.md)。

默认安装包和 ZIP 不包含插件，开发中的插件列表暂不显示设置入口。可选插件的构建和配置归属见[插件指南](docs/development/plugins.md)。

软件更新时，同一路径的已收纳快捷方式在消失后 5 分钟内重建，可恢复原分组和位置。仅在当前桌面会话内保留，不写数据库、不跨重启恢复。升级时可保留现有设置与布局；安装版沿用原安装格式与范围，便携版保留 `data`。升级前建议备份。

**面板不会搬动你的文件。** 将图标拖入面板只改变桌面上的收纳方式；拖回桌面即可移出，关闭面板也不会删除原文件。文件菜单中的删除、重命名和剪切，以及文件夹面板中的拖入、粘贴，会操作真实文件。

<a id="开始使用"></a>

## 🚀 开始使用

### 选择下载版本

在 [GitHub Releases](https://github.com/Yuch3nE/luciddesk/releases) 下载。日常使用推荐安装版；三种发布包功能相同。

| 版本 | 如何识别发布包 | 默认配置位置 | 适合场景 |
| --- | --- | --- | --- |
| 安装版 | `windows-x64-setup.exe` | `%LOCALAPPDATA%\LucidDesk` | 安装向导、开始菜单与卸载入口 |
| 普通 ZIP（非便携版） | `.zip`，不含 `portable` | `%LOCALAPPDATA%\LucidDesk` | 免安装，程序与配置分开保存 |
| 便携版 | 名称含 `windows-x64-portable` | 程序目录中的 `data` 文件夹 | 希望配置随程序目录一起携带 |

详见[安装说明](docs/installer.md)、[普通 ZIP 说明](docs/package.md)和[便携版说明](docs/portable.md)。若尚无发布包，可按[源码构建](#源码构建)自行打包。

### 启动与整理

**安装版**：优先选择 `windows-x64-setup.exe`，可在向导中选择当前用户或所有用户安装。MSI 可从源码自行构建，详见[安装版说明](docs/installer.md)。

**ZIP 版**：

1. 退出正在运行的旧版，将压缩包完整解压到有写入权限的文件夹。
2. 双击 `luciddesk.exe` 启动。请保留包内其他文件，不要只复制 EXE，也不要直接在压缩包中运行。
   便携版还需保留 `portable`，以启用程序目录内的配置存储。
3. 将桌面图标拖入面板；右键托盘图标，选择“新建面板”或“新建文件夹面板…”添加更多内容。

点击托盘图标可显示面板；右键菜单提供新建、搜索、刷新、打开配置目录和退出入口。普通面板在切换应用后仍可被遮挡，“始终置顶”的面板保持置顶。

需要临时清空桌面时，可在“设置 → 通用”开启“**双击隐藏桌面**”（默认关闭）。双击桌面空白处隐藏所有图标和面板，再次双击或点击托盘图标恢复。面板淡入淡出，并协调桌面图标的恢复时机；系统关闭动画时直接切换。原先隐藏的图标和面板保持原状，布局不会改变。

需要键盘操作时，可在“设置 → 面板布局”启用“显示所有面板”快捷键，默认关闭，预设为 `Ctrl + Shift + D`。

### 开启搜索与预览

搜索和文件预览**默认关闭**，需要单独安装对应软件，并在设置中启用。

| 想使用的功能 | 准备与设置 |
| --- | --- |
| 文件搜索 | 安装并运行 Everything，在“设置 → Everything 搜索”中启用搜索面板 |
| 空格预览 | 安装 PowerToys Peek 或 QuickLook，在“设置 → 文件预览”中选择程序并启用 |

这些软件不包含在 LucidDesk 的任何发布包中。更多操作和快捷键见[使用说明](docs/usage.md)。

### CLI 与 Agent

`luciddesk-cli.exe` 为脚本和 Agent 提供结构化的桌面管理接口。启动同一版本主程序后，即可查询当前工作区、规划修改并让主程序应用到界面。

| 能力 | 可以完成的操作 |
| --- | --- |
| 状态与清单 | 查询连接状态、可用能力、显示器、面板、图标和工作区快照 |
| 面板管理 | 创建、修改和移除面板，调整标题、锁定、折叠、自动收起和置顶等选项 |
| 布局整理 | 调整位置与大小，按图标列数适配内容，分列排列，指定目标面板、吸附侧和对齐方式 |
| 图标与标签 | 将桌面图标收纳到面板或移回桌面，按名称或指定顺序排列；合并、切换、排序和分离标签 |
| 文件夹与搜索 | 管理文件夹映射、视图和排序，浏览与刷新目录；提交搜索、刷新结果和加载下一页 |
| 设置与系统 | 查询和修改支持的应用设置，查找可用字体，查询与设置开机启动 |

在“设置 → 通用 → Agent 与 CLI”中可开关 CLI 控制，默认启用。复制 Skill 安装提示词后交给 Agent，可安装与当前程序配套的操作指南；复制本身不会执行安装。关闭 CLI 控制后，在线查询与修改均被拒绝，离线帮助、协议和 Skill 导出仍可使用。

在程序目录中从只读查询开始：

```powershell
.\luciddesk-cli.exe status --json
.\luciddesk-cli.exe capabilities --json
.\luciddesk-cli.exe workspace get --json
.\luciddesk-cli.exe skill show
```

**Agent 工作流程：查询 → 生成计划 → 预览差异 → 应用 → 核对结果。** CLI 提供 JSON 输出、协议查询和请求回执；预览不保存修改，状态冲突时重新查询，超时后通过回执确认执行结果。快捷修改命令加 `--dry-run` 只预览，省略时会预览后立即应用。

Skill 提供图标分组整理、按行适配与吸附、文件夹浏览、搜索及设置等常见流程，Agent 按任务读取对应参考文档。整理时先分组和排序，再按每行图标数适配面板，最后按指定方向吸附并验证；布局位置由你的要求决定。CLI 与手动操作共用尺寸和边缘吸附计算，吸附保持固定 5 物理像素间距。

CLI 以 Agent 调用为主：`help RESOURCE COMMAND --json` 提供参数与行为说明，`next_step` 提供后续动作和参数数组；待生效或不确定提交应查询原回执，避免重复修改。主程序和 CLI 使用相同发布版本，控制协议单独版本化。

Skill 使用渐进式披露：[SKILL.md](skills/luciddesk-control/SKILL.md) 保留通用流程，布局、文件夹、搜索、设置及恢复规则按任务读取。安装时将设置中的安装提示词交给 Agent，直接复制安装包内的完整 `skills/luciddesk-control/` 目录；CLI 导出保留用于兼容和发现，纯文本 `skill show` 只显示入口。不同面板类型支持的操作有所区别，完整说明见 [CLI 使用说明](docs/cli.md)，近期变化见[更新记录](CHANGELOG.md)。

<a id="升级与备份"></a>

## 🔄 升级与备份

“设置 → 关于”提供默认开启的“自动检查更新”开关：启动时及之后每 24 小时检查稳定版本，检测结果静默显示在关于页，不弹出更新对话框。有新版本时显示最新版本号和“下载新版本”按钮，也可随时手动“检查更新”；下载和安装由你决定。

| 版本 | 升级方式 |
| --- | --- |
| 安装版 | 运行新安装包，沿用安装目录；程序运行时可确认自动退出后升级 |
| 普通 ZIP | 退出程序，完整替换程序文件；配置目录保留 |
| 便携版 | 退出程序，替换程序文件，保留 `data` 和 `portable` |

EXE 与 DLL 必须来自同次构建。安装版卸载时默认勾选“保留用户配置”；取消勾选才会删除当前账户的默认设置、布局和备份。详见[安装说明](docs/installer.md)。

在“设置 → 备份与恢复”中可以打开配置目录、导出配置或恢复备份。备份只包含设置与布局，**不包含面板引用的原文件**；复制便携目录时，原文件也不会自动跟随。

普通版的设置默认位于 `%LOCALAPPDATA%\LucidDesk`。从普通版改用便携版时，可通过备份与恢复转入配置。自定义数据位置和详细升级步骤见[便携版说明](docs/portable.md)与[使用说明](docs/usage.md)。

<a id="界面语言"></a>

## 🌐 界面语言

支持简体中文、繁體中文、English、日本語、한국어、Deutsch 和 Русский。默认跟随 Windows 显示语言，其他语言回退到英语。在“设置 → 语言”中手动选择，立即生效。

切换语言时原位更新界面，保留面板、项目选择和滚动位置；字体搜索词也会保留，候选字体按新语言在后台重新筛选。

语言资源内置于程序，无需下载语言包。用户命名的面板、文件名和 Windows 原生文件菜单保持原样。

<a id="系统与兼容性"></a>

## 🖥️ 系统与兼容性

| 环境 | 支持情况 |
| --- | --- |
| Windows 11 x64 | 优先维护与验证平台，提供纯色、亚克力、Mica 和 Mica Alt |
| Windows 10 x64 | 已由用户完成实机验证，提供纯色和亚克力；图标字体与圆角包含兼容处理 |
| ARM64、远程桌面 | 尚未完整验证 |

系统停用背景效果时，材质会回退为随深浅主题变化的底色，保留原来的材质设置。系统重新允许效果后恢复。具体验证范围见[验证与兼容边界](docs/development/validation.md)。

<a id="常见问题"></a>

## 💬 常见问题

**桌面面板无法连接怎么办？** 在“设置 → 关于”查看连接状态并尝试重新连接。程序会保留面板配置并自动重试；文件夹与搜索面板仍可独立使用。

**重启 Explorer 后出现断线提示？** Explorer 退出时桌面连接会中断，恢复后程序会自动尝试重连。弹窗记录的是断线时的状态，不会随恢复更新；关闭提示后，可在“设置 → 关于”或通过 `luciddesk-cli status --json` 查看当前状态。CLI 的 `desktop_connected=true` 表示已连接，`desktop_sync_status=applied` 表示分组同步完成。若持续失败，请保留诊断日志中的错误信息。

**图标和面板突然隐藏了？** 如果开启了“双击隐藏桌面”，再次双击桌面空白处或点击托盘图标即可恢复，也可在“设置 → 通用”关闭此功能。

**如何反馈问题？** 请提供复现步骤、预期与实际表现，以及“设置 → 关于 → 复制诊断信息”中的信息。诊断信息包含近期错误和日志，时间与电脑本地时间一致。分享前请检查其中的个人路径等信息。显示异常时，附上截图、屏幕缩放比例和所选材质，便于定位。

<a id="源码构建"></a>

## 🛠️ 源码构建

准备 Windows x64、rustup、Visual Studio C++ 构建工具和 Windows SDK。仓库通过 `rust-toolchain.toml` 指定 Rust 版本，环境脚本自动选择满足最低版本要求的已安装 x64 MSVC 和 SDK；详见[构建指南](docs/development/build.md)。

主程序、CLI 和内部组件的发布版本统一来自根目录 `Cargo.toml`；插件版本和协议兼容版本独立维护。

先获取公开仓库：

```powershell
git clone https://github.com/Yuch3nE/luciddesk.git
cd luciddesk
```

在仓库根目录执行：

```powershell
.\tools\use-windows-toolchain.ps1
cargo build -p luciddesk -p luciddesk-cli -p luciddesk-explorer --locked
.\target\debug\luciddesk.exe
```

主程序和 `luciddesk_explorer.dll` 必须来自同次构建并放在同一目录。默认生成 EXE。生成安装包与 ZIP：

```powershell
.\tools\ensure-inno.ps1
.\tools\package.ps1 -Installer
.\tools\package.ps1 -Portable
```

MSI 使用 `ensure-wix.ps1` 准备工具后，加 `-InstallerFormat Msi`；`Both` 同时生成两种安装包。MSI 构建需要 .NET SDK 8 或更新版本。

安装包位于 `target/installers/`，普通 ZIP 位于 `target/packages/`，便携 ZIP 位于 `target/portable/时间戳/`。各发布包附带 SHA256 校验文件。测试命令、诊断构建和绑定生成见[构建指南](docs/development/build.md)。

<a id="参与开发"></a>

## 🤝 参与开发

欢迎通过 [Issue](https://github.com/Yuch3nE/luciddesk/issues) 和 [Pull Request](https://github.com/Yuch3nE/luciddesk/pulls) 提交问题、改进建议或代码。

开始前请阅读[贡献指南](CONTRIBUTING.md)；数据保存、联网检查及诊断分享见[隐私政策](PRIVACY.md)。

- **反馈问题**：附上复现步骤、系统版本和“设置 → 关于 → 复制诊断信息”的信息；界面问题请附截图与缩放比例。
- **提出功能**：说明使用场景和期望的操作方式，便于讨论是否适合桌面工作流。
- **提交代码**：先阅读[架构说明](docs/development/architecture.md)，保持改动聚焦，并提供相应验证结果。涉及可见行为时同步更新文档。

提交日志前请检查其中的个人文件路径等信息。项目作者：**Yuchen95**。

<a id="文档"></a>

## 📚 文档

| 文档 | 内容 |
| --- | --- |
| [使用说明](docs/usage.md) | 面板操作、快捷键、外观与故障处理 |
| [安装版](docs/installer.md) · [普通 ZIP](docs/package.md) · [便携版](docs/portable.md) | 安装、升级与配置保存 |
| [更新记录](CHANGELOG.md) | 各版本的应用功能与修复 |
| [CLI 使用说明](docs/cli.md) · [Agent 技能](skills/luciddesk-control/SKILL.md) | 命令、整理计划、预览与恢复 |
| [开发文档](docs/development/README.md) | 架构、构建、存储与验证 |
| [贡献指南](CONTRIBUTING.md) · [隐私政策](PRIVACY.md) | 参与方式与数据处理说明 |

<a id="友情链接"></a>

## 🔗 友情链接

[**Linux DO**](https://linux.do/)

<a id="许可证"></a>

## 📄 许可证

LucidDesk 使用 [MIT 许可证](LICENSE)。第三方依赖仍遵循各自的许可证。
