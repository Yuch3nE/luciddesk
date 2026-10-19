# Crates 导航

这些库按领域和平台依赖边界划分。应用窗口与交互代码位于 `app/`，各库保留独立的 Cargo 包名。

| Crate | 职责 | 入口 |
| --- | --- | --- |
| `luciddesk-api` | CLI 与主程序的协议、计划及本地管道传输 | [lib.rs](luciddesk-api/src/lib.rs) |
| `luciddesk-diagnostics` | 主程序和底层库共用的分级、限流诊断日志 | [lib.rs](luciddesk-diagnostics/src/lib.rs) |
| `luciddesk-plugin-api` | 插件 C ABI、JSON 协议和 Rust SDK，无宿主实现依赖 | [lib.rs](luciddesk-plugin-api/src/lib.rs) |
| `luciddesk-plugin-host` | DLL 发现、校验、生命周期、启停策略及故障诊断，不依赖 UI 或存储 | [lib.rs](luciddesk-plugin-host/src/lib.rs) |
| `luciddesk-core` | 无平台依赖的领域模型、身份、面板与工作区 | [lib.rs](luciddesk-core/src/lib.rs) |
| `luciddesk-storage` | SQLite 工作区与 TOML 配置持久化 | [lib.rs](luciddesk-storage/src/lib.rs) |
| `luciddesk-shell` | Shell 枚举、通知、文件操作和菜单 | [lib.rs](luciddesk-shell/src/lib.rs) |
| `luciddesk-explorer` | 过滤控制端、DLL 入口、成员恢复与独立 Shell 菜单 | [lib.rs](luciddesk-explorer/src/lib.rs) |
| `luciddesk-menu` | 应用与 Explorer 共用的原生菜单主题和边框 | [lib.rs](luciddesk-menu/src/lib.rs) |
| `luciddesk-graphics` | DWM/DComp 绑定和合成层 | [lib.rs](luciddesk-graphics/src/lib.rs) |
| `luciddesk-window` | 显示器枚举和启动错误提示 | [lib.rs](luciddesk-window/src/lib.rs) |

## 内部组织

```text
luciddesk-core/src/
  lib.rs, identity.rs, geometry.rs, appearance.rs
  item.rs, panel.rs, workspace.rs, tests.rs

luciddesk-storage/src/
  lib.rs, error.rs
  store/
    mod.rs                 # 连接、加载与事务保存
    codec.rs               # 外观／偏好的编解码与校验
    desktop_items.rs       # 桌面身份与成员位置读写
    monitor_layout.rs      # 按显示器拓扑保存与读取布局
    config.rs, schema.rs, recovery.rs
    tests.rs, config_tests.rs

luciddesk-shell/src/
  lib.rs, error.rs, tests.rs
  namespace.rs             # 枚举、名称、文件身份、PIDL
  desktop.rs               # Explorer 桌面查询
  notification.rs          # Shell 变更订阅
  activation.rs            # 打开项目、解码拖放身份
  apartment.rs             # OLE 守卫
  file_command.rs, rename.rs, native_layout.rs
  native_menu.rs, native_menu/

luciddesk-explorer/src/
  lib.rs, discovery.rs, notifications.rs
  filter.rs, filter/      # 客户端、IPC、成员过滤与恢复
  filter/menu/            # 独立 Shell 宿主与原生菜单

luciddesk-menu/src/
  lib.rs, theme.rs, frame.rs # 原生菜单外观，无 Shell 枚举或 IPC

luciddesk-graphics/src/
  lib.rs, layer.rs
  bindings/dcomp.rs, bindings/dwm.rs

luciddesk-window/src/
  lib.rs, monitors.rs
```

## 维护边界

- 公共导入路径保持 `luciddesk_storage::WorkspaceStore`、`luciddesk_shell::ShellApartment`、`luciddesk_graphics::Layer` 等形式；内部模块不作为新的公共 API。
- 存储实现放在 `store/`，配置和恢复作为其子模块共享私有状态，避免把连接字段公开到 crate 外部。
- Shell 内部的身份／PIDL 帮助函数通过明确的 `namespace` 路径复用，不塞回根入口。
- `luciddesk-core` 不依赖 Windows 或数据库；当前 `luciddesk-storage`、`luciddesk-shell`、`luciddesk-window` 使用其领域类型。
- 修改生成绑定时同步相应工具、筛选清单和生成结果。
- 原生菜单外观通过 `luciddesk-menu` 显式依赖共享，不跨 crate 用 `#[path]` 引入实现。
- `luciddesk-explorer` 输出 `luciddesk_explorer.dll`；Rust 导入名为 `luciddesk_explorer`，同时包含应用侧客户端与 Explorer 侧实现。
- 小库无需为了目录对称而继续拆分。`luciddesk-window` 的显示器模块和错误入口已足够清楚。

构建与验证命令见[开发指南](../docs/development/build.md)，整体目录规则见[目录结构](../docs/development/structure.md)。
