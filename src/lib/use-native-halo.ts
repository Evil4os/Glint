import { useEffect, useRef, useState } from "react";
import { listenHaloReady, publishHaloPresentation, supportsNativeHalo, type HaloPresentation } from "./halo-bridge";

export function useNativeHalo(presentation: HaloPresentation): boolean {
  const native = supportsNativeHalo();
  const latest = useRef(presentation);
  latest.current = presentation;
  const [connected, setConnected] = useState(false);

  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let cleanup = () => {};
    void listenHaloReady(() => {
      if (disposed) return;
      setConnected(true);
      void publishHaloPresentation(latest.current).catch(() => { if (!disposed) setConnected(false); });
    }).then((unlisten) => {
      if (disposed) unlisten(); else cleanup = unlisten;
    }).catch(() => { if (!disposed) setConnected(false); });
    return () => { disposed = true; cleanup(); };
  }, [native]);

  const { percent, expanded, sidebarDocked } = presentation;
  useEffect(() => {
    if (!native || !connected) return;
    let disposed = false;
    void publishHaloPresentation({ percent, expanded, sidebarDocked }).catch(() => { if (!disposed) setConnected(false); });
    return () => { disposed = true; };
  }, [native, connected, percent, expanded, sidebarDocked]);

  return native && connected && sidebarDocked;
}
