# 文档导航

第一次使用，从[中文使用手册](usage.md)或 [English manual](usage.en.md)开始。官网[使用指南](https://luciddesk.bbkingdom365.com/guide/)使用同一份内容。

以下使用文档适用于 **0.24.0**。找不到选项时，先检查“设置 → 关于”中的版本，并使用与主程序配套的 CLI。

## 安装、更新与迁移

| 你的情况 | 阅读 |
| --- | --- |
| 日常安装、更新或卸载 | [安装版](installer.md) |
| 解压运行，配置保存在用户目录 | [普通 ZIP](package.md) |
| 希望配置随程序一起携带 | [便携版](portable.md) |
| 换电脑或恢复布局 | [备份与恢复](usage.md#数据与备份) |

普通 ZIP 与便携版的数据位置不同。迁移前，通过托盘“打开配置目录”确认实际位置。**配置备份不包含原始文件**，换电脑时需要单独迁移这些文件。

## 按任务查找

| 想做什么 | 阅读 |
| --- | --- |
| 新建面板、收纳图标 | [桌面面板](usage.md#桌面面板) |
| 合并、切换和分离标签 | [标签页](usage.md#标签页) |
| 直接浏览常用文件夹 | [文件夹面板](usage.md#文件夹面板) |
| 查找或预览文件 | [Everything 搜索](usage.md#everything-搜索面板) · [文件预览](usage.md#文件预览) |
| 调整主题、材质和字体 | [外观设置](usage.md#外观设置) |
| 使用键盘操作 | [快捷键](usage.md#键盘操作) |
| 备份、导出或恢复配置 | [数据与备份](usage.md#数据与备份) |
| 连接 Agent 或编写脚本 | [CLI 指南](cli.md) · [English quick start](cli.md#english-quick-start) |
| 找回面板、排查连接问题 | [常见问题](usage.md#常见问题) |

## 版本与数据

- [中文更新记录](../CHANGELOG.md) · [English changelog](../CHANGELOG.en.md)
- [隐私说明](../PRIVACY.md) · [Privacy policy](../PRIVACY.en.md)
- [配置示例](config.example.toml)：适合需要手动编辑的用户；先备份，编辑后重新加载或重启。它不是完整布局备份。

## 开发与贡献

普通使用无需编译或阅读实现文档。参与开发请看[贡献指南](../CONTRIBUTING.md)、[构建指南](development/build.md)与[开发文档导航](development/README.md)。[MSIX 打包](msix.md)和[品牌规范](brand.md)面向打包与维护工作。
