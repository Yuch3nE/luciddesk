# 可选系统监控插件（PDH 子进程方案）

0.21.2 默认安装包和 ZIP 不含监控 DLL。源码支持可选构建；监控插件位于 EXE 同级 `plugins` 目录，系统监控是插件提供的功能面板，启用时由独立子进程承载。

## 主进程、子进程与插件

- 主进程提供插件管理入口与配置目录，管理监控子进程的启动、配置同步与退出；插件负责保存自己的配置。
- 监控面板启动同目录的 `luciddesk.exe --monitor-child`，复用原生窗口、标题栏、拖动、缩放、层级、折叠、DPI 和材质代码。子进程不启动托盘、Explorer Hook、控制 API 或单实例入口。
- `plugins/system-monitor` DLL 提供设置页描述并处理设置动作、业务校验和配置保存；在子进程中负责监控绘图、字体、曲线、采样线程、设备查询及快照。主进程加载 DLL 不会自动启动采样或创建监控绘图对象。
- `crates/luciddesk-plugin-api` 定义版本化 C ABI 和设置页描述及动作协议，不跨 DLL 传递 Rust 对象。绘图借用子进程宿主的 `ID2D1DeviceContext`，宿主负责 BeginDraw/EndDraw，插件不持有该指针。
- `luciddesk-plugin-host` 枚举 EXE 旁 `plugins` 的顶层 DLL，校验 ABI、清单与能力，`app/src/plugins.rs` 桥接应用语言和用户数据目录配置；监控适配器要求 ID `system-monitor` 和能力 `system-monitor.v1`。详见[插件开发指南](plugins.md)。

软件设置导航在独立“插件”组中列出插件管理和系统监控设置页。主程序统一渲染控件，将设置动作路由回插件；插件保存成功后，宿主同步监控窗口启停。子进程回传设备清单，设置页不在主进程执行设备采样。字体和语言由宿主传入；插件调用在各进程的 UI 线程进行，回复缓冲区仅在同步回调期间有效。

缺少插件时不显示监控设置入口、不创建已保存的监控窗口，保留配置供以后恢复。添加或移除 DLL 后需重启程序；当前不支持热插拔。已接受的 DLL 在所属进程退出前保持加载。

插件整体启用状态统一保存在用户数据目录的 `plugins/config.toml`，由宿主管理；监控功能开关及窗口参数保存在 `plugins/system-monitor/config.toml`，由插件管理。系统监控页面与软件自带设置页共用窗口；禁用插件后其设置页隐藏，仍可在插件管理中重新启用。

## 构建与使用

在仓库根目录运行：

```powershell
# 默认发行包，不包含监控插件
./tools/package.ps1 -Installer
# 显式加入监控插件的开发预览包
./tools/package.ps1 -Plugins system-monitor
# 单独构建插件，部署到 target/production/release/plugins
./tools/build-plugins.ps1 -Release -TargetDirectory target/production
```

自定义部署位置使用 `-Destination <EXE目录>/plugins`。不需要额外分发 helper EXE；主程序与子进程使用同一个可执行文件。

设置 → 系统监控提供面板开关、各指标开关、指标顺序、标准/紧凑布局、1/2/5 秒刷新、暂停和设备选择。关闭面板保留配置；禁用指标、隐藏、折叠或暂停会停止相应采样，但只有关闭面板才退出子进程并回收其进程级状态。

布局、刷新间隔和网卡/磁盘/GPU 来源使用统一下拉框。指标顺序显示完整的编号列表，通过每项的上移、下移按钮调整，首尾方向禁用；排序动作读取插件当前配置，交换相邻指标并保存。设置页不提供任务管理器入口。紧凑模式采用纵向指标行，百分比与进度条对齐，网络和磁盘分列显示双向速率，默认窗口保持 320×280 DIP。

## 指标与采样

CPU/GPU 使用 `plugins/system-monitor/src/counters.rs` 的 PDH 查询，查询由采样线程持有并通过 `PdhCloseQuery` 正常关闭。使用英文计数器路径，避免依赖系统显示语言。

| 指标 | 数据来源与含义 |
| --- | --- |
| CPU | `\Processor(_Total)\% Idle Time` 的补值，表示忙碌时间百分比 |
| GPU 占用 | `\GPU Engine(*)\Utilization Percentage`；按适配器 LUID 匹配，同一引擎的进程贡献求和，再取最忙引擎 |
| GPU 专用显存 | `\GPU Adapter Memory(*)\Dedicated Usage`；DXGI 提供适配器名称、LUID 和专用显存容量 |
| 物理内存 | `sysinfo` 的 RAM 已用量与总量 |
| 网络 | 累计接收/发送字节差除以实际采样间隔 |
| 磁盘 | 卷累计读取/写入字节差除以实际采样间隔，容量与设备清单低频刷新 |

