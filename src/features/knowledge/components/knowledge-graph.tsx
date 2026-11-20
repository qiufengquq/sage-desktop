import { useEffect, useMemo, useRef, useState } from "react";
import type { KnowledgeEdge, KnowledgeNode } from "@/types";
import { cn } from "@/lib/utils";

interface KnowledgeGraphProps {
  nodes: KnowledgeNode[];
  edges: KnowledgeEdge[];
  selectedId: string | null;
  matchedIds: Set<string>;
  onSelect: (id: string | null) => void;
  className?: string;
}

interface Point {
  x: number;
  y: number;
}

const WIDTH = 900;
const HEIGHT = 560;

const KIND_COLOR: Record<string, string> = {
  word: "var(--brand)",
  phrase: "var(--ember)",
  grammar: "var(--success)",
  sentence: "var(--info)",
};

const KIND_LABEL: Record<string, string> = {
  word: "单词",
  phrase: "词组",
  grammar: "语法",
  sentence: "句子",
};

/** 轻量力导向布局：只做局部松弛，初始位置来自数据层，保证稳定 */
function relax(
  nodes: KnowledgeNode[],
  edges: KnowledgeEdge[],
  fixed?: Map<string, Point>,
): Map<string, Point> {
  const positions = new Map<string, Point>();
  nodes.forEach((node) => {
    positions.set(node.id, fixed?.get(node.id) ?? { x: node.x * WIDTH, y: node.y * HEIGHT });
  });

  const adjacency = new Map<string, string[]>();
  edges.forEach((edge) => {
    adjacency.set(edge.source, [...(adjacency.get(edge.source) ?? []), edge.target]);
    adjacency.set(edge.target, [...(adjacency.get(edge.target) ?? []), edge.source]);
  });

  const iterations = fixed ? 0 : 160;
  for (let step = 0; step < iterations; step += 1) {
    const alpha = 1 - step / iterations;

    for (let i = 0; i < nodes.length; i += 1) {
      const a = positions.get(nodes[i].id) as Point;
      for (let j = i + 1; j < nodes.length; j += 1) {
        const b = positions.get(nodes[j].id) as Point;
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        const dist2 = dx * dx + dy * dy || 0.01;
        const dist = Math.sqrt(dist2);
        if (dist < 140) {
          const force = (46 * alpha) / dist2;
          dx *= force;
          dy *= force;
          a.x += dx;
          a.y += dy;
          b.x -= dx;
          b.y -= dy;
        }
      }
    }

    edges.forEach((edge) => {
      const a = positions.get(edge.source);
      const b = positions.get(edge.target);
      if (!a || !b) return;
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const dist = Math.hypot(dx, dy) || 1;
      const target = 120;
      const force = ((dist - target) / dist) * 0.045 * alpha;
      a.x += dx * force;
      a.y += dy * force;
      b.x -= dx * force;
      b.y -= dy * force;
    });

    positions.forEach((point) => {
      point.x += (WIDTH / 2 - point.x) * 0.008 * alpha;
      point.y += (HEIGHT / 2 - point.y) * 0.008 * alpha;
      point.x = Math.min(WIDTH - 40, Math.max(40, point.x));
      point.y = Math.min(HEIGHT - 40, Math.max(40, point.y));
    });
  }

  return positions;
}

