import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function clamp(value: number, min = 0, max = 100) {
  return Math.min(max, Math.max(min, value));
}

export function percentage(part: number, total: number) {
  if (total <= 0) return 0;
  return Math.round((part / total) * 100);
}

export function sum(values: number[]) {
  return values.reduce((acc, v) => acc + v, 0);
}

export function average(values: number[]) {
  return values.length === 0 ? 0 : sum(values) / values.length;
}

export function groupBy<T, K extends string>(items: T[], key: (item: T) => K) {
  return items.reduce<Record<K, T[]>>(
    (acc, item) => {
      const k = key(item);
      (acc[k] ??= []).push(item);
      return acc;
    },
    {} as Record<K, T[]>,
  );
}

export function unique<T>(items: T[]) {
  return Array.from(new Set(items));
}

export function sortBy<T>(items: T[], key: (item: T) => number, dir: "asc" | "desc" = "asc") {
  return [...items].sort((a, b) => (dir === "asc" ? key(a) - key(b) : key(b) - key(a)));
}

/** 稳定 ID 生成：Mock 数据与服务端回落共用 */
export function createId(prefix: string) {
  const rand = Math.random().toString(36).slice(2, 8);
  return `${prefix}_${Date.now().toString(36)}${rand}`;
}

export function truncate(text: string, max = 80) {
  return text.length <= max ? text : `${text.slice(0, max - 1)}…`;
}

/** 归一化用户输入，用于拼写/翻译题判分 */
export function normalizeAnswer(input: string) {
  return input
    .trim()
    .toLowerCase()
    .replace(/[.,!?;:"'’]/g, "")
    .replace(/\s+/g, " ");
}
