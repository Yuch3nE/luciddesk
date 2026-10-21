import { Button } from './ui/button';
import { BookOpen, Download, Sparkles, ShieldCheck } from 'lucide-react';
const icons = { installation: Download, guide: BookOpen, agent: Sparkles, privacy: ShieldCheck };
export default function HelpPageLinks({pages, current}: {pages: {slug:string;label:string}[]; current:string}) {
  return <div className="grid grid-cols-2 gap-1 lg:grid-cols-1">{pages.map(item => {
    const Icon = icons[item.slug as keyof typeof icons];
    return <Button key={item.slug} asChild variant="ghost" className={`h-auto justify-start gap-2.5 whitespace-normal px-3 py-3 text-sm font-medium ${current === item.slug ? 'bg-primary/10 text-primary dark:text-blue-300' : 'text-muted'}`}><a href={`/${item.slug}/`} aria-current={current === item.slug ? 'page' : undefined}>{Icon && <Icon className="size-4 shrink-0" aria-hidden="true"/>}{item.label}</a></Button>;
  })}</div>;
}
