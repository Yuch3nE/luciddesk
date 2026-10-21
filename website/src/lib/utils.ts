import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
export function cn(...inputs: ClassValue[]) { return twMerge(clsx(inputs)); }

export const getColumnsClass = (columns?: number, fallback = ''): string =>
  columns === 4
    ? 'lg:grid-cols-4 md:grid-cols-3 sm:grid-cols-2'
    : columns === 3
      ? 'lg:grid-cols-3 sm:grid-cols-2'
      : columns === 2
        ? 'sm:grid-cols-2'
        : fallback;
