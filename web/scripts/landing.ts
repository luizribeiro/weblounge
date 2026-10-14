import { animateChair } from "./chair/animate";
import { expectElement } from "./dom";

animateChair(expectElement(document.getElementById("chair"), SVGSVGElement));

if (DEV) {
  new EventSource("/esbuild").addEventListener("change", () =>
    location.reload(),
  );
}
