import { field } from "./dom";

/**
 * Fills the code from the link's fragment, which no server sees, and
 * hides the field, leaving the Confirm button. A link cut short, as mail
 * apps sometimes wrap them, leaves the field showing to say so.
 */
function fillCodeFromLink(form: HTMLFormElement) {
  const code = field(form, "code");
  const wrapper = code.closest<HTMLElement>("[data-link-code]");
  const hint = form.querySelector<HTMLElement>("#code-hint");
  const fromLink = location.hash.slice(1);
  if (!wrapper || !hint || fromLink === "") {
    return;
  }
  code.value = fromLink;
  if (code.checkValidity()) {
    wrapper.hidden = true;
    return;
  }
  hint.textContent = hint.dataset.cutShort ?? hint.textContent;
  hint.className = "field-error";
  code.setAttribute("aria-invalid", "true");
}

const verify = document.querySelector<HTMLFormElement>("form[data-verify]");
if (verify) {
  fillCodeFromLink(verify);
}
