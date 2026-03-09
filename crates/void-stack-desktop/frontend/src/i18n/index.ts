import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import es from './es.json'
import en from './en.json'

const STORAGE_KEY = 'void-stack-lang'

const savedLang = localStorage.getItem(STORAGE_KEY) || 'es'

i18n
  .use(initReactI18next)
  .init({
    resources: {
      es: { translation: es },
      en: { translation: en },
    },
    lng: savedLang,
    fallbackLng: 'es',
    interpolation: {
      escapeValue: false,
    },
  })

// Persist language changes
i18n.on('languageChanged', (lng) => {
  localStorage.setItem(STORAGE_KEY, lng)
})

export default i18n

export const LANGUAGES = [
  { code: 'es', label: 'Español' },
  { code: 'en', label: 'English' },
] as const
