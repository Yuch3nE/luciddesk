import { useEffect, useState } from 'react';
import { Moon, Sun } from 'lucide-react';
import { Button } from './ui/button';

export default function ThemeToggle({lang = 'zh'}: {lang?: 'zh' | 'en'}) {
  const [dark, setDark] = useState(false);
  useEffect(() => {
    const sync = () => setDark(document.documentElement.dataset.theme === 'dark');
    sync();
    document.addEventListener('luciddesk:theme-updated', sync);
    return () => document.removeEventListener('luciddesk:theme-updated', sync);
  }, []);
  const label = lang === 'en'
    ? (dark ? 'Switch to light mode' : 'Switch to dark mode')
    : (dark ? '切换为浅色模式' : '切换为深色模式');
  return <Button variant="ghost" size="icon" className="size-11 rounded-xl text-muted-foreground" aria-label={label} title={label} onClick={() => {
    // Read the applied theme, including system changes, at the moment of the click.
    const next = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
    document.dispatchEvent(new CustomEvent('luciddesk:theme-change', {detail: next}));
  }}>
    <Sun className="size-[18px] dark:hidden" aria-hidden="true"/>
    <Moon className="hidden size-[18px] dark:block" aria-hidden="true"/>
  </Button>;
}
