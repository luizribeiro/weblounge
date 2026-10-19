import { expectElement } from "./dom";

/**
 * Fills the code from the link's fragment, which no server sees, and
 * hides the field, leaving the form's button. A link cut short, as mail
 * apps sometimes wrap them, leaves the field showing to say so. A code
 * the server filled back in, as a form it refused does, hides it too.
 */
function fillCodeFromLink(wrapper: HTMLElement) {
  const code = expectElement(wrapper.querySelector("input"), HTMLInputElement);
  const hint = wrapper.querySelector<HTMLElement>("#code-hint");
  const fromLink = location.hash.slice(1);
  if (fromLink !== "") {
    code.value = fromLink;
  }
  if (!hint || code.value === "") {
    return;
  }
  if (code.checkValidity()) {
    wrapper.hidden = true;
    return;
  }
  hint.textContent = hint.dataset.cutShort ?? hint.textContent;
  hint.className = "field-error";
  code.setAttribute("aria-invalid", "true");
}

for (const wrapper of document.querySelectorAll<HTMLElement>(
  "[data-link-code]",
)) {
  fillCodeFromLink(wrapper);
}
