import { useEffect, useRef, useState } from "react";
import { listenHaloPresentation, type HaloPresentation } from "../lib/halo-bridge";
import { useQuotaPalette } from "../lib/use-quota-palette";

export function QuotaHalo({ state }: { state: HaloPresentation }) {
  const root = useRef<HTMLDivElement>(null);
  useQuotaPalette(root, state.percent);
  return <main className="quota-halo-surface" aria-hidden="true">
    <div ref={root} className={`quota-widget quota-halo-core quota-widget--bottom-left is-docked${state.expanded ? " is-expanded" : ""}`}>
      <div className="quota-widget-panel"><div className="quota-widget-cloud"><div className="quota-widget-backdrop" /></div></div>
    </div>
  </main>;
}

export function QuotaHaloSurface() {
  const [state, setState] = useState<HaloPresentation | null>(null);
  useEffect(() => {
    let disposed = false;
    let cleanup = () => {};
    void listenHaloPresentation((value) => { if (!disposed) setState(value); }).then((unlisten) => {
      if (disposed) unlisten(); else cleanup = unlisten;
    }).catch(() => {});
    return () => { disposed = true; cleanup(); };
  }, []);
  return state?.sidebarDocked ? <QuotaHalo state={state} /> : null;
}
