import { expectElement, svgElement } from "../dom";
import type { ChairDrawing } from "./graph";

const ROUTES = [
  [50, 40, 36, 31, 26, 23, 9, 6, 3, 0],
  [52, 41, 34, 10, 8, 6, 4, 2],
  [48, 39, 38, 28, 21, 18, 17, 16, 23, 14, 6, 1],
];
const LAP_SECONDS = 11;
const SPACING = 0.29;

interface Segment {
  edge: number;
  start: number;
  length: number;
  forward: boolean;
}

interface Route {
  total: number;
  segments: Segment[];
}

const mod = (value: number, base: number) => ((value % base) + base) % base;

function planRoute(
  { graph: { edges } }: ChairDrawing,
  measurements: number[],
  nodes: number[],
): Route {
  let total = 0;
  const segments = nodes.slice(0, -1).map((a, index) => {
    const b = nodes[index + 1];
    const edge = edges.findIndex(
      (e) => (e.a === a && e.b === b) || (e.a === b && e.b === a),
    );
    const segment = {
      edge,
      start: total,
      length: measurements[edge],
      forward: edges[edge].a === a,
    };
    total += segment.length;
    return segment;
  });
  return { total, segments };
}

// Packets ride the live, possibly stretched paths, so lengths are re-read.
function routePosition(
  edgeEls: SVGPathElement[],
  route: Route,
  fraction: number,
): DOMPoint {
  const target = Math.max(0, Math.min(0.999999, fraction)) * route.total;
  let segment = route.segments[route.segments.length - 1];
  for (const s of route.segments) {
    if (target < s.start + s.length) {
      segment = s;
      break;
    }
  }
  let f = (target - segment.start) / segment.length;
  if (!segment.forward) f = 1 - f;
  const path = edgeEls[segment.edge];
  return path.getPointAtLength(f * path.getTotalLength());
}

export function createPackets(
  svg: SVGSVGElement,
  chair: ChairDrawing,
): { move: (time: number) => void } {
  const measurements = chair.edgeEls.map((path) => path.getTotalLength());
  const routes = ROUTES.map((nodes) => planRoute(chair, measurements, nodes));
  const layer = expectElement(svg.querySelector(".packets"), SVGGElement);
  const packets = routes.map(() => {
    const halo = svgElement("circle", {
      r: "8",
      fill: "none",
      stroke: "currentColor",
      "stroke-width": ".7",
      opacity: ".16",
    });
    const dot = svgElement("circle", { r: "3.4", opacity: ".95" });
    layer.append(halo, dot);
    return [halo, dot];
  });

  function move(time: number) {
    packets.forEach((circles, i) => {
      const p = routePosition(
        chair.edgeEls,
        routes[i],
        mod(time / LAP_SECONDS + i * SPACING, 1),
      );
      for (const circle of circles) {
        circle.setAttribute("cx", String(p.x));
        circle.setAttribute("cy", String(p.y));
      }
    });
  }

  return { move };
}
