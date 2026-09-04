import { defaultLocale, locales, type Locale } from '../config';
import { en, type Translations } from './en';
import { de } from './de';

export const translations: Record<Locale, Translations> = { en, de };

export const localeNames: Record<Locale, string> = {
  en: 'English',
  de: 'Deutsch',
};

export function isLocale(value: string | undefined): value is Locale {
  return locales.includes(value as Locale);
}

export function t(locale: Locale): Translations {
  return translations[locale] ?? translations[defaultLocale];
}

/** Build a locale-prefixed path with a trailing slash, e.g. `/de/pricing/`. */
export function localePath(locale: Locale, path = ''): string {
  const clean = path.replace(/^\/+|\/+$/g, '');
  return clean ? `/${locale}/${clean}/` : `/${locale}/`;
}

/** Strip the locale prefix from a pathname so it can be re-prefixed. */
export function stripLocale(pathname: string): string {
  const parts = pathname.split('/').filter(Boolean);
  if (parts.length && isLocale(parts[0])) parts.shift();
  return parts.join('/');
}

export function getStaticLocalePaths() {
  return locales.map((lang) => ({ params: { lang } }));
}