`sampling.rs` 只用 `sysinfo` 刷新 RAM、网络和磁盘，不加载进程清单，不调用其 CPU 采样。CPU/GPU 指标组合改变时重建 PDH 查询；两者都关闭时释放查询。查询创建或计数器添加失败保留既有重试间隔，缺失或无效数据不伪装成零占用。

监控子进程有自己的后台采样服务，耗时查询不在 UI 线程执行。窗口定时器更新订阅并读取快照；暂停时保留当前快照。恢复采样时重新建立速率基线，首份差分数据未就绪时显示“采集中”，历史空档不连线。

采样订阅由可见性、暂停、配置变更及客户端析构显式更新，不再依赖 UI 心跳续期；UI 消息短暂阻塞不终止后台采样。采样线程按下一次截止时间等待请求，避免固定轮询叠加刷新误差。历史帧记录恢复边界，修改刷新间隔不重新解释旧历史；短暂调度延迟按实际时间连接有效读数。显式暂停、隐藏、折叠后的恢复，以及超过 7 秒的采样空档会开始新线段并重置差分基线，单项读数缺失仍保留断档，不补零。

历史保留最近 60 秒，仅存在内存中；采样线程、当前快照与历史通过 `Arc<Frame>` 共享不可变帧。CPU/GPU 的 Y 轴固定为 0–100%；网络曲线显示双向吞吐合计；磁盘分别绘制读写曲线，使用两者峰值计算共享刻度。速率单位为 B/s、KiB/s、MiB/s、GiB/s。

自动网卡选择当前最活跃接口，磁盘优先系统盘，GPU 优先能读取计数器且专用显存最大的适配器。手动选定的设备消失时显示不可用，不自动换设备。核显专用显存容量可能为零，不用该值作除数。目前不采集温度、风扇或功耗。`sysinfo` Windows 磁盘后端不能传播所有 I/O 查询错误，因此未观察到有效累计计数之前不显示零速率；后续驱动查询失败仍可能表现为保持原计数。

GPU 使用物理设备 PnP 实例作为稳定身份，合并虚拟显示驱动为同一显卡暴露的多个 DXGI LUID；相同型号的不同物理显卡仍保持独立。身份通过 [D3DKMT_QUERY_PHYSICAL_ADAPTER_PNP_KEY](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/ns-d3dkmthk-_d3dkmt_query_physical_adapter_pnp_key) 查询，查询不可用时保留 LUID。采样时从同一物理设备的逻辑别名选取有效读数，设备清单同时向主进程同步别名映射，使已保存的有效旧选择继续指向原物理设备。PDH LUID 匹配忽略十六进制字母大小写，占用率可用而显存不可用时仍保留占用率。

## 通信、关闭与恢复

`app/src/pane/monitor/process.rs` 使用继承的匿名管道传输按行 JSON。这是同一 EXE 构建之间的私有协议，不是第三方插件 ABI。

主进程发送配置快照；子进程回传窗口就绪、面板变更、设备清单、设置请求与关闭状态。采样帧和曲线不跨进程传输。子进程使用临时渲染模型和内存设置，不访问正式配置文件。主进程中的插件负责读写自己的配置。

子进程的消息控制窗口始终通过公共可见性门禁保持隐藏，并使用 `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`，不会创建任务栏按钮或抢占焦点。可见的监控面板沿用普通面板的工具窗口标记。

- 设置开关关闭：主进程关闭输入管道，子进程收到 EOF 后退出消息循环，销毁窗口、释放绘图和采样客户端、等待采样线程，正常退出。
- 窗口关闭按钮：子进程回传关闭状态，主进程禁用该面板并保留配置。
- 主进程退出或异常终止：管道断开，子进程使用相同的退出路径。
- 已启动的子进程初始化失败、协议错误或退出：主进程禁用对应面板，错误回复或非零退出码写入诊断，不自动循环拉起。重新启用时启动新进程。

系统监控仅有一个功能窗口，启用期间对应一个监控子进程。图表借用子进程宿主的 Direct2D factory，不另建 GPU 设备；绘图目标及相关图形缓存随该子进程退出回收。

进程隔离使 PDH 名称表和图形驱动的进程级保留不再留在主进程。关闭流程不清空工作集、不执行全局性能注册表清理、不主动卸载 DLL，也不强制终止子进程。主进程仍可能保留线程创建、设置页和分配器的少量可复用容量，不保证内存逐字节回到首次打开前。

## 保存和兼容

系统监控是插件功能，不创建工作区面板记录。插件在数据目录的 `plugins/system-monitor/config.toml` 中保存 `[options]`（功能开关、指标、布局、设备、采样间隔）和 `[window]`（位置、大小、折叠、锁定、置顶、自动收起）。插件负责默认值、校验和原子保存；相同值不写入。宿主只传递目录与操作，关闭后配置保留。子进程的 Panel ID 仅用于内部渲染，不写入工作区。产品不包含开发版监控数据库结构的迁移或兼容代码。

