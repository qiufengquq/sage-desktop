import { useEffect, useRef, useState } from "react";

/**
 * 观测容器宽度，供手绘 SVG 图表使用。
 * 用实测像素作为 viewBox 宽度，可保证 1:1 映射，文字不会被拉伸或缩得过小。
 */
export function useElementWidth<T extends HTMLElement>(fallback = 640) {
  const ref = useRef<T | null>(null);
  const [width, setWidth] = useState(fallback);

  useEffect(() => {
    const element = ref.current;
    if (!element) return undefined;

    const observer = new ResizeObserver((entries) => {
      const next = entries[0]?.contentRect.width;
      if (next && Number.isFinite(next)) setWidth(Math.max(260, Math.round(next)));
    });

    observer.observe(element);
    setWidth(Math.max(260, Math.round(element.getBoundingClientRect().width || fallback)));

    return () => observer.disconnect();
  }, [fallback]);

  return { ref, width };
}
