# WinGet 发布

首个包标识为 `Yuchen95.LucidDesk`，使用 GitHub Release 中的正式 x64 Inno Setup EXE，当前仅声明当前用户安装。清单中的 `/CURRENTUSER` 固定安装范围；静默参数采用 WinGet 对 `InstallerType: inno` 的内置支持。

0.20.1 清单提交：[microsoft/winget-pkgs #446565](https://github.com/microsoft/winget-pkgs/pull/446565)。提交记录不代表已经合并或可从公共源安装；以该 PR 的审核状态为准。

## 本地校验

```powershell
winget validate --manifest installer/winget/manifests/y/Yuchen95/LucidDesk/0.20.1
```

本地安装测试需要管理员启用 `LocalManifestFiles`。记录原状态，测试结束后恢复；安装会关闭正在运行的 LucidDesk。测试应优先在 Windows Sandbox 或测试机器上进行。

```powershell
winget settings --enable LocalManifestFiles
winget install --manifest installer/winget/manifests/y/Yuchen95/LucidDesk/0.20.1 --silent --force --scope user
winget list --name LucidDesk
winget settings --disable LocalManifestFiles
```

## 0.20.1 验证记录

测试环境：Windows 10.0.26300.9550 x64、WinGet 1.29.380。

- 正式 Release EXE 的 SHA256 与发布校验文件一致。
- `winget validate` 通过；`winget install --manifest ... --silent --force --scope user` 返回 0。
- WinGet 的已安装应用列表识别 `0.20.1`，卸载登记 `DisplayVersion` 为 `0.20.1`。
- 正式 EXE 在程序运行时静默覆盖安装返回 0；配置文件及数据库逻辑内容保持不变。
- 安装后的 GUI、CLI、桌面组件、CLI 文档、协议 schema 和全部 6 个 Skill 文件均与 Release 构建清单哈希一致。
- 独立 AppId、名称、数据目录及互斥锁的测试包验证了静默首次安装、仅显示进度的升级、正常关闭等待、降级拦截和静默卸载；登记与程序被清除，未跟踪的用户文件被保留。
- 测试完成后恢复禁用 `LocalManifestFiles`，重新启动正式程序，确认桌面组件连接正常。最终核对配置与面板数据未变；桌面清单仅更新了安装时重建的 LucidDesk 快捷方式的 `identity_key`、`file_id`。

范围：真实安装原本已经是 `0.20.1`，因此正式包测试为同版本覆盖安装。跨版本流程使用同一安装脚本生成的 `99.0.1 → 99.0.2` 隔离测试包；不等同于历史正式版本升级实测。未验证全用户安装或其他 Windows 版本。

## 后续版本

### VC++ 运行库修复

0.20.1 的正式 GUI 导入 `VCRUNTIME140.dll`，安装器在复制文件前将 GUI 提取到临时目录执行 `--check-desktop-component`；缺少运行库会在此阶段弹出系统错误。该版本清单已声明 `Microsoft.VCRedist.2015+.x64`，但使用 `--skip-dependencies` 的测试仍不能据此保证通过。

后续源码通过 Windows MSVC `crt-static` 消除三个发布二进制对可再分发 VC++ DLL 的直接依赖，打包前用 `tools/test-runtime-dependencies.ps1` 校验最终导入表。2026-10-09 本地验证：旧安装版被检查器以 `VCRUNTIME140.dll` 拦截，新 GUI、CLI 和 Explorer DLL 均通过；仅包含 GUI 的独立目录中，预检在约 0.19 秒返回 1（当前桌面组件仍在使用），CLI 启动及 DLL 在测试进程中加载成功。静态运行库构建的 CLI/Core/Storage 共 85 项测试通过，EXE 与 ZIP 构建成功。

以上是开发机依赖检查和启动回归，不是缺少运行库的干净 Windows 安装验收；本机无 Windows Sandbox。发布修复版本前仍需在干净 Windows 上完成首次静默安装、升级和卸载验证。不要卸载开发机的共享运行库来模拟该环境，也不要用本地同版本测试包覆盖已发布附件。

Release 安装包发布完成后，更新版本、固定下载地址和 SHA256，重新验证并向 `microsoft/winget-pkgs` 提交单个版本的清单。不要覆盖已被 WinGet 清单引用的同版本 EXE，否则现有哈希会失效。

```powershell
wingetcreate update Yuchen95.LucidDesk --version <版本> --urls <正式EXE地址>
```

检查生成结果并完成安装测试后再提交。主程序发布不会自动更新 WinGet 源；本仓库 CI 尚未接入清单自动提交。
