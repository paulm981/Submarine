// Where a line typed to Broadcast's servers ends, as a shell would see it
// (#218): Enter ends it unless the shell would ask for more.

import { describe, expect, it } from "vitest";
import { lineAfter, needsMore, TYPED_KEPT } from "../../src/fork/terminal/broadcastLine";

describe("the shell asks for more", () => {
  it.each([
    ["a backslash before Enter", "ls \\\n"],
    ["&& at the end", "make &&\n"],
    ["|| at the end", "test -f x ||\n"],
    ["| at the end", "ps aux |\n"],
    ["an open single quote", "echo 'a\n"],
    ["an open double quote", 'echo "a\n'],
    ["an open $(", "echo $(date\n"],
    ["an if not closed", "if true; then\n"],
    ["a for not closed", "for f in *; do\n"],
    ["a while not closed", "while read l; do\n"],
    ["a case not closed", "case $x in\n"],
    ["a { not closed", "{ echo a\n"],
    ["a ( not closed", "( cd /tmp\n"],
    ["a function's body to come", "f()\n"],
    ["a function's body to come, named with function", "function f()\n"],
    ["a heredoc whose end word hasn't come", "cat <<EOF\nline\n"],
    ["a <<- heredoc whose end word hasn't come", "cat <<-END\n\tline\n"],
  ])("after %s", (_why, text) => {
    expect(needsMore(text)).toBe(true);
  });

  it.each([
    ["a plain command", "ls -la\n"],
    ["a closed quote", "echo 'a b'\n"],
    ["an escaped quote in double quotes", 'echo "a \\" b"\n'],
    ["a closed if", "if true; then echo y; fi\n"],
    ["a closed for", "for f in *; do echo $f; done\n"],
    ["a closed case, with its patterns", "case $x in a) echo a;; esac\n"],
    ["a closed $(", "echo $(date)\n"],
    ["a closed subshell", "(cd /tmp && ls)\n"],
    ["a closed { }", "{ echo a; }\n"],
    ["a whole function", "f() { echo a; }\n"],
    ["a function whose body is a subshell", "f() (cd /tmp)\n"],
    ["a finished heredoc", "cat <<EOF\nline\nEOF\n"],
    ["a finished <<- heredoc with tabs before its end word", "cat <<-END\n\tline\n\tEND\n"],
    ["a here-string", "cat <<< word\n"],
    ["& running it in the background", "sleep 9 &\n"],
    ["a comment with a quote in it", "ls # don't\n"],
    ["a keyword that's an argument", "echo if for while\n"],
    ["a quoted keyword", "'if' x\n"],
    ["a redirect", "ls > /tmp/x 2>&1\n"],
  ])("not after %s", (_why, text) => {
    expect(needsMore(text)).toBe(false);
  });
});

describe("the line, as it's typed", () => {
  it("opens with the first key and ends at Enter", () => {
    expect(lineAfter(null, "l")).toBe("l");
    expect(lineAfter("l", "s")).toBe("ls");
    expect(lineAfter("ls", "\r")).toBeNull();
  });

  it("an Enter with nothing typed opens no line", () => {
    expect(lineAfter(null, "\r")).toBeNull();
  });

  it("goes on past Enter where the shell asks for more", () => {
    expect(lineAfter("echo 'a", "\r")).toBe("echo 'a\n");
    expect(lineAfter("echo 'a\n", "b'\r")).toBeNull();
  });

  it("ends at Ctrl+C, wherever it is", () => {
    expect(lineAfter("for f in *; do\n", "\x03")).toBeNull();
    expect(lineAfter("abc", "\x03def")).toBe("def");
  });

  it("is edited as a shell would: Backspace, Ctrl+U, Ctrl+W", () => {
    expect(lineAfter("abc", "\x7f")).toBe("ab");
    expect(lineAfter("abc", "\b")).toBe("ab");
    expect(lineAfter("one two", "\x17")).toBe("one ");
    expect(lineAfter("one two  ", "\x17")).toBe("one ");
    expect(lineAfter("echo 'a\nbc", "\x15")).toBe("echo 'a\n");
  });

  it("Backspace doesn't go back past a line the shell already took", () => {
    expect(lineAfter("echo 'a\n", "\x7f")).toBe("echo 'a\n");
  });

  it("leaves it as it was for Ctrl+L, arrows and other escapes, but an arrow opens a line (Up brings back a command)", () => {
    expect(lineAfter("ls", "\x0c")).toBe("ls");
    expect(lineAfter("ls", "\x1b[A")).toBe("ls");
    expect(lineAfter("ls", "\x1bOD")).toBe("ls");
    expect(lineAfter(null, "\x1b[A")).toBe("");
  });

  it("keeps no more than TYPED_KEPT of a long line", () => {
    const t = lineAfter(null, "x".repeat(TYPED_KEPT + 10));
    expect(t?.length).toBe(TYPED_KEPT);
  });
});
