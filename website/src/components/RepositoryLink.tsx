import { Star, GitFork } from 'lucide-react';
import { Button } from './ui/button';
import repository from '~/data/repository.json';
export default function RepositoryLink({className = '', lang = 'zh', compact = false}: {className?: string; lang?: 'zh' | 'en'; compact?: boolean}) {
  return <Button asChild variant="ghost" className={`h-9 shrink-0 gap-2 rounded-lg px-2 text-muted-foreground hover:text-foreground ${className}`}>
    <a href="https://github.com/Yuch3nE/luciddesk" aria-label={lang === 'en' ? `${repository.name}, ${repository.version}, ${repository.stars} stars on GitHub` : `${repository.name}，版本 ${repository.version}，${repository.stars} 个 Star，前往 GitHub`} title={`${repository.name} · ${repository.version} · ${lang === 'en' ? 'Updated' : '更新于'} ${repository.updatedAt.slice(0,10)}`}>
      <GitFork className="size-4 text-muted-foreground" aria-hidden="true"/>
      <span className="text-xs font-medium leading-4">{compact ? 'GitHub' : repository.name}</span>
      <span className="ml-1 inline-flex items-center gap-1.5 text-xs font-medium leading-4 tabular-nums"><Star className="size-3.5!" aria-hidden="true"/>{repository.stars.toLocaleString('en-US')}</span>
    </a>
  </Button>;
}
