import type { ReactNode } from "react";
import { FileTabBar } from "./FileTabBar.js";
import { StatusBar } from "./StatusBar.js";

function EditorPane({ children }: { children: ReactNode }) {
  return (
    <div id="editor-pane">
      <FileTabBar />
      <div id="editor">{children}</div>
      <StatusBar />
    </div>
  );
}

export { EditorPane };
