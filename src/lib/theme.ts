import { writable } from "svelte/store";

export type Theme = "dark" | "light" | "high-contrast";

const stored =
  (typeof localStorage !== "undefined"
    ? localStorage.getItem("theme")
    : null) as Theme | null;

const initialTheme: Theme = stored || "dark";

if (typeof document !== "undefined") {
  document.documentElement.dataset.theme = initialTheme;
}

export const theme = writable<Theme>(initialTheme);

theme.subscribe((value) => {
  if (typeof document !== "undefined") {
    document.documentElement.dataset.theme = value;
    localStorage.setItem("theme", value);
  }
});

export function cycleTheme() {
  theme.update((t) => {
    if (t === "dark") return "light";
    if (t === "light") return "high-contrast";
    return "dark";
  });
}