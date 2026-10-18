# 原生系统监控面板

系统监控是独立的面板内容类型，使用现有原生窗口、Canvas 绘制、材质、DPI、吸附、锁定、置顶和折叠机制。托盘右键菜单中的“新建系统监控面板”创建该面板，不依赖 Explorer 成员过滤连接。监控面板不接收 Shell 项目，也不参与面板标签合并。

## 第一版功能

- CPU 总占用、物理内存已用与总量。
- 网卡上传与下载、磁盘卷读写速率。
- GPU 最忙引擎占用、专用显存使用与容量。不包含温度、风扇和功耗。
- 标准纵向与紧凑双列布局；指标显隐、移到首位；1/2/5 秒刷新；暂停和恢复。
- 网卡、磁盘、显卡手动选择。自动网卡选择当前最活跃接口，磁盘优先系统盘，GPU 优先能读取计数器且专用显存最大的适配器。

## 数据和生命周期

`app/src/pane/monitor/sampling.rs` 使用 `sysinfo` 的 system/network/disk 功能，关闭其他默认功能，明确只刷新 CPU、RAM、网络与磁盘，不加载进程清单。磁盘容量和设备清单低频刷新，速率由累计字节差与实际经过时间计算。

所有监控窗口通过弱引用共享一个后台采样服务。窗口定时器只发送订阅与读取快照，耗时采样不在 UI 线程执行。采样服务合并可见且未暂停窗口的指标，以最短订阅间隔采集；全部隐藏、折叠或暂停时停止采样。最后一个窗口释放后，通道关闭，线程退出。恢复时重新建立 CPU/网络/磁盘基线，第一份速率显示“采集中”，历史空档不连线。

`gpu.rs` 通过 DXGI 获取适配器名称、LUID 和专用显存容量；使用英文 PDH 路径读取 GPU Engine 与 GPU Adapter Memory 通配实例。按 LUID 匹配，进程贡献先按引擎相加，再取最忙引擎，不能把不同引擎简单相加或平均。缺失与无效计数器显示“暂不可用”；定期重试失败数据源。核显的专用显存容量可能为零，UI 不用该值作除数。

历史保留最近 60 秒，只存在内存中。CPU/GPU 的 Y 轴固定为 0–100%；网络曲线显示双向吞吐合计；磁盘分别显示读取、写入曲线，用两者峰值计算共享速率刻度，不把两个方向相加。速率单位为 B/s、KiB/s、MiB/s、GiB/s。磁盘容量使用同一设备的 sysinfo 已有采样，不新增高频查询。手动选定的设备消失时显示不可用，不自动换到另一设备。`sysinfo` Windows 磁盘后端不会传播所有 I/O 查询错误，因此未观察到有效累计计数之前不展示零速率；后续驱动查询失败仍可能表现为保持原计数，这是当前库的限制。

## 保存和兼容

核心 `PanelSource::Monitor` 区分监控内容。SQLite 通过 `panel_monitor_settings` 的外键行标记来源，基础 `panels.kind` 保留已有约束；读取时恢复完整逻辑类型。已知升级仅添加标记表，不重建现有 panels 表或外键。布局、窗口选项沿用已有保存机制，指标顺序、布局模式、设备和采样偏好保存为 `metadata` 中的 `monitor:<panel_id>` JSON。删除面板同步清理偏好和外键标记。

CLI 工作区查询返回 `kind=monitor`，通用几何和窗口选项仍可用；图标内容适配、排序、标签与拖放不适用。第一版创建入口在托盘，尚未新增专用 CLI 创建或监控数据查询命令。

## 验证

```powershell
cargo test -p luciddesk-core -p luciddesk-storage --lib --locked --offline -- --test-threads=1
cargo test -p luciddesk --bin luciddesk --locked --offline pane::monitor:: -- --test-threads=1 --nocapture
python tools/validation/check-locales.py
```

监控测试覆盖真实采样、速率时间与计数重置、共享服务与隐藏暂停、GPU 引擎聚合、配置恢复、原生窗口生命周期，以及深浅主题和多 DPI 的离屏渲染。设置 `LUCIDDESK_MONITOR_PREVIEWS=1` 可将固定示例数据的四种布局 BGRA 输出到 `target/monitor-previews`，用于视觉检查；它们不是本机实际性能截图。
