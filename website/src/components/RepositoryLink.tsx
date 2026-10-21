import { Star, GitFork } from 'lucide-react';
import { Button } from './ui/button';
import repository from '~/data/repository.json';
export default function RepositoryLink({className = '', lang = 'zh'}: {className?: string; lang?: 'zh' | 'en'}) {
  return <Button asChild variant="outline" className={`h-11 shrink-0 gap-2.5 rounded-xl bg-transparent px-3 shadow-none dark:bg-transparent ${className}`}>
    <a href="https://github.com/Yuch3nE/luciddesk" aria-label={lang === 'en' ? `${repository.name}, ${repository.version}, ${repository.stars} stars on GitHub` : `${repository.name}，版本 ${repository.version}，${repository.stars} 个 Star，前往 GitHub`} title={`${lang === 'en' ? 'GitHub data updated' : 'GitHub 数据更新于'} ${repository.updatedAt.slice(0,10)}`}>
      <GitFork className="size-4 text-muted-foreground" aria-hidden="true"/>
      <span className="flex flex-col items-start gap-1">
        <span className="text-xs font-semibold leading-none">{repository.name}</span>
        <span className="flex items-center gap-3 text-[11px] leading-none text-muted-foreground">
          <span>{repository.version}</span>
          <span className="inline-flex items-center gap-1"><Star className="size-3" aria-hidden="true"/>{repository.stars.toLocaleString('en-US')}</span>
        </span>
      </span>
    </a>
  </Button>;
}
