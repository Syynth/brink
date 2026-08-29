# brink debug

Step through a story: breakpoints, stepping, locals, and the call stack, from the terminal.

```sh
brink debug [--script <FILE>] <FILE>
```

Accepts raw `.ink` or `.brink` source, or a compiled story (`.inkb`, `.inkt`). Both source surfaces are debuggable.

Source entries are **compiled with debug info automatically** — you do not pass `brink compile`'s `--debug-info` flag here. Without that section there is nothing to map a bytecode position back to a line, so breakpoints could not bind and stepping could not tell when it had crossed a line; a debugger that offered to run without it would only be offering a debugger that does not work.

A *prebuilt* `.inkb`/`.inkt` is taken as it is, since whether it carries the section was decided when it was built. One built without `--debug-info` still runs, it just cannot say where it is: breakpoints refuse to bind (and say why), and stepping reports `<no source position>`.

## Verbs

| Verb | Meaning |
|------|---------|
| `break <file>:<line>` | Arm a breakpoint. Lines are 1-based, as your editor shows them. |
| `run`, `continue` | Advance until a breakpoint, a choice, or the story ends. |
| `step into\|over\|out` | Advance one **source line**. `next` is `step over`. |
| `stepi into\|over\|out` | Advance one **VM instruction**. |
| `locals` | Named locals in the innermost frame. |
| `stack` | The call stack, innermost first. |
| `list`, `l` | Source around the current line, with the stopped line marked. *(interactive only)* |
| `help`, `?` / `quit`, `q` | *(interactive only)* |

The two granularities are deliberate: `step` is what an author wants, `stepi` is what you want when you are reading the compiled `.inkt` beside the source and need to see a single line's worth of bytecode go by.

`step out` in the outermost frame reports `nostepouttarget` and stays where it is: there is no caller to return to, so the honest answer is to refuse rather than to run somewhere and call it a return.

A breakpoint that cannot bind is an **error**, not a silent no-op: a breakpoint you believe is armed and that can never hit is worse than no breakpoint at all.

## Interactive

```text
$ brink debug story.ink
brink debug — `help` for verbs, `quit` to leave
(brink) break story.ink:7
(brink) run -> breakpoint story.ink:7
  at story.ink:7
(brink) list
      4
      5 === start ===
      6 ~ temp who = greet("vendor")
->    7 ~ temp n = 2
      8 Hello {who}, {n}.
      9 -> END
     10
(brink) step over -> step
  at story.ink:8
(brink) locals
  who = "hi vendor"
  n = 2
(brink) quit
```

## Scripted

`--script` runs a `.dbg` file instead of prompting, and prints the transcript. `#` starts a comment; blank lines are skipped.

```text
# session.dbg
break story.ink:7
run
expect-line 7
step over
locals
expect-local who = "hi vendor"
```

```sh
brink debug story.ink --script session.dbg
```

The `expect-*` verbs — `expect-line`, `expect-local`, `expect-stack`, `expect-terminal` — are assertions: a violated one fails the process, so a `.dbg` script is usable as a CI test. An unknown verb is an error rather than a skipped line, so a typo can never quietly turn an assertion off.

This is the same script format, and the same verb implementations, that the compiler's own debug-session goldens run. There is one definition of "step over" behind the terminal, the studio, and the test harness rather than three that can drift apart.
