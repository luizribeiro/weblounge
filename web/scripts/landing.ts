import { readChair } from "./chair/graph";
import { makeInteractive } from "./chair/interaction";
import { createPackets } from "./chair/packets";
import { expectElement } from "./dom";

const svg = expectElement(document.getElementById("chair"), SVGSVGElement);
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

if (DEV) {
  new EventSource("/esbuild").addEventListener("change", () =>
    location.reload(),
  );
}
