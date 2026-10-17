function differsFromLoaded(form: HTMLFormElement): boolean {
  return [...form.elements].some((element) => {
    if (!(element instanceof HTMLInputElement)) {
      return false;
    }
    return element.type === "checkbox"
      ? element.checked !== element.defaultChecked
      : element.value !== element.defaultValue;
  });
}

function saveOnlyWhenChanged(form: HTMLFormElement) {
  const submit = [...form.elements].find(
    (element): element is HTMLButtonElement =>
      element instanceof HTMLButtonElement && element.type === "submit",
  );
  if (!submit) {
    return;
  }
  const update = () => {
    submit.disabled = !differsFromLoaded(form);
  };
  form.addEventListener("input", update);
  form.addEventListener("change", update);
  // Back and forward restore typed values without firing input events.
  window.addEventListener("pageshow", update);
  update();
}

for (const form of document.querySelectorAll<HTMLFormElement>(
  "form[data-save-when-changed]",
)) {
  saveOnlyWhenChanged(form);
}
