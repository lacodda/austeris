import { useTranslation } from 'react-i18next'
import { Segment, SegmentedControl } from '@/components/ui/segmented-control'
import { languageName, LANGUAGES, setLanguage, type Language } from '@/i18n'

/** The interface's language, switched in place and remembered in this browser. */
export function LanguageSwitch() {
  const { t, i18n } = useTranslation()
  return (
    <SegmentedControl
      aria-label={t('language.label')}
      value={i18n.language}
      onValueChange={(language) => setLanguage(language as Language)}
    >
      {LANGUAGES.map((language) => (
        <Segment key={language} value={language}>
          <span lang={language}>{languageName(language)}</span>
        </Segment>
      ))}
    </SegmentedControl>
  )
}
