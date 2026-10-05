type LanguageSelectorOption = {
  value: string;
  label: string;
};

type LanguageSelectorProps = {
  label: string;
  value: string;
  options: readonly LanguageSelectorOption[];
  onChange: (value: string) => void;
};

function LanguageSelector({
  label,
  value,
  options,
  onChange,
}: LanguageSelectorProps) {
  return (
    <label className="language-selector">
      <span className="language-selector-label">{label}</span>
      <select
        className="language-selector-control"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}

export default LanguageSelector;
