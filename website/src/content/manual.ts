import * as zhManual from '../../../docs/usage.md';
import * as enManual from '../../../docs/usage.en.md';
import type { HelpPage } from './help';

// Keep the public guide anchors stable while the Markdown headings remain readable on GitHub.
const anchors: Record<string, string> = {
  '版本与阅读方式': 'versions', 'Versions and reading this manual': 'versions',
  '五分钟开始使用': 'first-panel', 'Start in five minutes': 'first-panel',
  '操作范围与文件安全': 'file-safety', 'What changes files': 'file-safety',
  '桌面面板': 'panels', 'Desktop panels': 'panels',
  '标签页': 'tabs', 'Tabs': 'tabs',
  '文件夹面板': 'folders', 'Folder panels': 'folders',
  'Everything 搜索面板': 'search', 'Everything search': 'search',
  '键盘操作': 'shortcuts', 'Keyboard and file operations': 'shortcuts',
  '文件预览': 'preview', 'File preview': 'preview',
  '外观设置': 'organize', 'Theme, materials and appearance': 'organize',
  '设置导航': 'settings', 'Settings and scope': 'settings',
  'Agent 与命令行': 'agent', 'Agents and the command line': 'agent',
  '数据与备份': 'backup', 'Backup and restore': 'backup',
  '常见问题': 'troubleshooting', 'Troubleshooting': 'troubleshooting',
  '字体': 'fonts', 'Fonts and language': 'fonts',
  '检查更新': 'updates', 'Updates': 'updates',
};

async function manualPage(source: typeof zhManual, english: boolean): Promise<HelpPage> {
  const ref = import.meta.env.RELEASE_TAG || 'main';
  if (ref !== 'main' && !/^v?\d+\.\d+\.\d+$/.test(ref)) throw new Error('Invalid manual release reference');
  let html = await source.compiledContent();
  const headings = source.getHeadings().filter(heading => heading.depth === 2);
  const ids = new Map(headings.map(heading => [heading.slug, anchors[heading.text] ?? heading.slug]));
  // Docs-relative links must resolve from the website, not from /guide/.
  html = html.replace(/(href|src)="([^"]+)"/g, (_match, attribute: string, target: string) => {
    if (target.startsWith('#')) target = '#' + (ids.get(decodeURIComponent(target.slice(1))) ?? target.slice(1));
    else if (target === 'usage.md' || target === 'usage.en.md') target = '/guide/';
    else if (target.startsWith('../CHANGELOG')) target = '/changelog/';
    else if (['installer.md', 'portable.md', 'package.md', 'msix.md'].includes(target)) target = '/installation/';
    else if (target === 'cli.md' || target.startsWith('cli.md#')) target = '/agent/';
    else if (!/^(https?:|mailto:|\/)/.test(target)) {
      const path = new URL(target, `https://github.com/Yuch3nE/luciddesk/blob/${ref}/docs/`);
      target = attribute === 'src'
        ? path.href.replace(`github.com/Yuch3nE/luciddesk/blob/${ref}`, `raw.githubusercontent.com/Yuch3nE/luciddesk/${ref}`)
        : path.href;
    }
    return `${attribute}="${target}"`;
  });
  const blocks = [...html.matchAll(/<h2\b[^>]*>[\s\S]*?<\/h2>/g)];
  if (blocks.length !== headings.length) throw new Error('Manual headings and rendered sections differ');
  const sections = headings.map((heading, index) => {
    const start = blocks[index].index! + blocks[index][0].length;
    return {
      id: ids.get(heading.slug)!, title: heading.text,
      html: html.slice(start, blocks[index + 1]?.index ?? html.length)
        .replace(/<table>/g, '<div class="manual-table" tabindex="0" role="region" aria-label="' + (english ? 'Scrollable table' : '可横向滚动的表格') + '"><table>')
        .replace(/<\/table>/g, '</table></div>'),
    };
  });
  return {
    slug: 'guide', label: english ? 'User guide' : '使用指南',
    title: english ? 'LucidDesk User Manual' : 'LucidDesk 使用手册',
    description: english
      ? 'Create a panel, organize your files, and find the steps you need for search, previews and backups.'
      : '从创建面板开始，了解文件整理、搜索预览和备份恢复的常用操作。',
    sections,
  };
}

export const manualZh = await manualPage(zhManual, false);
export const manualEn = await manualPage(enManual, true);
