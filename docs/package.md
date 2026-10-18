# LucidDesk 免安装版

LucidDesk 提供桌面分组、文件夹面板和 Everything 搜索入口。开发者：**Yuchen95**。主要验证与维护平台为 Windows 11 x64。

本包是普通免安装 ZIP：解压即可运行，配置默认保存在用户目录。若希望设置随程序目录携带，请使用[便携版](portable.md)；需要安装向导、快捷方式和卸载入口时，使用[安装版](installer.md)。

## 启动与退出

1. 将 ZIP 完整解压到独立文件夹，不要直接从压缩包内运行。
2. 保持 `luciddesk.exe` 与 `luciddesk_explorer.dll` 同目录，且来自同一构建。
3. 退出已运行的 LucidDesk，再双击 `luciddesk.exe`。
4. 使用通知区域的托盘菜单退出；图标可能位于系统折叠菜单中。

重复启动会唤起已有实例，不会让两个版本同时接管桌面。因此验证新版本前应先退出旧实例，不能只根据双击的文件路径判断当前运行版本。

普通 ZIP 不运行安装器，也不会代替安装版注册卸载入口。请保留整套文件，避免只复制 EXE 或混用其他版本的 DLL。

## 包内文件

| 文件 | 用途 |
| --- | --- |
| `luciddesk.exe` | 主程序 |
| `luciddesk_explorer.dll` | 配套 Explorer 桌面组件 |
| `build.json` | 版本、Git 修订、构建环境及二进制校验值 |
| `README.md` | 本说明 |
| `usage.md` | 面板、文件操作、快捷键及设置说明 |
| `installer.md`、`portable.md`、`brand.md` | 安装、便携和名称与目录约定 |
| `CHANGELOG.md`、`CHANGELOG.en.md` | 中英文更新记录 |
| `PRIVACY.md`、`PRIVACY.en.md`、`LICENSE` | 隐私说明与许可 |
| `Refresh-App-Icon.ps1` | 应用图标刷新工具 |

普通包不含 `portable` 标记，也不附带个人配置。Everything、PowerToys Peek 和 QuickLook 需另行安装；不要把它们视为包内依赖程序。

## 数据保存与备份

应用使用 `config.toml` 和 `workspace.db`，默认位于 `%LOCALAPPDATA%\LucidDesk`，备份位于数据目录的 `backups` 子目录。免安装不表示数据随程序目录移动；复制或删除程序目录不会同时复制或删除默认用户配置。

数据路径遵循以下规则：

- `LUCIDDESK_DATA_DIR` 可指定独立数据目录，适合隔离测试。
- 未指定环境变量时，使用 `%LOCALAPPDATA%\LucidDesk`。

完整名称和目录约定见[品牌规范](brand.md)。不要随意添加 `portable` 或其他包标记来切换模式；使用对应发行包，避免误判配置位置。

“设置 → 备份与恢复”可导出、恢复配置和布局。备份保存文件引用，不包含桌面真实文件和文件夹内容，不能用来找回已经删除的文件。移动程序目录也不会改变面板所引用文件的实际位置。

数据库打开时会校验结构，仅对明确支持的结构执行限定升级；不兼容时报告错误，不应删除原文件来绕过提示。升级前可导出备份，遇到问题时保留数据与错误信息。

## 更新

1. 在“设置 → 关于”手动检查新版本，打开更新页面并查看说明。
2. 下载对应的普通 ZIP 及校验文件，核对后解压到新目录。
3. 从托盘退出旧实例，等待桌面组件释放，再启动新目录中的程序。
4. 确认版本与布局；默认用户数据会继续使用。如需覆盖原目录，必须先退出并保留配套 EXE/DLL。

应用不会通过更新检查自行下载并替换本包。也可改用安装包，已有 LocalAppData 配置仍按相同路径规则读取；见[安装与更新](installer.md)。

若 DLL 持续被占用，不要通过改名、混搭文件绕过检查。先确认 LucidDesk 已退出；仍无法释放时，手动重启 Windows 资源管理器或注销重新登录后再替换。程序启动时也会检查 Explorer 中残留的组件，避免新旧组件混用。

## 完整性与构建来源

ZIP 旁的 `.sha256` 记录压缩包哈希，可在 PowerShell 中计算并与文件内容比较：

```powershell
Get-FileHash -LiteralPath '<下载的 ZIP 完整路径>' -Algorithm SHA256
```

`build.json` 另行记录主程序和桌面 DLL 的 SHA256，以及版本、Git 修订、未提交修改状态、构建时间和环境。ZIP 校验与包内二进制校验作用不同：前者核对下载文件，后者用于确认解压后的程序配对。哈希一致不等于来源可信，仍应从项目发布渠道获取文件。

## 兼容性与常见问题

| 场景 | 行为与处理 |
| --- | --- |
| Explorer 提供所需桌面视图接口 | 连接桌面分组；收纳图标只改变引用归属，不移动真实文件 |
| DLL 缺失、过滤接口不支持或 Explorer 暂不可用 | 保留桌面分组配置并尝试重连；文件夹与已配置的搜索功能可独立使用，DLL 缺失时先重新完整解压 |
| Explorer 重启 | 检测连接失效并尝试重连、重建面板；恢复行为需在对应系统版本验收 |
| 显示器组合改变 | 等待变化稳定后恢复对应布局，将不可见面板移入可用屏幕；混合 DPI 与热插拔需实机检查 |
| Everything 不可用 | 先安装并运行 Everything，再在设置中检测或选择程序路径并启用搜索 |
| Peek 或 QuickLook 不可用 | 不影响基本面板操作；在设置中配置所选预览工具或关闭预览 |

Windows 10 的用户反馈不等于每个构建均完成验收；ARM64 和远程桌面也不能由 Windows 11 x64 启动成功推断支持。系统更新和第三方 Shell 扩展可能影响菜单或桌面连接，报告问题时附上 Windows 构建号、应用版本、复现步骤与错误信息。

面板使用、快捷键、文件夹映射及更多故障处理见[使用指南](usage.md)。


## CLI 与 Agent Skill

包内包含同版本的 `luciddesk-cli.exe`、`cli.md`、`protocol.schema.json` 和 `skills/luciddesk-control/SKILL.md`。主程序运行后，在程序目录执行 `./luciddesk-cli.exe status --json` 验证连接。`./luciddesk-cli.exe skill show` 离线显示配套 Skill，Agent 可使用 `skill show --json` 获取结构化结果。

需要让 Agent 自动发现 Skill 时，将完整 `skills/luciddesk-control` 文件夹复制到该 Agent 配置的技能目录；若已有同名技能，先比较内容，保留本地定制。安装程序不会修改 Agent 的配置、技能目录或系统 PATH。程序移动或升级后，使用新目录中的 CLI 与 Skill。操作命令和恢复规则见 `cli.md`。

## 可选插件

0.20.5 默认包不包含系统监控。开发构建可通过 `tools/package.ps1 -SystemMonitorPlugin` 添加 `plugins/luciddesk_monitor.dll`；主程序重启时检测加载。插件不存在时不显示对应面板及设置页，已有配置保留。详细边界见 [插件开发说明](development/system-monitor.md)。
