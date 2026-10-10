# 贡献指南

简体中文 · [English](CONTRIBUTING.en.md)

欢迎为 LucidDesk 反馈问题、改进功能、完善翻译或提交代码。开始前先查看已有 [Issues](https://github.com/Yuch3nE/luciddesk/issues) 和 [Pull Requests](https://github.com/Yuch3nE/luciddesk/pulls)，避免重复工作。使用问题可先查阅[使用说明](docs/usage.md)，代码入口见[开发文档导航](docs/development/README.md)。

## 反馈问题与建议

问题报告应让其他人能够复现，建议包含：

- **环境**：Windows 构建号、LucidDesk 版本、发行方式（安装版、普通 ZIP、便携 ZIP 或 MSIX）。
- **操作**：最小复现步骤、预期行为和实际结果，注明是否每次出现。
- **证据**：完整错误信息和必要截图；界面问题补充显示器布局、DPI、主题与材质。
- **相关条件**：涉及搜索、预览或 Shell 菜单时，注明相关外部程序和扩展；升级问题注明新旧版本及安装方式。

可使用“设置 → 关于 → 复制诊断”获取环境信息，分享前检查内容。数据或崩溃问题优先提供可复现的测试数据及必要日志，不直接上传完整工作区数据库、含个人路径的配置、未脱敏转储、敏感截图或密钥。数据范围见[隐私政策](PRIVACY.md)。

功能建议请描述实际场景、当前操作中的困难和期望行为。较大的功能、架构或兼容性调整，建议先通过 Issue 讨论范围；小型修复和文档改进可直接提交 PR。

## 准备开发环境

使用 Windows x64、PowerShell 7、rustup、Visual Studio C++ 构建工具和 Windows SDK。Rust 版本由 `rust-toolchain.toml` 指定，Windows 构建环境由仓库脚本选择；完整要求见[构建指南](docs/development/build.md)。

从仓库根目录执行：

```powershell
git clone https://github.com/Yuch3nE/luciddesk.git
cd luciddesk
.\tools\use-windows-toolchain.ps1
cargo build -p luciddesk -p luciddesk-explorer --locked
```

构建成功后，退出已运行的 LucidDesk，再用独立数据目录启动开发版本：

```powershell
$previousDataDirectory = $env:LUCIDDESK_DATA_DIR
try {
    $env:LUCIDDESK_DATA_DIR = Join-Path $PWD 'target\dev-data'
    Start-Process -FilePath '.\target\debug\luciddesk.exe' -WindowStyle Hidden -Wait
} finally {
    $env:LUCIDDESK_DATA_DIR = $previousDataDirectory
}
```

上面的命令等待应用退出后恢复当前终端原有变量。独立数据目录隔离配置，但不隔离真实 Explorer 或文件操作；重复启动仍可能唤起已有实例，不能把目录隔离当作多实例隔离。

EXE 与 `luciddesk_explorer.dll` 必须来自同次构建并放在同一目录。原生桌面测试会操作真实 Explorer，应在可中断的环境中执行；测试前确认文件、布局及运行实例，完成后恢复需要保留的状态。

## 选择代码与文档入口

| 改动 | 先阅读 |
| --- | --- |
| 模块或进程协作 | [架构说明](docs/development/architecture.md)、[目录结构](docs/development/structure.md) |
| 原生句柄、COM 与退出 | [Rust API 与资源生命周期约定](docs/development/rust-api-review.md) |
| 配置、数据库或恢复 | [配置与工作区存储](docs/development/storage.md) |
| 翻译、语言或字体 | [本地化指南](docs/development/localization.md) |
| UI、材质或绘制 | [设置组件](docs/development/settings-components.md)、[绘图与绑定](docs/development/rendering.md) |

沿用附近代码的风格和所属模块的校验规则。生成绑定通过工具更新，不手工修改生成结果；诊断构建和发布构建使用各自的产物路径，避免混用 EXE、DLL 和符号。

## 验证修改

根据改动选择检查，不必为纯文档变更编译整个应用。以下是常用入口，并非每个 PR 都必须逐条执行：

```powershell
cargo check --workspace --all-targets --locked
cargo test -p luciddesk-core -p luciddesk-storage --lib --locked
python tools/validation/check-locales.py
cargo test -p luciddesk --bin luciddesk i18n::tests --locked
```

| 范围 | 验证重点 |
| --- | --- |
| 文档 | 与实现一致、命令和路径正确、本地链接与锚点有效 |
| 模型与存储 | 有效及无效输入、状态保持、保存失败和兼容边界 |
| 翻译与设置 | 资源键与参数、语言切换、长文本、字体回退和布局 |
| UI 与 Explorer | 实际输入、取消、焦点、第三方扩展及最终文件操作 |
| 生命周期 | 正常退出、重连、异常终止后的恢复和 DLL 文件释放 |
| 发布与安装 | 包来源、标记、签名、升级、卸载和数据保留 |

原生窗口测试按专题说明串行或单独运行，默认忽略的交互测试需显式选择。确认测试过滤器实际命中用例；零项测试成功退出不能算行为验证。CI 不覆盖所有实机操作，当前 Build CI 也不由普通 PR 自动触发，不能把没有失败状态当作已通过检查。

完整方法见[验证与兼容边界](docs/development/validation.md)。报告实际执行的命令、结果和未覆盖项，将环境阻塞、测试失败与未执行分别说明。离屏截图不能替代真实首帧与输入检查，强制结束主进程也不能替代真实崩溃路径验证。

## 提交 Pull Request

保持一个 PR 聚焦一个问题，描述最终变更：

1. 说明用户遇到的问题或具体触发条件。
2. 描述修改后的行为和必要的设计取舍。
3. 列出已执行的验证、结果及剩余限制；界面变更附必要截图。
4. 关联相关 Issue，注明数据兼容或迁移影响。

不要混入无关格式调整、临时日志、测试产物、个人数据、证书或私钥。确保有权提交新增代码和素材，遵循项目 [MIT 许可证](LICENSE)及相关第三方许可。

新增文案按本地化指南维护资源键与占位参数；新增语言同时检查注册和测试覆盖。可见行为变化同步更新使用说明，开发约束变化更新所属专题，避免将临时排查过程写成长期实现说明。

Changelog 只记录与应用相关的变化，遵循中英文现有结构；尚未发布的内容放在未发布章节。纯文档整理不必添加应用更新条目。版本编号与发布规则见[构建指南](docs/development/build.md)，不要随每个提交独立递增版本。

提交信息建议使用 `feat`、`fix`、`docs`、`test`、`build` 或 `ci` 等 Conventional Commits 类型。收到审核意见后补充复现、解释或修改，并同步更新 PR 描述和验证结果；讨论聚焦问题与证据，尊重不同观点。
