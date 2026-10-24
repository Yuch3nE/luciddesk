# Explorer 过滤性能验证

计时默认关闭。需要在交互式 Windows 会话运行包含计时代码的 LucidDesk，且桌面过滤已连接。仅记录次数、累计微秒和最大微秒，不记录文件名或身份。

```powershell
pwsh -File tools/validation/measure-hook-performance.ps1
```

默认请求 120 次桌面重绘，报告写入 `target/hook-performance.json`。每次绘制消息有 2 秒超时。脚本结束或异常时移除计时属性。不要并行运行多个采样脚本，也不要在采样期间升级或退出应用，否则窗口属性可能被清理，结果不完整。

- `Paint`：绘制前过滤，包括必要时的完整写入事务。
- `Request`：显式请求处理；不包含客户端排队时间。
- `Background`：后台校正与更新释放后的校正。
- `Restore`：退出或故障恢复尝试。
- `Snapshot`：清单读取；`Folder`、`Read`、`Names`、`Validate` 为子阶段。
- `Present`：写入后的 Shell 身份核验。
- `Add`、`Remove`、`Position`：Shell 图标添加、移除与位置恢复调用。

计时为嵌套的包含时间，不能将各阶段相加。子阶段在 STA 线程本地累计，最外层完成后才发布窗口属性，避免每个图标都跨入窗口属性 API。零次表示本轮未采到该阶段，不表示该功能没有耗时。正常退出会清理计时属性，恢复阶段的历史值不会持久保存。

## 隐藏与恢复实机回归

先构建独立只读 Shell 探针：

```powershell
./tools/use-windows-toolchain.ps1
cargo build -p luciddesk-shell --example desktop_snapshot --locked --offline
./tools/validation/measure-hook-performance.ps1 -OutputPath target/hook-transactions.json -Workload {
    ./tools/validation/test-hook-membership.ps1
}
```

实机回归创建 20 个唯一命名的空文件和一个临时面板，循环 5 次隐藏、Shell 目录刷新和恢复。通过独立 Shell 快照确认可见性，并在清理后核对原有面板状态、原生图标身份与坐标。删除测试文件后同步发送 Shell 删除通知，避免 AddObject 加回的测试条目暂留在视图缓存。仅删除本轮成功创建的文件，不操作用户文件。

## 2026-10-24 本机对照

Windows build 26300，Release 构建。停止编译后，前后各 120 次重绘；大量图标场景另外创建 100 个空文本文件，结束后清理。

| 场景 | 优化前平均 | 优化后平均 | 优化前最大 | 优化后最大 |
| --- | ---: | ---: | ---: | ---: |
| 当前桌面，4 个可见原生图标 | 0.344 ms | 0.127 ms | 0.877 ms | 0.239 ms |
| 额外 100 个可见图标 | 2.688 ms | 0.511 ms | 4.444 ms | 0.813 ms |

这是本机不变视图的对照，不代表所有机器或批量写入时延。实测批量操作和 Shell 刷新重建仍会出现更长的绘制过滤时间；不能用稳定状态的数据代替这些情况。

优化仅用于未暂停的绘制检查：已有基线、隐藏目标非空且无待释放成员时，读取并二次验证 PIDL 顺序和名称，省去两轮位置读取。发现任何需要隐藏的项目时，重新执行完整快照与原事务。首次建立基线、释放、读取失败、恢复以及所有写入前的身份/位置验证保持完整；不缓存跨次读取的文件身份，不替换 ILIsEqual，不减少恢复重试。

最终构建的实机验收：63 项过滤回归测试通过；另外显式运行只读 Explorer 快照测试通过。20 个图标的 5 轮收纳、刷新与释放通过，原有面板及原生图标坐标保持不变。正常退出时独立探针观察到全部 82 个原生图标恢复，重启后过滤状态为 applied，recovery=none。

批量验收中，10 次显式请求平均 22.60 ms；16 次写后身份核验平均 0.215 ms。Shell `AddObject` 100 次平均 1.11 ms，`RemoveObject` 668 次平均 1.72 ms。刷新会使已隐藏成员重新出现，因此该场景的绘制过滤最高约 213 ms，主要时间消耗在重新移除图标的 Shell 调用。这次未改变写入事务和恢复语义，也未为优化小占比的身份核验而替换 `ILIsEqual`。
