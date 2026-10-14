import type { ChairDrawing, Point } from "./graph";
import { type Hold, limitSpeed, Physics, type Velocity } from "./physics";
import { ChairView } from "./view";

const STEP = 1 / 120;

interface Drag extends Hold, Velocity {
  id: number;
  offsetX: number;
  offsetY: number;
  lastMove: number;
}

interface Pointer extends Point {
  inside: boolean;
}

interface Bounds {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

const clamp = (value: number, min: number, max: number) =>
  Math.max(min, Math.min(max, value));

export function makeInteractive(
  svg: SVGSVGElement,
  chair: ChairDrawing,
  media: MediaQueryList,
): { step: (dt: number) => void } {
  const { layer } = chair;
  const { home } = chair.graph;
  const physics = new Physics(chair.graph);
  const { particles } = physics;
  const view = new ChairView(chair);

  let alive = false;
  let accumulator = 0;
  let cssScale = 1;
  let drag: Drag | null = null;
  let pointer: Pointer = { x: 0, y: 0, inside: false };
  let hoverSuppressed = false;
  let bounds: Bounds = { left: -42, right: 601, top: -16, bottom: 605 };

  function updateScale(ctm: DOMMatrix) {
    cssScale = Math.max(0.05, Math.hypot(ctm.a, ctm.b));
  }

  function updateMetrics() {
    const ctm = layer.getScreenCTM();
    if (!ctm) return;
    updateScale(ctm);
    const coarse = window.matchMedia("(pointer:coarse)").matches;
    view.setHitRadii((coarse ? 20 : 13) / cssScale);
    // Derive local limits from the actual viewBox, not viewport pixels.
    const m = layer.getCTM();
    const s = svg.getCTM();
    if (m && s) {
      const inverse = m.inverse().multiply(s);
      const box = svg.viewBox.baseVal;
      const tl = new DOMPoint(box.x, box.y).matrixTransform(inverse);
      const br = new DOMPoint(
        box.x + box.width,
        box.y + box.height,
      ).matrixTransform(inverse);
      bounds = {
        left: tl.x + 16,
        right: br.x - 16,
        top: tl.y + 16,
        bottom: br.y - 16,
      };
    }
  }
  updateMetrics();

  function localPoint(event: PointerEvent): DOMPoint | null {
    const ctm = layer.getScreenCTM();
    if (!ctm) return null;
    updateScale(ctm);
    const p = new DOMPoint(event.clientX, event.clientY).matrixTransform(
      ctm.inverse(),
    );
    return Number.isFinite(p.x) && Number.isFinite(p.y) ? p : null;
  }

  function nearest(p: Point, touch = false): number {
    let best = -1;
    let distance = (touch ? 23 : 16) / cssScale;
    particles.forEach((n, i) => {
      const d = Math.hypot(n.x - p.x, n.y - p.y);
      if (d < distance) {
        distance = d;
        best = i;
      }
    });
    return best;
  }

  function hover(): Point | null {
    const hovering =
      pointer.inside &&
      !hoverSuppressed &&
      !media.matches &&
      !drag &&
      physics.isHovering(pointer);
    return hovering ? pointer : null;
  }

  function targetFor(held: Drag, p: Point): Point {
    const n = home[held.i];
    let dx = p.x - held.offsetX - n.x;
    let dy = p.y - held.offsetY - n.y;
    const distance = Math.hypot(dx, dy);
    // Direct manipulation for the first 125 SVG units, then a soft limit.
    if (distance > 125) {
      const extra = distance - 125;
      const limited = 125 + extra / (1 + extra / 130);
      dx *= limited / distance;
      dy *= limited / distance;
    }
    return {
      x: clamp(n.x + dx, bounds.left, bounds.right),
      y: clamp(n.y + dy, bounds.top, bounds.bottom),
    };
  }

  function draw() {
    view.draw(particles);
  }

  function restore() {
    physics.reset();
    view.reset();
    alive = false;
    accumulator = 0;
  }

  function step(dt: number) {
    if (media.matches || !alive) return;
    accumulator += clamp(dt, 0, 0.05);
    while (accumulator >= STEP) {
      physics.integrate(STEP, drag, hover());
      accumulator -= STEP;
    }
    draw();
    if (!drag && !hover() && physics.isResting()) restore();
  }

  function clearHover() {
    pointer.inside = false;
  }

  function endDrag(event: PointerEvent | null, cancelled = false) {
    if (!drag || (event && event.pointerId !== drag.id)) return;
    const previous = drag;
    const n = particles[previous.i];
    drag = null;
    svg.removeAttribute("data-grabbing");
    try {
      if (svg.hasPointerCapture(previous.id)) {
        svg.releasePointerCapture(previous.id);
      }
    } catch {}
    // Ignore stale movement when someone holds the chair still, then lets go.
    const recent = !cancelled && performance.now() - previous.lastMove < 85;
    n.vx = recent ? previous.vx * 0.22 : 0;
    n.vy = recent ? previous.vy * 0.22 : 0;
    clearHover();
    hoverSuppressed = true;
    alive = true;
    if (media.matches) restore();
  }

  function down(event: PointerEvent) {
    if (drag || event.button !== 0 || event.isPrimary === false) return;
    const p = localPoint(event);
    if (!p) return;
    const i = nearest(p, event.pointerType === "touch");
    if (i < 0) return;
    const n = particles[i];
    drag = {
      i,
      id: event.pointerId,
      x: n.x,
      y: n.y,
      offsetX: p.x - n.x,
      offsetY: p.y - n.y,
      vx: 0,
      vy: 0,
      lastMove: performance.now(),
    };
    clearHover();
    alive = true;
    svg.setAttribute("data-grabbing", "true");
    try {
      svg.setPointerCapture(event.pointerId);
    } catch {}
    event.preventDefault();
  }

  function move(event: PointerEvent) {
    if (drag && event.pointerId !== drag.id) return;
    const p = localPoint(event);
    if (!p) return;
    if (drag) {
      const target = targetFor(drag, p);
      const now = performance.now();
      const dt = Math.max(0.008, (now - drag.lastMove) / 1000);
      drag.vx = drag.vx * 0.35 + ((target.x - drag.x) / dt) * 0.65;
      drag.vy = drag.vy * 0.35 + ((target.y - drag.y) / dt) * 0.65;
      limitSpeed(drag);
      drag.x = target.x;
      drag.y = target.y;
      drag.lastMove = now;
      // The held point follows the input immediately, not a frame later.
      physics.pin(drag);
      if (media.matches) {
        physics.solveStill(drag);
      }
      draw();
      alive = true;
      event.preventDefault();
    } else if (event.pointerType !== "touch" && event.buttons === 0) {
      hoverSuppressed = false;
      pointer = { x: p.x, y: p.y, inside: true };
      if (hover()) alive = true;
    }
  }

  function cancel() {
    endDrag(null, true);
    clearHover();
    restore();
  }

  // Some engines negotiate touch gestures at the outer SVG rather than
  // the tiny graphics element. Cancel native handling only after a node
  // was actually caught.
  const ownTouch = (event: TouchEvent) => {
    if (drag && event.cancelable) event.preventDefault();
  };
  svg.addEventListener("touchstart", ownTouch, { passive: false });
  svg.addEventListener("touchmove", ownTouch, { passive: false });
  svg.addEventListener("pointerdown", down);
  svg.addEventListener("pointermove", move, { passive: false });
  svg.addEventListener("pointerleave", clearHover);
  svg.addEventListener("pointerup", (event) => endDrag(event));
  svg.addEventListener("pointercancel", (event) => endDrag(event, true));
  svg.addEventListener("lostpointercapture", (event) => endDrag(event, true));
  svg.addEventListener("dragstart", (event) => event.preventDefault());
  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape") endDrag(null, true);
  });
  window.addEventListener("blur", () => {
    endDrag(null, true);
    clearHover();
  });
  const onResize = () => {
    cancel();
    updateMetrics();
  };
  if ("ResizeObserver" in globalThis) new ResizeObserver(onResize).observe(svg);
  else window.addEventListener("resize", onResize, { passive: true });
  media.addEventListener("change", cancel);
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) cancel();
  });

  return { step };
}
