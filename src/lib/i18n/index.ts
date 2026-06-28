import { writable, derived } from "svelte/store";
import de from "./de.json";
import en from "./en.json";

export type Locale = "de" | "en";

const dictionaries: Record<Locale, Record<string, string>> = { de, en };

const initialLocale =
  ((typeof localStorage !== "undefined"
    ? localStorage.getItem("locale")
    : null) as Locale) || "de";

if (typeof document !== "undefined") {
  document.documentElement.lang = initialLocale;
}

export const locale = writable<Locale>(initialLocale);

locale.subscribe((value) => {
  localStorage.setItem("locale", value);
  document.documentElement.lang = value;
});

export const t = derived(locale, ($locale) => {
  const dict = dictionaries[$locale];
  return (key: string) => dict[key] ?? key;
});

export function toggleLocale() {
  locale.update((l) => (l === "de" ? "en" : "de"));
}