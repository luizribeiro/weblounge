// A dialog the page opens with is modal; without JavaScript it shows in
// place.
for (const dialog of document.querySelectorAll<HTMLDialogElement>(
  "dialog[data-show-modal][open]",
)) {
  dialog.close();
  dialog.showModal();
}

// Links that name a dialog open it instead; without JavaScript they lead to
// a page asking the same thing.
for (const link of document.querySelectorAll<HTMLAnchorElement>(
  "a[data-dialog]",
)) {
  const dialog = document.getElementById(link.dataset.dialog ?? "");
  if (dialog instanceof HTMLDialogElement) {
    link.addEventListener("click", (event) => {
      event.preventDefault();
      dialog.showModal();
    });
  }
}
