# 官网自动发布

GitHub 仓库 `Yuch3nE/luciddesk` 的 `Deploy website` 工作流监听 `Build CI` 完成，仅在其 `release` 任务成功后执行。不能只依赖 `release: published`：现有发布使用 `GITHUB_TOKEN`，其产生的事件通常不会触发另一个工作流。

## 配置 Secrets

在 GitHub 仓库 **Settings → Secrets and variables → Actions → Repository secrets** 添加：

| 名称 | 内容 |
| --- | --- |
| `CLOUDFLARE_API_TOKEN` | Cloudflare 部署 API Token，不是 Global API Key |
| `CLOUDFLARE_ACCOUNT_ID` | 当前 `luciddesk-website` Worker 所属账户 ID |

创建 Token 时使用 Cloudflare 的 **Edit Cloudflare Workers** 模板，只授权目标账户；如设置区域权限，只选择 `bbkingdom365.com`。需要能够部署 `luciddesk-website`。若使用按 Worker 授权的模式，授予该 Worker 的 Editor 权限；修改路由或自定义域名还需要对应区域的 Workers Routes Write。已有域名保持不变时可限制为部署权限。

GitHub API 使用工作流自动提供的 `GITHUB_TOKEN`，无需额外 PAT。Cloudflare Secrets 仅传给部署步骤，不写入源码、静态产物或日志。

官方说明：[GitHub Actions 部署](https://developers.cloudflare.com/workers/ci-cd/external-cicd/github-actions/)、[Workers 权限](https://developers.cloudflare.com/workers/authorization/workers/)。

## 更新内容与保护

- 官网代码从默认 `main` 分支读取；版本信息读取最新正式 GitHub Release，拒绝草稿、预发布和缺失安装包/校验文件的版本。
- 从该 Release 的标签检出文档，覆盖构建工作区中的中英文手册、CLI 文档、安装说明、隐私 Markdown 和 changelog。更新日志仅用 main 中对应版本的中英文章节替换，允许发布后修正文案；不会导入 main 的未来版本或其他历史章节。仓库和标签不会被改写。独立的官网 Agent、安装和隐私介绍页仍由网站源码维护。
- 标签版本必须匹配 Cargo 版本；自动触发还核对发布构建的提交。旧构建不能覆盖新发布。
- CI API 请求失败时直接失败，禁止使用缓存版本部署。部署前再次检查最新版本。
- 执行 `npm ci`、网站测试、类型检查、双语生成与手册链接检查，通过后使用固定 Wrangler 版本部署。部署任务串行运行，不中断正在进行的部署。
- 发布 `release.json`，记录标签、提交、更新日志修订来源提交及最终文档 SHA-256；部署后核对线上记录和指南页面。构建失败不会触碰线上；部署后验证失败会报告错误，需检查 Cloudflare 部署记录，不会自动回滚。

## 手动重试

工作流合入并推送 GitHub 默认分支后，在 **Actions → Deploy website → Run workflow** 执行。`release_tag` 留空选择最新正式版，也可填当前最新标签，例如 `v0.23.0`；旧版本会被拒绝。此入口无需重新构建 Windows 安装包。

已经完成的发布不会因新增工作流自动补跑，首次可使用手动入口。修正已发布版本的更新日志时，同步修改 main 中的中英文对应章节，推送后手动部署，并用 `tools/release/release-notes.py` 生成同一版本的 GitHub Release 正文。两种语言都必须有唯一且非空的对应章节，否则部署失败。其他文档仍按标签原文导入，不会自动删除“未发布”描述；发布前应整理好内容。本流程不移动已有标签。
