import { useLayoutEffect, useRef, type RefObject } from "react";
import { getQuotaColors, interpolateQuotaColors, type QuotaColors } from "./quota-colors";

export function useQuotaPalette(element: RefObject<HTMLElement | null>, percent: number | null) {
  const current = useRef<QuotaColors | null>(null);

  useLayoutEffect(() => {
    const node = element.current;
    if (!node) return;
    const target = getQuotaColors(percent);
    let animation = 0;
    const paint = (colors: QuotaColors) => {
      current.current = colors;
      node.style.setProperty("--quota-a", `light-dark(${colors.a}, ${colors.darkA})`);
      node.style.setProperty("--quota-bridge", `light-dark(${colors.bridge}, ${colors.darkBridge})`);
      node.style.setProperty("--quota-b", `light-dark(${colors.b}, ${colors.darkB})`);
    };
    if (!current.current || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      paint(target);
      return;
    }
    const from = current.current;
    const start = performance.now();
    const tick = (now: number) => {
      const t = Math.max(0, Math.min(1, (now - start) / 450));
      paint(interpolateQuotaColors(from, target, 1 - (1 - t) ** 3));
      if (t < 1) animation = window.requestAnimationFrame(tick);
    };
    animation = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(animation);
  }, [element, percent]);
}
