import { field } from "./dom";

const LOOKUP_DELAY_MS = 300;

/** Says whether the nick is free as it's typed; the server decides at signup anyway. */
function enhanceNickLookup(form: HTMLFormElement) {
  const nick = field(form, "nick");
  const status = form.querySelector<HTMLElement>("[data-nick-status]");
  const url = nick.dataset.availability;
  if (!status || !url) {
    return;
  }
  const hint = status.dataset.hint ?? "";
  let timer: number | undefined;
  let latest = 0;

  const show = (text: string, error: boolean) => {
    status.textContent = text;
    status.className = error ? "field-error" : "hint";
    if (error) {
      nick.setAttribute("aria-invalid", "true");
    } else {
      nick.removeAttribute("aria-invalid");
    }
    nick.setCustomValidity(error ? text : "");
  };

  const lookup = async (value: string, request: number) => {
    const response = await fetch(url, {
      method: "POST",
      body: new URLSearchParams({ nick: value }),
    }).catch(() => undefined);
    if (request !== latest || !response?.ok) {
      return;
    }
    const answer: { status: string } = await response.json();
    if (answer.status === "available") {
      show(`${value} is free.`, false);
    } else if (answer.status === "taken") {
      show(`${value} is taken.`, true);
    }
  };

  nick.addEventListener("input", () => {
    clearTimeout(timer);
    latest += 1;
    show(hint, false);
    if (nick.value === "" || !nick.checkValidity()) {
      return;
    }
    const request = latest;
    timer = window.setTimeout(
      () => lookup(nick.value, request),
      LOOKUP_DELAY_MS,
    );
  });
}

const signup = document.querySelector<HTMLFormElement>("form[data-signup]");
if (signup) {
  enhanceNickLookup(signup);
}
