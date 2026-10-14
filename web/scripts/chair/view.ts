import { svgElement } from "../dom";
import type { ChairDrawing, Link, Point } from "./graph";

const round = (n: number) => Math.round(n * 1000) / 1000;

function linkPath(e: Link, a: Point, b: Point): string {
  let d = `M${round(a.x)},${round(a.y)} ${e.kind}`;
  if (e.controls.length) {
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const length = Math.hypot(dx, dy);
    const nx = length > 0.001 ? -dy / length : 0;
    const ny = length > 0.001 ? dx / length : 0;
    const bow = Math.max(0, Math.min(1.25, Math.sqrt(length / e.length)));
    d += `${e.controls
      .map(
        (c) =>
          `${round(a.x + c.t * dx + nx * c.normal * bow)},${round(a.y + c.t * dy + ny * c.normal * bow)}`,
      )
      .join(" ")} `;
  }
  return `${d}${round(b.x)},${round(b.y)}`;
}

export class ChairView {
  private readonly hitEls: SVGCircleElement[];
  private readonly originalPaths: string[];
  private drawn = false;

  constructor(private readonly chair: ChairDrawing) {
    const { home, edges } = chair.graph;
    this.originalPaths = edges.map((e) => e.d);
    const hits = svgElement("g", { "aria-hidden": "true" });
    this.hitEls = home.map((n) =>
      hits.appendChild(
        svgElement("circle", { cx: n.x, cy: n.y, r: 16, class: "node-target" }),
      ),
    );
    chair.layer.appendChild(hits);
  }

  setHitRadii(minimum: number): void {
    this.chair.graph.home.forEach((n, i) => {
      this.hitEls[i].setAttribute("r", String(Math.max(n.r + 4, minimum)));
    });
  }

  draw(particles: Point[]): void {
    particles.forEach((n, i) => {
      this.place(i, round(n.x), round(n.y));
    });
    this.chair.graph.links.forEach((e, i) => {
      const path = linkPath(e, particles[e.a], particles[e.b]);
      this.chair.edgeEls[i].setAttribute("d", path);
    });
    this.drawn = true;
  }

  reset(): void {
    if (!this.drawn) return;
    this.chair.graph.home.forEach((n, i) => {
      this.place(i, n.x, n.y);
    });
    this.chair.edgeEls.forEach((p, i) => {
      p.setAttribute("d", this.originalPaths[i]);
    });
    this.drawn = false;
  }

  private place(i: number, x: number, y: number): void {
    for (const el of [this.chair.nodeEls[i], this.hitEls[i]]) {
      el.setAttribute("cx", String(x));
      el.setAttribute("cy", String(y));
    }
  }
}
