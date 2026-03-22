import type { ReactNode } from "react";
import { Binder } from "./Binder.js";
import { EditorPane } from "./EditorPane.js";
import { PlayerPane } from "./PlayerPane.js";
import { Toast } from "./Toast.js";
import { useStudioStore } from "./StoreContext.js";

function App({ editorSlot }: { editorSlot: ReactNode }) {
  const fullscreen = useStudioStore((s) => s.playerFullscreen);

  // No wrapper div — React renders directly into the existing #app element.
  return (
    <>
      <div id="binder-pane">
        <div className="header">Binder</div>
        <Binder />
      </div>
      {!fullscreen && <EditorPane>{editorSlot}</EditorPane>}
      <PlayerPane />
      <Toast />
    </>
  );
}

export { App };
