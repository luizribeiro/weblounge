const LOOKUP_DELAY_MS = 300;

/** Says whether a nick, a member's or a bot's, is free as it's typed; the server decides as the form is sent anyway. */
function enhanceNickLookup(nick: HTMLInputElement) {
  const status = nick.form?.querySelector<HTMLElement>("[data-nick-status]");
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

for (const nick of document.querySelectorAll<HTMLInputElement>(
  "input[data-availability]",
)) {
  enhanceNickLookup(nick);
}
