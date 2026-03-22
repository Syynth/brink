import type { ReactNode } from "react";
import { Binder } from "./Binder.js";
import { EditorPane } from "./EditorPane.js";
import { PlayerPane } from "./PlayerPane.js";
import { useStudioStore } from "./StoreContext.js";

function App({ editorSlot }: { editorSlot: ReactNode }) {
  const fullscreen = useStudioStore((s) => s.playerFullscreen);

  return (
    <div id="app">
      <div id="binder-pane">
        <div className="header">Binder</div>
        <Binder />
      </div>
      {!fullscreen && <EditorPane>{editorSlot}</EditorPane>}
      <PlayerPane />
    </div>
  );
}

export { App };
