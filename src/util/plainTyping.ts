// What's typed into the app's own fields stays as typed. Server names,
// hosts, users, paths and commands aren't sentences, but a Mac's text
// features (System Settings › Keyboard: capitalize words, correct spelling,
// add a period with a double space) and Android's keyboard would change
// them — "ai" becoming "Ai" — in any field that doesn't say otherwise. The
// terminal opts out on its own (xterm.js); this does it for every other
// text field as it's focused, unless the field asks for `spellCheck`.

// `type` reads "text" when it's left out.
const TEXT_TYPES = new Set(["text", "search", "url", "email", "password", "tel"]);

const isTextField = (el: Element): el is HTMLInputElement | HTMLTextAreaElement =>
  el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && TEXT_TYPES.has(el.type));

export const keepTypingPlain = () => {
  document.addEventListener(
    "focusin",
    (e) => {
      const el = e.target;
      if (!(el instanceof Element) || !isTextField(el) || el.getAttribute("spellcheck") === "true") return;
      el.setAttribute("spellcheck", "false");
      el.setAttribute("autocorrect", "off");
      el.setAttribute("autocapitalize", "off");
    },
    true,
  );
};
