<!-- release-doc-version: 0.25.2 -->

# 安装与更新

下载请到 [GitHub Releases](https://github.com/Yuch3nE/luciddesk/releases)，选择对应版本的附件。安装后从[使用手册](https://luciddesk.bbkingdom365.com/guide/)开始。

默认发布 EXE 安装包，另支持从源码构建 MSI。日常使用推荐 EXE，可在向导中选择当前用户或所有用户；MSI 适合使用 Windows Installer 部署的场景。

## 更新前先做什么

1. 在“设置 → 备份与恢复”导出一份配置备份。
2. 从托盘退出 LucidDesk，关闭设置窗口不等于退出。
3. 使用与原来相同的安装格式和范围更新；不要只替换单个 EXE 或 DLL。
4. 启动后检查版本、面板和文件夹路径。原始文件需要另行备份。

程序只检查更新并打开下载页面，不会自动安装新版。安装后可在“设置 → 关于”确认版本。

## EXE 安装包

运行 `LucidDesk-<版本>-windows-x64-setup.exe`，按向导选择安装范围和目录。所有用户安装需要管理员权限，默认安装到 Program Files。支持开始菜单快捷方式和可选桌面快捷方式；更新沿用原目录并保留配置。卸载默认保留配置，取消“保留用户配置”可删除当前账户的默认数据目录。

```powershell
# 静默安装到当前用户
& ".\LucidDesk-<版本>-windows-x64-setup.exe" /CURRENTUSER /VERYSILENT /SUPPRESSMSGBOXES

# 所有用户安装
& ".\LucidDesk-<版本>-windows-x64-setup.exe" /ALLUSERS
```

## MSI 安装包

`LucidDesk-<版本>-windows-x64.msi` 使用 Windows 自带的安装服务，无需安装其他运行时。双击后选择安装目录，安装完成后从开始菜单启动 LucidDesk。

默认安装到 `%LOCALAPPDATA%\Programs\LucidDesk`，仅当前用户可用。配置、布局和备份保存在 `%LOCALAPPDATA%\LucidDesk`。

### 更新与卸载

运行新版 MSI 即可更新，同一安装范围内沿用原目录并保留配置，不允许降级。更新前会请求 LucidDesk 正常退出；应用未退出或 Explorer 中的桌面组件仍被占用时，安装会停止。

从 Windows「设置 → 应用」卸载，移除程序、安装器创建的开始菜单快捷方式及当前账户指向本安装目录的开机启动项。用户自行复制的快捷方式保留。卸载不会删除配置；需要彻底清理时，自行删除 `%LOCALAPPDATA%\LucidDesk`。所有用户安装的其他账户数据及启动项需由对应账户处理。

**切换安装格式**：EXE 与 MSI 不能相互覆盖升级。切换前先卸载原安装版，并保留配置，再安装另一种格式。

便携版请使用便携 ZIP 更新并保留 `data`，不要将 MSI 安装到含有 `portable` 或 `portable.marker` 的目录。

### 静默安装

```powershell
# 当前用户
msiexec.exe /i "LucidDesk-<版本>-windows-x64.msi" /qn /norestart

# 所有用户，需管理员权限；默认安装到 Program Files
msiexec.exe /i "LucidDesk-<版本>-windows-x64.msi" ALLUSERS=1 MSIINSTALLPERUSER="" /qn /norestart

# 自定义目录；禁止安装器自动请求应用退出
msiexec.exe /i "LucidDesk-<版本>-windows-x64.msi" INSTALLFOLDER="D:\Apps\LucidDesk" CLOSEAPP=0
```

更新时使用与原安装相同的用户/所有用户范围。`/L*v "install.log"` 可指定日志；`/fa "安装包路径"` 用于修复，`/x "安装包路径"` 用于卸载。


## CLI 与 Agent Skill

主程序旁附带 `luciddesk-cli.exe` 和完整的 `skills/luciddesk-control` 文件夹。使用同一版本的主程序、CLI 和 Skill；更新时一起替换，不要混用。

不使用 Agent 或脚本时，无需配置 CLI。需要连接 Agent 时，在“设置 → 通用 → Agent 与 CLI”复制 Skill 安装提示词，交给你使用的 Agent；复制不会自动安装或授权整理。安装器不会修改 Agent 的目录或系统 PATH。

在程序目录打开 PowerShell，运行 `.\luciddesk-cli.exe status --json` 可检查连接，`help` 可查看命令。详细用法见随包 `cli.md` 或[在线 Agent 指南](https://luciddesk.bbkingdom365.com/agent/)。CLI 支持备份、导出与恢复；使用前通过 `capabilities --json` 检查正在运行的程序支持哪些操作。


## EXE 安装内容

EXE 安装包采用明确的文件清单，包含主程序、桌面 DLL、CLI、Agent Skill、CLI 指南、协议、许可证、构建信息和安装标记。品牌说明、便携版指南、更新日志及图标刷新脚本保留在源码或 ZIP 包中，不再安装到程序目录。升级旧安装时不主动删除用户目录内已有的同名文档。
