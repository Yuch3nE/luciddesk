import { Button } from './ui/button';
import type { Locale } from '~/lib/i18n';

export default function LanguageLink({lang}: {lang: Locale}) {
  return <Button type="button" data-language-switch data-language-target={lang === 'en' ? 'zh' : 'en'} aria-label={lang === 'en' ? 'Switch to Chinese' : '切换为英文'} title={lang === 'en' ? 'Switch to Chinese' : '切换为英文'} variant="ghost" className="h-9 shrink-0 rounded-full border border-border/60 bg-transparent p-0.5 text-xs font-medium hover:bg-transparent dark:hover:bg-transparent">
      <span className="language-track relative inline-flex h-7 items-center gap-0.5" data-language-selected={lang} aria-hidden="true">
        <span className="language-thumb absolute inset-y-0 left-0 w-8 rounded-full bg-background shadow-sm" />
        <span className={`relative inline-flex h-7 w-8 items-center justify-center leading-4 ${lang === 'en' ? 'text-foreground' : 'text-muted-foreground'}`}>EN</span>
        <span className={`relative inline-flex h-7 w-8 items-center justify-center leading-4 ${lang === 'zh' ? 'text-foreground' : 'text-muted-foreground'}`}>中</span>
      </span>
  </Button>;
}
