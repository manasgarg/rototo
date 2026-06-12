/* Focus a screen's primary control on load — but never on small screens,
   where autofocus pops the keyboard, and never when something else already
   holds focus. */
export function shouldAutoFocus(): boolean {
  return (
    typeof window !== "undefined" &&
    !window.matchMedia("(max-width: 880px)").matches &&
    (document.activeElement === null || document.activeElement === document.body)
  );
}
