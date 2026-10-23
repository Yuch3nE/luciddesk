import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from './ui/accordion';
import ChangeBadge from './ChangeBadge';
import type { ChangeType } from '~/lib/change-types';
import { Badge } from './ui/badge';
import { ArrowUpRight } from 'lucide-react';
import type { Locale } from '~/lib/i18n';

export interface ChangelogEntry {
  version: string;
  id: string;
  date: string | null;
  dateLabel: string;
  latest: boolean;
  introduction: string[];
  groups: {title: string; items: {html: string; type: ChangeType}[]}[];
}

export function ChangelogBody({entry, lang, headingLevel = 3}: {entry: ChangelogEntry; lang: Locale; headingLevel?: 2 | 3}) {
  const Heading = headingLevel === 2 ? 'h2' : 'h3';
  return <div className="release-body">
    {entry.introduction.length > 0 && <div className="release-intro">{entry.introduction.map((html, i) => <p key={i} dangerouslySetInnerHTML={{__html: html}}/>)}</div>}
    {entry.groups.map((group, i) => <section key={i} id={`${entry.id}-section-${i}`} className="release-section">
      <Heading className="release-section-title">{group.title}</Heading>
      <ul className="release-items">{group.items.map((item, j) => <li key={j}><ChangeBadge type={item.type} lang={lang}/><span dangerouslySetInnerHTML={{__html:item.html}}/></li>)}</ul>
    </section>)}
  </div>;
}

export default function ChangelogAccordion({entries, lang}: {entries: ChangelogEntry[]; lang: Locale}) {
  const countsFor = (entry: ChangelogEntry) => {
    const counts = new Map<ChangeType, number>();
    entry.groups.forEach(group => group.items.forEach(item => counts.set(item.type, (counts.get(item.type) ?? 0) + 1)));
    return [...counts];
  };
  const en = lang === 'en';
  return <Accordion type="multiple" defaultValue={entries.slice(0, 1).map(entry => entry.version)} className="release-list">
    {entries.map(entry => <AccordionItem key={entry.version} value={entry.version} id={entry.id} className="release-card">
      <AccordionTrigger className="release-trigger" aria-label={`${en ? 'Version' : '版本'} ${entry.version}`}>
        <span className="min-w-0 flex-1">
          <span className="release-version-row flex flex-wrap items-center gap-x-3 gap-y-2">
            <span className="release-version-number text-2xl font-semibold tracking-tight text-heading">v{entry.version}</span>
            {entry.latest && <Badge className="border-primary/15 bg-primary/10 text-primary dark:text-blue-300">{en ? 'Latest' : '最新版本'}</Badge>}
          </span>
          <span className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm font-normal text-muted">
            {entry.date ? <time dateTime={entry.date}>{entry.dateLabel}</time> : <span>{entry.dateLabel}</span>}
            <span aria-hidden="true">·</span>
            <span>{entry.groups.reduce((sum, group) => sum + group.items.length, 0)} {en ? 'updates' : '项更新'}</span>
          </span>
          <span className="mt-3 flex flex-wrap gap-2">{countsFor(entry).map(([type, count]) => <ChangeBadge key={type} type={type} count={count} lang={lang}/>)}</span>
        </span>
      </AccordionTrigger>
      <AccordionContent className="release-content">
        <ChangelogBody entry={entry} lang={lang}/>
        <a href={`/changelog/${entry.version}/`} className="release-permalink">{en ? 'View version details' : '查看版本详情'}<ArrowUpRight size={16} aria-hidden="true"/></a>
      </AccordionContent>
    </AccordionItem>)}
  </Accordion>;
}
