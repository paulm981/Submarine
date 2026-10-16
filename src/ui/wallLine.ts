// What a Wall follows of a line typed to all its terminals (see WallView's
// wallLines): where it ends, as a shell would see it.

export const TYPED_KEPT = 65536;

// Words that open and close a compound command where a command starts
// (`end` closes fish's), and those a command follows.
const OPENS = new Set(["if", "for", "while", "until", "case", "select"]);
const CLOSES = new Set(["fi", "done", "esac", "end"]);
const LEADS = new Set(["if", "while", "until", "then", "do", "else", "elif", "!", "time"]);

/// The shell would ask for more (its `>` prompt) after `text`, which ends
/// with an Enter: a `\` before it; `&&`, `||` or `|` at the end; an open
/// quote, `$(…` or heredoc; or an `if`, `for`, `while`, `case`, `{` or `(`
/// not closed yet. A close reading of what was typed, not a shell — where
/// it's wrong, a line goes on further than it does, so a terminal waits a
/// line longer to join.
export const needsMore = (text: string): boolean => {
  let depth = 0; // compound commands open
  let parens = 0; // of them, ( subshells
  let subst = 0; // $( <( a=( open
  let quote = "";
  let word = "";
  let inWord = false;
  let bare = true; // the word has no quoting, so it can be a keyword
  let cmd = true; // a command starts at the next word
  let op = false; // the last thing was && || |
  let doc: { tabs: boolean } | null = null; // after <<: its end word is next
  const docs: { end: string; tabs: boolean }[] = [];
  const endWord = () => {
    if (!inWord) return;
    const w = word;
    const plain = bare;
    word = "";
    inWord = false;
    bare = true;
    op = false;
    if (doc) {
      docs.push({ end: w, tabs: doc.tabs });
      doc = null;
      return;
    }
    if (subst) return;
    if (plain && w === "{") {
      depth++;
      cmd = true;
      return;
    }
    if (plain && w === "}") {
      depth = Math.max(0, depth - 1);
      cmd = false;
      return;
    }
    if (plain && cmd && OPENS.has(w)) depth++;
    else if (plain && cmd && CLOSES.has(w)) depth = Math.max(0, depth - 1);
    cmd = plain && cmd && LEADS.has(w);
  };
  const n = text.length;
  for (let i = 0; i < n; i++) {
    const ch = text[i];
    const next = text[i + 1];
    if (quote === "'") {
      if (ch === "'") quote = "";
      else word += ch;
      continue;
    }
    if (quote === '"') {
      if (ch === "\\" && i + 1 < n) word += text[++i];
      else if (ch === '"') quote = "";
      else word += ch;
      continue;
    }
    if (ch === "\\") {
      if (next === "\n" && i + 2 >= n) return true;
      if (next !== "\n") {
        word += next ?? "";
        inWord = true;
        bare = false;
      }
      i++;
      continue;
    }
    if (ch === "'" || ch === '"') {
      quote = ch;
      inWord = true;
      bare = false;
    } else if (ch === "#" && !inWord) {
      while (i + 1 < n && text[i + 1] !== "\n") i++;
    } else if (ch === " " || ch === "\t") {
      endWord();
    } else if (ch === "\n") {
      endWord();
      for (const d of docs.splice(0)) {
        for (;;) {
          const end = text.indexOf("\n", i + 1);
          if (end < 0) return true; // its body isn't over
          const body = text.slice(i + 1, end);
          i = end;
          if ((d.tabs ? body.replace(/^\t+/, "") : body) === d.end) break;
        }
      }
      cmd = true;
    } else if (ch === ";") {
      endWord();
      if (next === ";") i++;
      cmd = true;
    } else if (ch === "&") {
      endWord();
      if (next === "&") {
        i++;
        op = true;
        cmd = true;
      } else if (next === ">") i++;
      else cmd = true;
    } else if (ch === "|") {
      endWord();
      if (next === "|" || next === "&") i++;
      op = true;
      cmd = true;
    } else if (ch === "<" || ch === ">") {
      endWord();
      if (next === "(") {
        i++;
        subst++;
        inWord = true;
      } else if (ch === "<" && next === "<") {
        if (text[i + 2] === "<") i += 2; // a here-string
        else {
          i++;
          const tabs = text[i + 1] === "-";
          if (tabs) i++;
          doc = { tabs };
        }
      } else if (next === ch || next === "&" || next === "|") i++;
    } else if (ch === "(") {
      if (next === ")" && !subst) {
        // f() — a function's body comes next
        endWord();
        i++;
        cmd = true;
      } else if (inWord || subst) {
        subst++;
        word += ch;
      } else if (cmd) {
        parens++;
        depth++;
      }
    } else if (ch === ")") {
      if (subst) {
        subst--;
        word += ch;
      } else {
        endWord();
        if (parens) {
          parens--;
          depth = Math.max(0, depth - 1);
          cmd = false;
        } else cmd = true; // a case pattern
      }
    } else {
      word += ch;
      inWord = true;
    }
  }
  return quote !== "" || depth > 0 || subst > 0 || op;
};

// Arrows and the like move about in the line, which isn't followed here.
const ESCAPES = /\x1b(?:\[[0-?]*[ -/]*[@-~]|O.|.)?/gs;

/// What's typed of the line open after `data` was typed (`typed`: what was
/// before, null when no line was open), or null when `data` ends a line:
/// Ctrl+C, or Enter where the shell doesn't ask for more (needsMore).
/// Backspace, Ctrl+U and Ctrl+W edit it as a shell would; Ctrl+L leaves it
/// as it was.
export const lineAfter = (typed: string | null, data: string): string | null => {
  let t: string | null = typed;
  for (const ch of data.replace(ESCAPES, "")) {
    const now: string = t ?? "";
    if (ch === "\x03") t = null;
    else if (ch === "\r" || ch === "\n") t = needsMore(`${now}\n`) ? `${now}\n` : null;
    else if (ch === "\x0c") continue;
    else if (ch === "\x7f" || ch === "\b") t = now.endsWith("\n") ? now : now.slice(0, -1);
    else if (ch === "\x15") t = now.replace(/[^\n]*$/, "");
    else if (ch === "\x17") t = now.replace(/[^\s]*[ \t]*$/, "");
    else if (ch === "\t" || ch >= " ") t = now + ch;
    else t = now;
  }
  return t === null ? null : t.slice(-TYPED_KEPT);
};
