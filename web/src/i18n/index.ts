import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from './locales/en.json'
import ru from './locales/ru.json'

// English is the source language, never a fallback for a missing translation.
// `tools/check-locales.mjs` holds every locale to the same shape at build time,
// so there is nothing to fall back *to*: a gap fails the build instead of
// reaching a person as a stray English line in a Russian screen.
export const defaultNS = 'translation'
export const resources = {
  en: { translation: en },
  ru: { translation: ru },
} as const

export type Language = keyof typeof resources
export const LANGUAGES = Object.keys(resources) as Language[]

/** A language's name in itself - `English`, and Russian written in Russian - so a reader who
 * cannot read the current language still finds their own. Asked of `Intl`,
 * which knows every language's name for itself; CLDR writes some of them in
 * lower case, and a menu entry starts with a capital. */
export function languageName(language: Language): string {
  const name = new Intl.DisplayNames([language], { type: 'language' }).of(language) ?? language
  return name.charAt(0).toLocaleUpperCase(language) + name.slice(1)
}

const STORAGE_KEY = 'austeris.language'

/** The language this browser chose last, else the one it asks pages for,
 * else English. Remembered per browser: a choice of language is about the
 * person at this screen, not about the books. */
function initialLanguage(): Language {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored && stored in resources) return stored as Language
  } catch {
    // Storage refused (a private window, blocked site data): ask the browser.
  }
  const asked = typeof navigator === 'undefined' ? [] : navigator.languages
  for (const tag of asked) {
    const base = tag.toLowerCase().split('-')[0] ?? ''
    if (base in resources) return base as Language
  }
  return 'en'
}

/**
 * `<html lang>` follows the interface's language.
 *
 * That attribute is what a screen reader reads the page by, and it is also
 * what dowel's `useLocale()` answers with - so every date and number on the
 * screen is written in the interface's language rather than the browser's,
 * from this one line.
 */
function follow(language: string) {
  if (typeof document !== 'undefined') document.documentElement.lang = language
}

i18n.on('languageChanged', follow)

void i18n.use(initReactI18next).init({
  resources,
  lng: initialLanguage(),
  fallbackLng: false,
  interpolation: { escapeValue: false },
})
follow(i18n.language)

/** Switch the interface's language, and remember it in this browser. */
export function setLanguage(language: Language) {
  try {
    localStorage.setItem(STORAGE_KEY, language)
  } catch {
    // Not remembered; still switched for this visit.
  }
  void i18n.changeLanguage(language)
}

export default i18n
