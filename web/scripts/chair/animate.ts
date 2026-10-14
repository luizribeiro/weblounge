import { readChair } from "./graph";
import { makeInteractive } from "./interaction";
import { createPackets } from "./packets";

/** Brings the chair drawing in `svg` to life: draggable nodes, and packets
 * travelling its edges unless the visitor prefers reduced motion. */
export function animateChair(svg: SVGSVGElement): void {
  const media = window.matchMedia("(prefers-reduced-motion: reduce)");
  const chair = readChair(svg);
  const packets = createPackets(svg, chair);
  const motion = makeInteractive(svg, chair, media);

  let previous = performance.now();
  let time = 0;
  function tick(now: number) {
    const dt = Math.min((now - previous) / 1000, 0.08);
    previous = now;
    motion.step(dt);
    if (!media.matches) {
      time += dt;
      packets.move(time);
    }
    requestAnimationFrame(tick);
  }
  requestAnimationFrame(tick);
}