export function KnowledgeGraph({
  nodes,
  edges,
  selectedId,
  matchedIds,
  onSelect,
  className,
}: KnowledgeGraphProps) {
  const [positions, setPositions] = useState<Map<string, Point>>(() =>
    relax(nodes, edges),
  );
  const [view, setView] = useState({ scale: 1, x: 0, y: 0 });
  const [dragging, setDragging] = useState<string | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const panRef = useRef<{ x: number; y: number; startX: number; startY: number } | null>(null);

  useEffect(() => {
    setPositions(relax(nodes, edges));
  }, [nodes, edges]);

  const neighbors = useMemo(() => {
    const set = new Set<string>();
    if (!selectedId) return set;
    edges.forEach((edge) => {
      if (edge.source === selectedId) set.add(edge.target);
      if (edge.target === selectedId) set.add(edge.source);
    });
    return set;
  }, [edges, selectedId]);

  const focusId = selectedId ?? hovered;

  const toLocal = (clientX: number, clientY: number) => {
    const rect = svgRef.current?.getBoundingClientRect();
    if (!rect) return { x: 0, y: 0 };
    return {
      x: ((clientX - rect.left) / rect.width) * WIDTH,
      y: ((clientY - rect.top) / rect.height) * HEIGHT,
    };
  };

  const handleWheel = (event: React.WheelEvent<SVGSVGElement>) => {
    event.preventDefault();
    setView((prev) => {
      const next = Math.min(2.4, Math.max(0.55, prev.scale - event.deltaY * 0.0012));
      return { ...prev, scale: next };
    });
  };

  return (
    <div className={cn("relative overflow-hidden rounded-md border border-line bg-surface", className)}>
      <svg
        ref={svgRef}
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        className="h-full w-full touch-none"
        onWheel={handleWheel}
        onPointerDown={(event) => {
          if (event.target === svgRef.current) {
            panRef.current = {
              x: view.x,
              y: view.y,
              startX: event.clientX,
              startY: event.clientY,
            };
            onSelect(null);
          }
        }}
        onPointerMove={(event) => {
          if (dragging) {
            const p = toLocal(event.clientX, event.clientY);
            setPositions((prev) => new Map(prev).set(dragging, p));
            return;
          }
          if (panRef.current) {
            const dx = event.clientX - panRef.current.startX;
            const dy = event.clientY - panRef.current.startY;
            setView((prev) => ({ ...prev, x: panRef.current!.x + dx, y: panRef.current!.y + dy }));
          }
        }}
        onPointerUp={() => {
          setDragging(null);
          panRef.current = null;
        }}
        onPointerLeave={() => {
          setDragging(null);
          panRef.current = null;
        }}
        role="img"
        aria-label="知识图谱，可拖拽节点与缩放"
      >
        <g transform={`translate(${view.x} ${view.y}) scale(${view.scale})`}>
          {edges.map((edge) => {
            const a = positions.get(edge.source);
            const b = positions.get(edge.target);
            if (!a || !b) return null;
            const active =
              focusId !== null && (edge.source === focusId || edge.target === focusId);
            return (
              <line
                key={edge.id}
                x1={a.x}
                y1={a.y}
                x2={b.x}
                y2={b.y}
                stroke={active ? "var(--brand)" : "var(--line-strong)"}
                strokeWidth={active ? 1.4 : 1}
                strokeOpacity={focusId && !active ? 0.28 : active ? 0.75 : 0.5}
                strokeDasharray={edge.relation === "explains" ? "3 3" : undefined}
              />
            );
          })}

          {nodes.map((node) => {
            const point = positions.get(node.id);
            if (!point) return null;
            const radius = 4 + node.weight * 3.4;
            const isSelected = node.id === selectedId;
            const isNeighbor = neighbors.has(node.id);
            const dim = focusId !== null && !isSelected && !isNeighbor;
            const matched = matchedIds.has(node.id);
            const showLabel =
              isSelected || isNeighbor || node.kind !== "word" || view.scale > 1.15 || hovered === node.id;

            return (
              <g
                key={node.id}
                transform={`translate(${point.x} ${point.y})`}
                className="cursor-pointer"
                onPointerDown={(event) => {
                  event.stopPropagation();
                  setDragging(node.id);
                }}
                onClick={(event) => {
                  event.stopPropagation();
                  onSelect(node.id);
                }}
                onMouseEnter={() => setHovered(node.id)}
                onMouseLeave={() => setHovered(null)}
                opacity={dim ? 0.32 : 1}
              >
                {isSelected && (
                  <circle r={radius + 5} fill="none" stroke={KIND_COLOR[node.kind]} strokeWidth={1} strokeOpacity={0.4} />
                )}
                <circle
                  r={radius}
                  fill={KIND_COLOR[node.kind]}
                  fillOpacity={matched ? 0.9 : 0.45}
                  stroke={isSelected ? "var(--surface)" : "transparent"}
                  strokeWidth={2}
                />
                {showLabel && (
                  <text
                    y={radius + 10}
                    textAnchor="middle"
                    className="pointer-events-none"
                    style={{
                      fontSize: 9,
                      fill: "var(--ink)",
                      fontFamily: node.kind === "word" ? "var(--font-display)" : "var(--font-sans)",
                    }}
                  >
                    {node.label.length > 16 ? `${node.label.slice(0, 15)}…` : node.label}
                  </text>
                )}
              </g>
            );
          })}
        </g>
      </svg>

      <div className="pointer-events-none absolute bottom-2.5 left-3 flex flex-wrap items-center gap-3">
        {Object.entries(KIND_LABEL).map(([kind, label]) => (
          <span key={kind} className="flex items-center gap-1 text-[10px] text-ink-muted">
            <span
              className="size-1.5 rounded-full"
              style={{ backgroundColor: KIND_COLOR[kind] }}
              aria-hidden
            />
            {label}
          </span>
        ))}
      </div>

      <div className="pointer-events-none absolute top-2.5 right-3 font-mono text-[10px] text-ink-muted">
        {nodes.length} 节点 · {edges.length} 关联 · {Math.round(view.scale * 100)}%
      </div>
    </div>
  );
}
