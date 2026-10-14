const SVG_NS = "http://www.w3.org/2000/svg";

export function expectElement<T extends Element>(
  element: Element | null,
  type: abstract new () => T,
): T {
  if (!(element instanceof type)) {
    throw new Error(`expected a ${type.name}`);
  }
  return element;
}

export function svgElement<K extends keyof SVGElementTagNameMap>(
  name: K,
  attributes: Record<string, string | number> = {},
): SVGElementTagNameMap[K] {
  const element = document.createElementNS(SVG_NS, name);
  for (const [key, value] of Object.entries(attributes)) {
    element.setAttribute(key, String(value));
  }
  return element;
}

export function field(form: HTMLFormElement, name: string): HTMLInputElement {
  return expectElement(
    form.querySelector(`input[name="${name}"]`),
    HTMLInputElement,
  );
}
