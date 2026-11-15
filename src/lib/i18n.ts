import type { Language } from "../types";
export const copy = {
  "zh-CN": {
    availableLabel: (percent: number) => `5 小时额度剩余 ${percent}%`,
    weeklyAvailableLabel: (percent: number) => `本周额度剩余 ${percent}%`,
    unavailableStatus: "暂时无法读取额度",
    loadingQuota: "正在读取额度",
  },
  en: {
    availableLabel: (percent: number) => `5-hour quota remaining ${percent}%`,
    weeklyAvailableLabel: (percent: number) => `Weekly quota remaining ${percent}%`,
    unavailableStatus: "Quota temporarily unavailable",
    loadingQuota: "Reading quota",
  },
} as const;
export function normalizeLanguage(value: unknown): Language {
  return value === "en" ? "en" : "zh-CN";
}
