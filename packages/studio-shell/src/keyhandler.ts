/**
 * @brink/studio-shell — global key handler.
 *
 * One window-level keydown listener resolving keybindings to command dispatch
 * (docs/studio-shell-spec.md §6). Chrome-level keys only: events something
 * closer to the focus already handled (defaultPrevented) are skipped, and
 * modifier-less chords never fire from editable targets — editor-internal
 * editing keys stay inside CodeMirror.
 */

import type { CommandRegistry } from "./command.js";
import type { Keymap } from "./keymap.js";
import { chordFromEvent } from "./keymap.js";

export interface KeyHandlerOptions {
  /** Override platform detection (tests). */
  isMac?: boolean;
}

type ListenerTarget = Pick<Window, "addEventListener" | "removeEventListener">;

/** Attach the handler; returns a dispose function. */
export function attachKeyHandler(
  target: ListenerTarget,
  registry: CommandRegistry,
  keymap: Keymap,
  options: KeyHandlerOptions = {},
): () => void {
  const isMac = options.isMac ?? detectMac();

  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.defaultPrevented) return;
    const chord = chordFromEvent(event, isMac);
    if (chord === null) return;
    // Modifier-less chords never fire from editable targets — typing keys
    // belong to the editor. Function keys are exempt: they never insert
    // text, so suppressing them buys no safety (#107; VS Code behaves the
    // same — F-keys work globally).
    if (
      !chord.mod &&
      !chord.alt &&
      !isFunctionKey(chord.key) &&
      isEditableTarget(event.target)
    ) {
      return;
    }
    const commandId = keymap.resolveChord(chord);
    if (commandId === undefined) return;
    if (registry.dispatch(commandId)) {
      event.preventDefault();
      event.stopPropagation();
    }
  };

  target.addEventListener("keydown", onKeyDown as EventListener);
  return () => {
    target.removeEventListener("keydown", onKeyDown as EventListener);
  };
}

function isFunctionKey(key: string): boolean {
  return /^f([1-9]|1[0-9]|2[0-4])$/.test(key);
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

function detectMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iP(hone|ad|od)/.test(navigator.platform || navigator.userAgent);
}
