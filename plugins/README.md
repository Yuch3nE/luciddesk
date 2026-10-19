# 插件源码

此目录放可选插件实现；运行时加载的是 **EXE 同级 `plugins` 中的 DLL**，不是这里的源码。

- `system-monitor/`：系统监控插件，包括设置内容、原生绘图、采样和语言资源。
- 通用 C ABI / Rust SDK：[`crates/luciddesk-plugin-api`](../crates/luciddesk-plugin-api)。
- 独立宿主运行时：[`crates/luciddesk-plugin-host`](../crates/luciddesk-plugin-host)，管理发现、校验、启停和故障；应用桥接不再实现 DLL 加载。
- 主程序提供插件管理页、面板宿主、配置目录和插件发现；各插件提供设置页描述、处理设置动作并管理业务配置；主程序在单独的“插件”分组中统一渲染这些页面。启用状态统一保存在用户数据目录的 `plugins/config.toml`。

## 构建与验证

```powershell
# 编译 DLL，放入 target/debug/plugins，供同目录下的调试主程序加载。
./tools/build-plugins.ps1
# 自定义运行目录；运行中的 DLL 需先关闭宿主再覆盖。
./tools/build-plugins.ps1 -Release -TargetDirectory target/production -Destination target/plugin-preview/plugins
# SDK、插件单测、真实 DLL 加载/绘图，以及缺失插件回归。
./tools/validation/test-plugins.ps1
# 默认包没有监控；显式选择才打包。
./tools/package.ps1 -Plugins system-monitor -Installer
```

新增插件时在此目录创建 Cargo `cdylib` 包，添加 workspace member，并在其 `Cargo.toml` 中声明：

```toml
[package.metadata.luciddesk]
plugin = "your-plugin-id"
```

构建脚本通过该 ID 选择包，从 Cargo target 名称确定 DLL 文件名。元数据 ID 应与运行时 manifest 一致。`-Plugins` 接受多个 ID，默认发行不会自动包含新插件。ZIP、EXE/MSI 安装包和依赖/hash 检查使用同一份所选 DLL。示例 DLL 仅用于测试。

完整接口、生命周期和能力协议见[插件开发指南](../docs/development/plugins.md)。
