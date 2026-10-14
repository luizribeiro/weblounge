import "./password";
import "./signup";

const FEEDBACK_MS = 1500;

const copyStatus = document.querySelector("[data-copy-status]");

async function copy(button: HTMLButtonElement) {
  const label = button.textContent;
  try {
    await navigator.clipboard.writeText(button.dataset.copy ?? "");
    button.textContent = "Copied";
  } catch {
    button.textContent = "Can't copy";
  }
  if (copyStatus) {
    copyStatus.textContent = "";
    copyStatus.textContent = `${button.textContent}: ${button.dataset.copyLabel ?? button.dataset.copy}`;
  }
  setTimeout(() => {
    button.textContent = label;
  }, FEEDBACK_MS);
}

if (navigator.clipboard) {
  for (const button of document.querySelectorAll<HTMLButtonElement>(
    "button[data-copy]",
  )) {
    button.hidden = false;
    button.addEventListener("click", () => copy(button));
  }
}
