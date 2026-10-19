/**
 * Shows a field, and requires it, only while the input it guards differs
 * from what was saved, which is when the server asks for it too.
 */
function showWhileChanged(field: HTMLElement) {
  const form = field.closest("form");
  const input = field.querySelector("input");
  const name = field.dataset.neededToChange;
  const guarded = form?.querySelector<HTMLInputElement>(
    `input[name="${name}"]`,
  );
  if (!input || !guarded) {
    return;
  }
  const saved = field.dataset.saved ?? guarded.defaultValue;
  const update = () => {
    const changed = guarded.value.trim() !== saved;
    field.hidden = !changed;
    input.required = changed;
  };
  guarded.addEventListener("input", update);
  // Back and forward restore typed values without firing input events.
  window.addEventListener("pageshow", update);
  update();
}

for (const field of document.querySelectorAll<HTMLElement>(
  "[data-needed-to-change]",
)) {
  showWhileChanged(field);
}
