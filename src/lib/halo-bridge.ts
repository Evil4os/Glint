export interface HaloPresentation {
  percent: number | null;
  expanded: boolean;
  sidebarDocked: boolean;
}

export function supportsNativeHalo(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window && /Mac/.test(navigator.platform);
}

export async function publishHaloPresentation(state: HaloPresentation): Promise<void> {
  const { emitTo } = await import("@tauri-apps/api/event");
  await emitTo("halo", "quota-halo-state", state);
}

export async function listenHaloReady(onReady: () => void): Promise<() => void> {
  const { emitTo, listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen("quota-halo-ready", onReady);
  try {
    await emitTo("halo", "quota-halo-request", null);
  } catch (error) {
    unlisten();
    throw error;
  }
  return unlisten;
}

export async function listenHaloPresentation(onState: (state: HaloPresentation) => void): Promise<() => void> {
  const { emitTo, listen } = await import("@tauri-apps/api/event");
  const ready = () => emitTo("widget", "quota-halo-ready", null);
  const unlistenState = await listen<HaloPresentation>("quota-halo-state", (event) => onState(event.payload));
  let unlistenRequest = () => {};
  try {
    unlistenRequest = await listen("quota-halo-request", () => { void ready().catch(() => {}); });
    await ready();
  } catch (error) {
    unlistenState();
    unlistenRequest();
    throw error;
  }
  return () => { unlistenState(); unlistenRequest(); };
}
