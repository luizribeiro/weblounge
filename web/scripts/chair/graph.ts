import { expectElement } from "../dom";

export interface Point {
  x: number;
  y: number;
}

export interface Vertex extends Point {
  r: number;
}

export interface Edge {
  d: string;
  a: number;
  b: number;
}

export interface Control {
  t: number;
  normal: number;
}

export interface Link {
  a: number;
  b: number;
  dx: number;
  dy: number;
  length: number;
  kind: "C" | "Q" | "L";
  controls: Control[];
  k: number;
}

export interface Neighbour {
  i: number;
  k: number;
}

export interface Graph {
  home: Vertex[];
  edges: Edge[];
  links: Link[];
  neighbours: Neighbour[][];
}

export interface ChairDrawing {
  layer: SVGGElement;
  nodeEls: SVGCircleElement[];
  edgeEls: SVGPathElement[];
  graph: Graph;
}

export const numbersIn = (d: string): number[] =>
  (d.match(/-?\d*\.?\d+(?:e[-+]?\d+)?/gi) ?? []).map(Number);

export function readChair(svg: SVGSVGElement): ChairDrawing {
  const nodeEls = [
    ...svg.querySelectorAll<SVGCircleElement>(".chair-nodes > circle"),
  ];
  const edgeEls = [
    ...svg.querySelectorAll<SVGPathElement>(".chair-edges > path"),
  ];
  const nodes = expectElement(svg.querySelector(".chair-nodes"), SVGGElement);
  const layer = expectElement(nodes.parentElement, SVGGElement);
  return { layer, nodeEls, edgeEls, graph: readGraph(nodeEls, edgeEls) };
}

// The drawing is the only copy of the geometry: every edge starts and ends
// exactly on a node, which is how edges learn which nodes they connect.
function readGraph(
  nodeEls: SVGCircleElement[],
  edgeEls: SVGPathElement[],
): Graph {
  const home = nodeEls.map((c) => ({
    x: Number(c.getAttribute("cx")),
    y: Number(c.getAttribute("cy")),
    r: Number(c.getAttribute("r")),
  }));
  const nodeAt = (x: number, y: number) =>
    home.findIndex((n) => n.x === x && n.y === y);
  const edges = edgeEls.map((path) => {
    const d = path.getAttribute("d") ?? "";
    const v = numbersIn(d);
    return {
      d,
      a: nodeAt(v[0], v[1]),
      b: nodeAt(v[v.length - 2], v[v.length - 1]),
    };
  });
  return { home, edges, ...springs(home, edges) };
}

// Springs resist changes to the rest vectors, rather than trying to turn
// the drawing into a generic force-directed blob. Curved outlines retain
// their original bow, even when their endpoints turn and stretch.
function springs(home: Vertex[], edges: Edge[]) {
  const neighbours: Neighbour[][] = home.map(() => []);
  const degree = home.map(() => 0);
  for (const e of edges) {
    degree[e.a]++;
    degree[e.b]++;
  }
  const links = edges.map((e): Link => {
    const a = home[e.a];
    const b = home[e.b];
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const length = Math.hypot(dx, dy);
    const values = numbersIn(e.d);
    const kind = e.d.includes("C") ? "C" : e.d.includes("Q") ? "Q" : "L";
    const controls: Control[] = [];
    for (let j = 2; j < values.length - 2; j += 2) {
      const cx = values[j] - a.x;
      const cy = values[j + 1] - a.y;
      controls.push({
        t: (cx * dx + cy * dy) / (length * length),
        normal: (dx * cy - dy * cx) / length,
      });
    }
    const k = 70 / Math.sqrt(Math.max(degree[e.a], degree[e.b]));
    neighbours[e.a].push({ i: e.b, k });
    neighbours[e.b].push({ i: e.a, k });
    return { a: e.a, b: e.b, dx, dy, length, kind, controls, k };
  });
  return { links, neighbours };
}
