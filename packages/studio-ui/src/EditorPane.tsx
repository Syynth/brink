import type { ReactNode } from "react";
import { FileTabBar } from "./FileTabBar.js";

function EditorPane({ children }: { children: ReactNode }) {
  return (
    <div className="editor-pane">
      <FileTabBar />
      <div className="editor">{children}</div>
    </div>
  );
}

export { EditorPane };
