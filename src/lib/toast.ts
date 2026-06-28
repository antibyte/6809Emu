import { writable } from "svelte/store";

export type ToastType = "info" | "success" | "error" | "warning";

export interface Toast {
  id: number;
  message: string;
  type: ToastType;
}

let nextId = 0;
export const toasts = writable<Toast[]>([]);

export function showToast(
  message: string,
  type: ToastType = "info",
  duration = 4000
) {
  const id = ++nextId;
  toasts.update((t) => [...t, { id, message, type }]);
  setTimeout(() => {
    toasts.update((t) => t.filter((x) => x.id !== id));
  }, duration);
}

export function dismissToast(id: number) {
  toasts.update((t) => t.filter((x) => x.id !== id));
}