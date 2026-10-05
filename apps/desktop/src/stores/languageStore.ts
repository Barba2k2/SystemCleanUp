import { create } from "zustand";
import { persist } from "zustand/middleware";

import { I18n } from "../i18n/i18n";
import { Languages, type LanguageCode } from "../i18n/languages";

type LanguageState = {
  language: LanguageCode;
  setLanguage: (language: LanguageCode) => void;
};

export const useLanguageStore = create<LanguageState>()(
  persist(
    (set) => ({
      language: Languages.detect(),
      setLanguage: (language) => {
        set({ language });
        void I18n.change(language);
      },
    }),
    { name: "system-cleanup.language" },
  ),
);
