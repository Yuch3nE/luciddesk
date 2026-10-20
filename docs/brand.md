# LucidDesk 品牌规范

产品名称统一使用 **LucidDesk**，连写且 L、D 大写。暂不设置中文品牌名；展示文案不使用 Lucid Desk、Luciddesk 或旧名称代替产品名。

本页约定对外名称、视觉资源和需要保留的内部标识。发布包使用方法见[发布包说明](package.md)，版本递增与发布流程见[构建指南](development/build.md)。

## 产品表达

| 场景 | 统一表述 |
| --- | --- |
| 产品名称 | LucidDesk |
| 功能说明 | 桌面分组与文件整理 |
| 主标语 | 把 Windows 桌面整理成顺手的工作区。 |
| 简介 | 将桌面图标拖入分组，把常用文件夹留在手边，用 Everything 随时查找文件。 |
| 作者 | Yuchen95 |
| EXE 文件描述 | LucidDesk |

中文功能说明是产品描述，不是中文品牌名。文案优先描述具体操作和用途：把桌面图标收纳到分组改变的是引用归属，不能将其描述为移动真实文件；文件操作与面板使用方式见[使用指南](usage.md)。Everything 搜索需按实际配置启用，不能把它描述为无需外部程序的内置文件索引。

## 图标与视觉资源

应用采用[正方形布局的三面板图标](design/luciddesk-green-square-v19.png)：左侧翡翠绿、右上薄荷绿、右下深松绿，保留曲面层次、圆角和透明间隙。三个色块组成的主体轮廓宽高相等，透明画布留白在导出时统一处理。

EXE、窗口、托盘和关于页共用程序内嵌图标资源；安装器使用同一 ICO，README 和双语功能示意图使用同源 PNG，MSIX 图像由打包脚本从应用图标生成。修改图标时更新原稿及对应导出物，避免各入口使用不同版本。

当前 ICO 资源路径为 `app/assets/luciddesk.ico`。尺寸、留白、透明度及对应导出资源以[应用资源说明](../app/assets/README.md)为准。

## 程序与产物命名

| 对象 | 当前名称或规则 |
| --- | --- |
| Cargo 应用包 | `luciddesk` |
| 主程序 | `luciddesk.exe` |
| 桌面组件 | `luciddesk_explorer.dll` |
| 普通 ZIP | `LucidDesk-<版本>-<构建标识>-windows-x64-<时间戳>.zip` |
| 便携 ZIP | `LucidDesk-<版本>-windows-x64-portable.zip` |
| 安装程序 | `LucidDesk-<版本>-windows-x64-setup.exe` / `LucidDesk-<版本>-windows-x64.msi` |
| 应用及安装快捷方式的 AppUserModelID | `Yuchen95.LucidDesk` |

应用版本以根目录 `Cargo.toml` 的 `[workspace.package].version` 为构建来源，主程序继承后自动写入程序版本信息；不要单独修改 EXE 文件属性制造不同版本。MSIX 的包名、发布者和四段版本由包身份规则决定，不能仅按展示名称推导，详见 [MSIX 打包与运行](msix.md)。

AppUserModelID、安装器 AppId 和 MSIX 包身份承担不同职责，不是可互换的字符串。品牌文案调整不应顺带改变这些身份或在每次版本更新时生成新值；相关变更需要单独评估快捷方式、升级与包注册行为。

## 数据目录与内部标识

数据目录选择按以下优先级执行：

1. 已设置 `LUCIDDESK_DATA_DIR` 时使用指定目录。
2. 未指定环境变量，且程序旁存在 `portable` 文件时，使用程序旁的 `data`。
3. 其他情况使用 `%LOCALAPPDATA%\LucidDesk`。

程序不再识别旧品牌的数据目录、环境变量或通信标识，也不自动迁移或合并旧工作区。配置与数据的具体分工见[存储说明](development/storage.md)。

内部标识统一使用 `LucidDesk`，例如单实例互斥量 `Local\LucidDesk.DesktopSession`、唤醒消息 `LucidDesk.ShowExisting` 和 Hook 通信名称；环境变量使用 `LUCIDDESK_` 前缀。

## 修改时的核对项

- 检查界面、安装器、文件属性、文档和发布附件的展示名称是否一致。
- 涉及身份或路径时，单独核对升级、单实例、既有数据及卸载行为，不把它们作为普通文案修改处理。
- 更新本页时保留仍生效的兼容规则；已替代方案和更名过程通过 Git 历史查询，不在正文重复记录。
