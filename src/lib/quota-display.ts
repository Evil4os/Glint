import type { ProviderSnapshot, UsageWindow } from "../types";

export type PrimaryQuotaKind = "short" | "weekly";

export interface QuotaDisplay {
  primaryKind: PrimaryQuotaKind | null;
  primaryWindow: UsageWindow | null;
  secondaryWeeklyWindow: UsageWindow | null;
}

export function resolveQuotaDisplay(snapshot: Pick<ProviderSnapshot, "shortWindow" | "weeklyWindow">): QuotaDisplay {
  if (snapshot.shortWindow) {
    return {
      primaryKind: "short",
      primaryWindow: snapshot.shortWindow,
      secondaryWeeklyWindow: snapshot.weeklyWindow,
    };
  }

  if (snapshot.weeklyWindow) {
    return {
      primaryKind: "weekly",
      primaryWindow: snapshot.weeklyWindow,
      secondaryWeeklyWindow: null,
    };
  }

  return {
    primaryKind: null,
    primaryWindow: null,
    secondaryWeeklyWindow: null,
  };
}
