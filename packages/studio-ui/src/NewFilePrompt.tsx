/**
 * NewFilePrompt — the `file.new` command's input surface.
 *
 * File creation used to live as a "+" button on the (ink-specific) file tab
 * bar; the shell's per-group tab bars are generic, so creation moved to a
 * palette-discoverable command (spec §7.8). The prompt reuses the shared
 * overlay + palette styling: type a name, Enter creates the file (".ink"
 * appended when no extension is given) and opens it pinned.
 */

import { useEffect, useRef, useState } from "react";
import { Overlay, useShell } from "@brink/studio-shell";
import { useStudioStore } from "./StoreContext.js";

export const FILE_NEW_COMMAND_ID = "file.new";

export function NewFilePrompt() {
  const { commands } = useShell();
  const addFile = useStudioStore((s) => s.addFile);
  const [open, setOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(
    () =>
      commands.register({
        id: FILE_NEW_COMMAND_ID,
        title: "File: New File",
        run: () => setOpen(true),
      }),
    [commands],
  );

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  const confirm = (): void => {
    const input = inputRef.current;
    if (!input) return;
    let name = input.value.trim();
    setOpen(false);
    if (name === "") return;
    if (!name.includes(".")) name += ".ink";
    void addFile(name);
  };

  return (
    <Overlay open={open} onClose={() => setOpen(false)} className="shell-palette">
      <input
        ref={inputRef}
        className="shell-palette-input"
        type="text"
        placeholder="filename.ink"
        aria-label="New file name"
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            confirm();
          }
        }}
      />
    </Overlay>
  );
}
