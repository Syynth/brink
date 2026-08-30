/**
 * A `file.ink:249` link back to source — ONE contract for every panel row
 * that carries provenance (#3339: the Line tables view's source column and
 * the Disassembly view's provenance column share this).
 *
 * Byte offsets in, `editor.reveal` out: the label's line number and the
 * revealed span both come from the file's {@link SourceIndex}, which also
 * converts UTF-8 bytes to the UTF-16 units the editor road consumes. A
 * file the session cannot serve still navigates best-effort with the raw
 * offsets, unnumbered rather than wrong.
 */

import { EDITOR_REVEAL_COMMAND_ID } from "@brink/studio-shell";
import type { SourceIndex } from "./source-index.js";

export function SourceLink({
  file,
  startByte,
  endByte,
  indexFor,
  commands,
}: {
  file: string;
  startByte: number;
  endByte: number;
  indexFor: (file: string) => SourceIndex | null;
  commands: { dispatch: (id: string, arg?: unknown) => void };
}) {
  const index = indexFor(file);
  const lineNo = index?.lineForByte(startByte) ?? null;
  return (
    <button
      type="button"
      className="pv-lines-source-link"
      title={`${file} · bytes ${startByte}–${endByte}`}
      onClick={() =>
        commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
          kind: "source",
          file,
          span:
            index === null
              ? { start: startByte, end: endByte }
              : { start: index.utf16ForByte(startByte), end: index.utf16ForByte(endByte) },
        })
      }
    >
      {file.split("/").pop()}
      {lineNo !== null ? `:${lineNo}` : ""}
    </button>
  );
}
