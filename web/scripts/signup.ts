import { expectElement } from "./dom";

/** What concierge serves at the checklist's data-password-rules URL. */
interface PasswordRules {
  minLength: number;
  maxBytes: number;
  common: string[];
}

function field(form: HTMLFormElement, name: string): HTMLInputElement {
  return expectElement(
    form.querySelector(`input[name="${name}"]`),
    HTMLInputElement,
  );
}

type Check = (password: string, confirmation: string) => boolean;

const encoder = new TextEncoder();

/** One check per checklist line, by its data-rule, mirroring password_rules.rs. */
function checksFor(rules: PasswordRules): Record<string, Check> {
  const common = new Set(rules.common);
  return {
    length: (password) =>
      [...password].length >= rules.minLength &&
      encoder.encode(password).length <= rules.maxBytes,
    "no-whitespace": (password) => !/\p{White_Space}/u.test(password),
    "no-leading-colon": (password) => !password.startsWith(":"),
    "not-common": (password) => !common.has(password.toLowerCase()),
    matches: (password, confirmation) => password === confirmation,
  };
}

async function enhancePasswordChecklist(form: HTMLFormElement) {
  const list = form.querySelector<HTMLElement>("[data-password-rules]");
  const password = field(form, "password");
  const confirmation = field(form, "confirmation");
  if (!list?.dataset.passwordRules) {
    return;
  }
  const response = await fetch(list.dataset.passwordRules);
  if (!response.ok) {
    return;
  }
  const checks = checksFor(await response.json());
  const lines = [...list.querySelectorAll<HTMLElement>("[data-rule]")];

  const update = () => {
    let unmet: string | undefined;
    for (const line of lines) {
      const check = checks[line.dataset.rule ?? ""];
      const met = check?.(password.value, confirmation.value) ?? true;
      if (password.value === "") {
        delete line.dataset.met;
      } else {
        line.dataset.met = String(met);
      }
      if (!met && unmet === undefined && line.dataset.rule !== "matches") {
        unmet = line.textContent ?? "";
      }
    }
    password.setCustomValidity(unmet ? `Needs: ${unmet}` : "");
    confirmation.setCustomValidity(
      checks.matches(password.value, confirmation.value)
        ? ""
        : "The passwords don't match.",
    );
  };
  password.addEventListener("input", update);
  confirmation.addEventListener("input", update);
  update();
}

function enhanceReveal(form: HTMLFormElement) {
  const button = form.querySelector<HTMLButtonElement>("button[data-reveal]");
  if (!button) {
    return;
  }
  const inputs = [
    ...form.querySelectorAll<HTMLInputElement>(
      'input[autocomplete="new-password"]',
    ),
  ];
  button.hidden = false;
  button.setAttribute("aria-pressed", "false");
  button.addEventListener("click", () => {
    const show = button.getAttribute("aria-pressed") !== "true";
    for (const input of inputs) {
      input.type = show ? "text" : "password";
    }
    button.setAttribute("aria-pressed", String(show));
    button.textContent = show ? "Hide" : "Show";
  });
}

const signup = document.querySelector<HTMLFormElement>("form[data-signup]");
if (signup) {
  enhanceReveal(signup);
  enhancePasswordChecklist(signup);
}
