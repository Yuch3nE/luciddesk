import zhSource from '../../../CHANGELOG.md?raw';
import enSource from '../../../CHANGELOG.en.md?raw';
import type { Locale } from '~/lib/i18n';

export interface ChangeGroup { title: string; items: string[] }
export interface Release { version: string; date: string | null; introduction: string[]; groups: ChangeGroup[] }
const categoryNames: Record<string, [string, string]> = {
  feat: ['新增功能', 'New features'], fix: ['问题修复', 'Fixes'],
  perf: ['性能优化', 'Performance'], changed: ['体验改进', 'Improvements'],
  docs: ['使用说明', 'Documentation'], build: ['安装与发布', 'Packaging'],
  ci: ['发布流程', 'Release process'], test: ['质量保障', 'Quality checks'],
};

export function parseChangelog(source: string, lang: Locale): Release[] {
  const releases: Release[] = [];
  let release: Release | undefined;
  let group: ChangeGroup | undefined;
  for (const line of source.split(/\r?\n/)) {
    const heading = line.match(/^## (\d+\.\d+\.\d+)\s*·\s*(.+)$/);
    if (heading) {
      release = { version: heading[1], date: /^\d{4}-\d{2}-\d{2}$/.test(heading[2]) ? heading[2] : null, introduction: [], groups: [] };
      releases.push(release);
      group = undefined;
    } else if (release && line.startsWith('### ')) {
      const title = line.slice(4).trim();
      group = { title: categoryNames[title]?.[lang === 'zh' ? 0 : 1] ?? title, items: [] };
      release.groups.push(group);
    } else if (release && line.startsWith('- ')) {
      if (!group) {
        group = { title: lang === 'zh' ? '本次更新' : 'What changed', items: [] };
        release.groups.push(group);
      }
      group.items.push(line.slice(2).trim());
    } else if (release && line.trim()) {
      // Continuations belong to the preceding bullet; prose before groups is a release note.
      if (group?.items.length && /^\s+/.test(line)) group.items[group.items.length - 1] += ` ${line.trim()}`;
      else release.introduction.push(line.trim());
    }
  }
  return releases;
}

export const releasesByLocale = { zh: parseChangelog(zhSource, 'zh'), en: parseChangelog(enSource, 'en') };
const zhVersions = releasesByLocale.zh.map(release => release.version).join(',');
if (zhVersions !== releasesByLocale.en.map(release => release.version).join(',')) throw new Error('Chinese and English changelog versions must match.');
export const PAGE_SIZE = 8;
export const releaseId = (version: string) => `v${version.replaceAll('.', '-')}`;
export const releaseDate = (date: string, lang: Locale) => new Intl.DateTimeFormat(lang === 'en' ? 'en-US' : 'zh-CN', {year:'numeric',month:'long',day:'numeric',timeZone:'UTC'}).format(new Date(`${date}T00:00:00Z`));

// Changelog entries use inline code and bold text. Escape first; do not execute raw HTML.
export const inlineNote = (value: string) => value.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
  .replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')
  .replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
