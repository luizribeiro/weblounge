import type { Graph, Point } from "./graph";

export interface Velocity {
  vx: number;
  vy: number;
}

export interface Particle extends Point, Velocity {}

export interface Hold extends Point {
  i: number;
}

const ANCHOR = 34;
const DAMPING = 8.2;
const POINTER_FORCE = 1580;
const HOVER_RADIUS = 83;
const MAX_SPEED = 950;

export function limitSpeed(v: Velocity): void {
  const speed = Math.hypot(v.vx, v.vy);
  if (speed > MAX_SPEED) {
    v.vx *= MAX_SPEED / speed;
    v.vy *= MAX_SPEED / speed;
  }
}

export class Physics {
  readonly particles: Particle[];

  constructor(private readonly graph: Graph) {
    this.particles = graph.home.map((n) => ({ x: n.x, y: n.y, vx: 0, vy: 0 }));
  }

  isHovering(pointer: Point): boolean {
    return this.graph.home.some(
      (n) => Math.hypot(n.x - pointer.x, n.y - pointer.y) < HOVER_RADIUS,
    );
  }

  isResting(): boolean {
    const { home } = this.graph;
    return this.particles.every(
      (n, i) =>
        Math.hypot(n.x - home[i].x, n.y - home[i].y) < 0.025 &&
        Math.hypot(n.vx, n.vy) < 0.1,
    );
  }

  reset(): void {
    this.graph.home.forEach((n, i) => {
      Object.assign(this.particles[i], { x: n.x, y: n.y, vx: 0, vy: 0 });
    });
  }

  pin(hold: Hold | null): void {
    if (!hold) return;
    const n = this.particles[hold.i];
    n.x = hold.x;
    n.y = hold.y;
    n.vx = 0;
    n.vy = 0;
  }

  // With reduced motion, an intentional drag still works. Its neighbours
  // follow a static equilibrium; there is no hover animation or rebound.
  solveStill(hold: Hold | null): void {
    const { home, neighbours } = this.graph;
    this.pin(hold);
    for (let pass = 0; pass < 28; pass++) {
      this.particles.forEach((n, i) => {
        if (hold && i === hold.i) return;
        let x = 0;
        let y = 0;
        let k = ANCHOR;
        for (const link of neighbours[i]) {
          const b = this.particles[link.i];
          const h = home[link.i];
          x += (b.x - h.x) * link.k;
          y += (b.y - h.y) * link.k;
          k += link.k;
        }
        n.x = home[i].x + x / k;
        n.y = home[i].y + y / k;
        n.vx = n.vy = 0;
      });
    }
  }

  integrate(dt: number, hold: Hold | null, hover: Point | null): void {
    this.pin(hold);
    const forces = this.anchorForces(hover);
    this.addSpringForces(forces);
    this.addRepulsion(forces);
    const damping = Math.exp(-DAMPING * dt);
    this.particles.forEach((n, i) => {
      if (hold && i === hold.i) return;
      n.vx = (n.vx + forces[i].x * dt) * damping;
      n.vy = (n.vy + forces[i].y * dt) * damping;
      limitSpeed(n);
      n.x += n.vx * dt;
      n.y += n.vy * dt;
    });
  }

  private anchorForces(hover: Point | null): Point[] {
    const { home } = this.graph;
    return this.particles.map((n, i) => {
      let x = (home[i].x - n.x) * ANCHOR;
      let y = (home[i].y - n.y) * ANCHOR;
      if (hover) {
        // Rest-space sampling keeps the field quiet under a stationary
        // pointer; a dot cannot chase itself out of its own hit target.
        const dx = home[i].x - hover.x;
        const dy = home[i].y - hover.y;
        const distance = Math.hypot(dx, dy);
        if (distance < HOVER_RADIUS) {
          const q = 1 - distance / HOVER_RADIUS;
          const force = (POINTER_FORCE * q * q) / (distance + 14);
          // A tiny stable bias also gives an exact centre hit a response.
          // It is smaller than the invisible grab target, so dots stay easy
          // to catch instead of fleeing the cursor.
          const core = Math.max(0, 1 - distance / 12) ** 2 * 6;
          x += (dx + Math.cos(i * 2.39996323) * core) * force;
          y += (dy + Math.sin(i * 2.39996323) * core) * force;
        }
      }
      return { x, y };
    });
  }

  private addSpringForces(forces: Point[]): void {
    for (const e of this.graph.links) {
      const a = this.particles[e.a];
      const b = this.particles[e.b];
      const x = (b.x - a.x - e.dx) * e.k;
      const y = (b.y - a.y - e.dy) * e.k;
      forces[e.a].x += x;
      forces[e.a].y += y;
      forces[e.b].x -= x;
      forces[e.b].y -= y;
    }
  }

  // Short-range repulsion stops pulled nodes piling into one another.
  // Every rest pair is outside its own exclusion radius, including the
  // deliberately tight joints around the feet.
  private addRepulsion(forces: Point[]): void {
    const { home } = this.graph;
    const { particles } = this;
    for (let i = 0; i < particles.length; i++) {
      for (let j = i + 1; j < particles.length; j++) {
        const a = particles[i];
        const b = particles[j];
        const ha = home[i];
        const hb = home[j];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let distance = Math.hypot(dx, dy);
        const gap =
          Math.min(
            Math.hypot(hb.x - ha.x, hb.y - ha.y) * 0.8,
            ha.r + hb.r + 4,
          ) - distance;
        if (gap <= 0) continue;
        if (distance < 0.01) {
          dx = hb.x - ha.x;
          dy = hb.y - ha.y;
          distance = Math.hypot(dx, dy);
        }
        const f = (gap * 260) / distance;
        const x = dx * f;
        const y = dy * f;
        forces[i].x -= x;
        forces[i].y -= y;
        forces[j].x += x;
        forces[j].y += y;
      }
    }
  }
}
