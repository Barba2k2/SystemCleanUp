export type LanguageCode = "en" | "pt-BR" | "es";

type LanguageOption = { code: LanguageCode; name: string };

export class Languages {
  static readonly defaultCode: LanguageCode = "en";

  static readonly options: readonly LanguageOption[] = [
    { code: "en", name: "English" },
    { code: "pt-BR", name: "Português (Brasil)" },
    { code: "es", name: "Español" },
  ];

  static isSupported(value: unknown): value is LanguageCode {
    return Languages.options.some((option) => option.code === value);
  }

  static detect(): LanguageCode {
    const browserLanguage = navigator.language.toLowerCase();
    const match = Languages.options.find(
      (option) =>
        browserLanguage === option.code.toLowerCase() ||
        browserLanguage.split("-")[0] === option.code.split("-")[0].toLowerCase(),
    );

    return match?.code ?? Languages.defaultCode;
  }
}
