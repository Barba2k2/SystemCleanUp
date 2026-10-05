import i18next from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en.json";
import es from "./locales/es.json";
import ptBR from "./locales/pt-BR.json";
import { Languages, type LanguageCode } from "./languages";

export class I18n {
  static init(language: LanguageCode): void {
    const initialLanguage = Languages.isSupported(language)
      ? language
      : Languages.defaultCode;

    void i18next.use(initReactI18next).init({
      resources: {
        en: { translation: en },
        "pt-BR": { translation: ptBR },
        es: { translation: es },
      },
      lng: initialLanguage,
      fallbackLng: Languages.defaultCode,
      interpolation: { escapeValue: false },
    });
    document.documentElement.lang = initialLanguage;
  }

  static async change(language: LanguageCode): Promise<void> {
    await i18next.changeLanguage(language);
    document.documentElement.lang = language;
  }
}
