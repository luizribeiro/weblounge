import { field } from "./dom";

type InviteAnswer =
  | { status: "open"; inviter: string | null }
  | { status: "unusable"; error: string };

/**
 * Fills the invite code from the link's fragment, which no server sees,
 * hides the field, and says who sent the invite or why it won't work.
 */
async function fillCodeFromLink(form: HTMLFormElement) {
  const code = field(form, "code");
  const wrapper = code.closest<HTMLElement>("[data-invite-code]");
  const hint = form.querySelector<HTMLElement>("#code-hint");
  const url = code.dataset.lookup;
  const fromLink = location.hash.slice(1);
  const refused = code.getAttribute("aria-invalid") === "true";
  if (!wrapper || !hint || !url || fromLink === "" || refused) {
    return;
  }
  code.value = fromLink;
  wrapper.hidden = true;

  const response = await fetch(url, {
    method: "POST",
    body: new URLSearchParams({ code: fromLink }),
  }).catch(() => undefined);
  if (!response?.ok) {
    return;
  }
  const answer: InviteAnswer = await response.json();
  if (answer.status === "unusable") {
    hint.textContent = answer.error;
    hint.className = "field-error";
    code.setAttribute("aria-invalid", "true");
    wrapper.hidden = false;
    return;
  }
  const heading = document.querySelector("[data-invite-heading]");
  if (heading && answer.inviter) {
    heading.textContent = `${answer.inviter} saved you a seat.`;
  }
}

const signup = document.querySelector<HTMLFormElement>("form[data-signup]");
if (signup) {
  fillCodeFromLink(signup);
}
