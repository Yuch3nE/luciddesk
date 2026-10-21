import chineseCopy from '~/content/copy.zh.json';
import translations from '~/content/translations.json';
export type Locale = 'zh' | 'en';
export const localeOf = (pathname: string): Locale => /^\/en(?:\/|$)/.test(pathname) ? 'en' : 'zh';
export const localizedPath = (path: string, _lang: Locale) => path;
export const translator = (lang: Locale) => (text: string): string => lang === 'en' ? (translations as Record<string, string>)[text] ?? text : (chineseCopy as Record<string, string>)[text] ?? text;
