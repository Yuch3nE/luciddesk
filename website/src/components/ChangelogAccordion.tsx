import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from './ui/accordion';
import { Badge } from './ui/badge';
import ChangeBadge from './ChangeBadge';
import type { ChangeType } from '~/lib/change-types';
import type { Locale } from '~/lib/i18n';

export interface ChangelogEntry {
  version: string;
  id: string;
  date: string | null;
  dateLabel: string;
  latest: boolean;
  introduction: string[];
  counts: [ChangeType, number][];
  groups: {title: string; items: {html: string; type: ChangeType}[]}[];
}

export function ChangelogBody({entry, lang}: {entry: ChangelogEntry; lang: Locale}) {
  return <div>
    {entry.introduction.map((html, i) => <p key={i} className="help-copy mb-5 text-sm!" dangerouslySetInnerHTML={{__html: html}}/>)}
    {entry.groups.map((group, i) => <section key={i} className="border-t border-line py-6 last:pb-0">
      <h4 className="mb-4 text-base font-semibold text-heading">{group.title}</h4>
      <ul className="space-y-4">{group.items.map((item, j) => <li key={j} className="changelog-entry flex items-start gap-3">
        <ChangeBadge type={item.type} lang={lang}/>
        <span className="help-copy min-w-0 flex-1 text-sm! leading-7!" dangerouslySetInnerHTML={{__html:item.html}}/>
      </li>)}</ul>
    </section>)}
  </div>;
}

export default function ChangelogAccordion({entries, lang}: {entries: ChangelogEntry[]; lang: Locale}) {
  return <Accordion type="multiple" defaultValue={entries.filter(entry => entry.latest).map(entry => entry.version)} className="space-y-4">
    {entries.map(entry => <AccordionItem key={entry.version} value={entry.version} id={entry.id} className="rounded-2xl border border-line bg-card/60 px-5 backdrop-blur-sm last:border-b sm:px-8">
      <AccordionTrigger className="items-center py-6 hover:no-underline" aria-label={`${lang === 'en' ? 'Version' : '版本'} ${entry.version}`}>
        <span className="flex min-w-0 flex-1 flex-col gap-3">
          <span className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <span className="text-xl font-semibold text-heading">v{entry.version}</span>
            {entry.latest && <Badge className="bg-primary/10 text-primary dark:text-blue-300">{lang === 'en' ? 'Latest' : '最新版本'}</Badge>}
            {entry.date ? <time dateTime={entry.date} className="text-xs font-normal text-muted sm:ml-auto">{entry.dateLabel}</time> : <span className="text-xs font-normal text-muted">{entry.dateLabel}</span>}
          </span>
          <span className="flex flex-wrap gap-2">{entry.counts.map(([type, count]) => <ChangeBadge key={type} type={type} count={count} lang={lang}/>)}</span>
        </span>
      </AccordionTrigger>
      <AccordionContent className="pb-7"><ChangelogBody entry={entry} lang={lang}/></AccordionContent>
    </AccordionItem>)}
  </Accordion>;
}
