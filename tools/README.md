# 开发脚本

命令默认从仓库根目录运行。常用入口保留在 `tools` 根目录，辅助脚本按职责归类。

| 位置 | 用途 |
| --- | --- |
| `package.ps1` | 构建生产版本，生成 ZIP、便携包和 EXE/MSI |
| `package-msix.ps1` | 从生产包生成 MSIX |
| `use-windows-toolchain.ps1`、`windows-toolchain.json` | 选择并记录 Windows 构建工具链 |
| `ensure-inno.ps1`、`ensure-wix.ps1` | 准备安装包编译器 |
| `packaging/` | EXE/MSI 构建、安装负载及 Skill 文件暂存 |
| `validation/` | CI 检查、脚本单元测试、安装与桌面交互验收 |
| `release/` | 生成双语发布说明、更新远端 Release 附件 |
| `assets/` | 应用图标生成、校验与图标缓存刷新 |
| `analysis/` | Hook 测量与 Explorer 符号分析 |
| `render-diagnostics/` | 随诊断包分发的渲染与崩溃采集工具 |
| `windows-bindings/` | Windows API 绑定生成工具 |

## 常用命令

```powershell
./tools/use-windows-toolchain.ps1
./tools/ensure-inno.ps1
./tools/package.ps1 -Installer -InstallerFormat Exe -Offline

python -m unittest discover -s tools/validation -p 'test_*.py'
python tools/validation/check-locales.py
python tools/validation/test-control-ci.py --offline
```

CLI Schema 校验依赖见 `validation/requirements-ci.txt`。安装、桌面交互和性能实验脚本应按需单独执行；它们不属于上述 Python 单元测试发现范围。`release/refresh-release.py` 会修改远端发布附件，执行前核对目标仓库与标签。

`validation/test-desktop-sync.ps1` 只执行文件夹监听重复回归，使用 `-LiveDesktop` 时额外运行两项只读桌面探针；每项必须实际执行一个测试。全量检查使用构建指南中的命令。

`validation/support/cli-idle.ps1` 和 `cli-desktop-items.ps1` 是 CLI 验收的内部步骤，依赖主脚本的进程、数据目录和函数。通过 `validation/test-cli-plan.ps1 -IdleSeconds N` 或 `-LiveDesktopItems` 调用，不独立执行。

`assets/Refresh-App-Icon.ps1` 会复制到发行包根目录，在包内仍以同目录的 `luciddesk.exe` 为默认目标。`render-diagnostics/` 内部文件保持相对路径，便于独立分发。

完整构建和验收说明见[构建指南](../docs/development/build.md)与[验证指南](../docs/development/validation.md)。