监控功能不出现在 CLI 工作区面板列表中；通过“插件 → 系统监控”设置页中的开关控制，目前没有专用监控 CLI 命令。图标内容适配、排序、标签和拖放不适用。

## 验证

```powershell
# 插件、SDK、宿主设置与绘图回归，分别覆盖有/无插件
./tools/validation/test-plugins.ps1 -TargetDirectory target/review-validation -Offline
# 当前生产路径：实际 EXE 子进程、原生窗口、PDH 与关闭后的内存
./tools/validation/test-monitor-process.ps1 -Offline
# 快速生命周期及故障复测
./tools/validation/test-monitor-process.ps1 -Offline -Cycles 2 -FirstSampleMilliseconds 2500
# 35 秒实机时序诊断：模拟 UI 无消息，并切换 5/2/1 秒刷新
cargo test -p luciddesk-monitor --lib --locked --offline native_sampling_cadence -- --ignored --nocapture --test-threads=1
```

子进程测试 `isolated_monitor_closes_process_and_keeps_host_sampling_empty` 需要交互桌面，不进入普通无界面测试。默认先暖机普通宿主窗口与插件设置描述，再开关一个监控面板 10 次：首次采样 63 秒，其余每轮 2.2 秒。覆盖关闭按钮、设置禁用、紧凑布局尺寸回传、管道断开、无效输入及重新启用。

测试检查子进程正常退出、HWND 失效、主进程没有监控客户端/绘图实例/采样服务，同时记录 Private Bytes、Working Set、句柄与 GDI/USER 数量。输出位于 `target/review-validation/monitor-process-memory.log`。内存数值用于观察趋势，不采用固定阈值或强制清空工作集来制造通过结果。

脚本构建实际 EXE 与带 `test-diagnostics` 的 DLL，结束时恢复普通 DLL。诊断功能仅查询资源状态，不主动释放资源；测试使用独立内存工作区，不改已安装程序的布局。原有进程内窗口测试仍用于检查组件释放，不替代上述子进程验收。

需要单独研究 PDH 查询关闭后的保留时，可使用现有 `pdh-memory-probe.cpp`（Windows SDK Developer PowerShell）：

```powershell
./tools/use-windows-toolchain.ps1
cl.exe /nologo /EHsc /MT /O2 tools/validation/pdh-memory-probe.cpp /Fetarget/pdh-memory-probe.exe /Fotarget/pdh-memory-probe.obj /link pdh.lib psapi.lib
./target/pdh-memory-probe.exe cpu
./target/pdh-memory-probe.exe gpu
```

## PDH 子进程实现实测（2026-10-19）

使用 `test-monitor-process.ps1`，真实 EXE、原生窗口和 PDH 数据源；主进程测试宿主先暖机普通分组窗口与插件设置描述，然后第一次启动监控。单面板开关 10 轮，首次采样 63 秒，其余每轮 2.2 秒。

| 阶段 | 主进程私有内存 | 主进程工作集 | 句柄 | GDI / USER |
| --- | ---: | ---: | ---: | ---: |
| 首次打开前 | 69,240 KiB | 76,532 KiB | 709 | 13 / 19 |
| 第 10 轮打开 | 69,148 KiB | 77,568 KiB | 716 | 13 / 19 |
| 第 10 轮关闭 | 69,096 KiB | 77,544 KiB | 708 | 13 / 19 |

第 10 轮监控子进程私有内存为 79,020 KiB，关闭后进程正常退出（退出码 0），该进程资源由 Windows 回收。主进程监控采样客户端、绘图实例始终为 0，采样服务为 false。关闭后没有累计增长；主进程工作集仍比首次基线高约 0.99 MiB，包含已经触碰的代码页和缓存，不等于仍持有监控采样或图形对象。

两轮故障回归的关闭后私有内存比该次基线高 88 KiB；最终构建的两轮复测为 +204 KiB，第二轮均不再增长。已验证：窗口关闭、设置禁用、紧凑布局尺寸回传、主进程管道断开、无效管道输入后的面板禁用与重新启用。原插件测试 15 项、SDK 6 项、核心 11 项，以及有插件/无插件的宿主测试 8/6 项通过。历史 9 MiB 增量不再留在主进程，但不承诺所有机器上的工作集或分配器容量逐字节回归。

完整 10 轮记录：`target/process-memory-10-cycles.log`。故障验证记录：`target/process-fault-validation.log`、`target/process-final-validation.log`。本轮仅构建和测试工作区 EXE，没有替换当前安装程序。


系统监控通过设置开关显示或关闭，保留指标和窗口配置；不提供普通分组的新建入口。
