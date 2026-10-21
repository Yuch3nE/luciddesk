import { Button } from './ui/button';
import type { Locale } from '~/lib/i18n';

export default function LanguageLink({lang}: {lang: Locale}) {
  return <Button type="button" data-language-switch data-language-target={lang === 'en' ? 'zh' : 'en'} aria-label={lang === 'en' ? 'Switch to Chinese' : '切换为英文'} title={lang === 'en' ? 'Switch to Chinese' : '切换为英文'} variant="ghost" className="h-11 rounded-xl px-1 text-xs hover:bg-transparent dark:hover:bg-transparent">
      <span className="inline-flex items-center gap-0.5 rounded-full border border-border/60 bg-accent/40 p-0.5" aria-hidden="true">
        <span className={`rounded-full px-2.5 py-1.5 leading-none ${lang === 'en' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground'}`}>EN</span>
        <span className={`rounded-full px-2.5 py-1.5 leading-none ${lang === 'zh' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground'}`}>中</span>
      </span>
  </Button>;
}
