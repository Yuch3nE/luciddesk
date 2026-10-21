import { Plus, Sparkles, Wrench, Gauge, BookOpen, Package, Minus, Settings2 } from 'lucide-react';
import { Badge } from './ui/badge';
import type { ChangeType } from '~/lib/change-types';
import type { Locale } from '~/lib/i18n';

const types = {
  feature: { labels: ['新增', 'Added'], Icon: Plus, style: 'border-emerald-600/20 bg-emerald-500/10 text-emerald-800 dark:text-emerald-300' },
  improvement: { labels: ['优化', 'Improved'], Icon: Sparkles, style: 'border-blue-600/20 bg-blue-500/10 text-blue-800 dark:text-blue-300' },
  fix: { labels: ['修复', 'Fixed'], Icon: Wrench, style: 'border-amber-600/20 bg-amber-500/10 text-amber-800 dark:text-amber-300' },
  performance: { labels: ['性能', 'Performance'], Icon: Gauge, style: 'border-violet-600/20 bg-violet-500/10 text-violet-800 dark:text-violet-300' },
  docs: { labels: ['文档', 'Docs'], Icon: BookOpen, style: 'border-cyan-600/20 bg-cyan-500/10 text-cyan-800 dark:text-cyan-300' },
  packaging: { labels: ['发布', 'Packaging'], Icon: Package, style: 'border-indigo-600/20 bg-indigo-500/10 text-indigo-800 dark:text-indigo-300' },
  removed: { labels: ['移除', 'Removed'], Icon: Minus, style: 'border-rose-600/20 bg-rose-500/10 text-rose-800 dark:text-rose-300' },
  maintenance: { labels: ['维护', 'Maintenance'], Icon: Settings2, style: 'border-slate-500/20 bg-slate-500/10 text-slate-700 dark:text-slate-300' },
};

export default function ChangeBadge({type, lang, count}: {type: ChangeType; lang: Locale; count?: number}) {
  const {labels, Icon, style} = types[type];
  return <Badge variant="outline" className={`shrink-0 gap-1.5 rounded-md px-2 py-1 text-xs font-medium ${style}`}>
    <Icon className="size-3" aria-hidden="true"/>{labels[lang === 'en' ? 1 : 0]}{count !== undefined && <span className="tabular-nums opacity-80">{count}</span>}
  </Badge>;
}
