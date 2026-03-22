import type { ReactNode } from "react";
import { Group, Panel, Separator } from "react-resizable-panels";
import { Binder } from "./Binder.js";
import { EditorPane } from "./EditorPane.js";
import { PlayerPane } from "./PlayerPane.js";
import { Toast } from "./Toast.js";
import { useStudioStore } from "./StoreContext.js";

function App({ editorSlot }: { editorSlot: ReactNode }) {
  const fullscreen = useStudioStore((s) => s.playerFullscreen);
  const playerVisible = useStudioStore((s) => s.playerVisible);
  const togglePlayerVisible = useStudioStore((s) => s.togglePlayerVisible);

  return (
    <Group orientation="horizontal" id="brink-layout">
      <Panel id="binder" defaultSize="220px" minSize="140px" maxSize="400px">
        <div id="binder-pane">
          <div className="header">
            <span>Binder</span>
            <button
              className="brink-panel-toggle"
              onClick={togglePlayerVisible}
              title={playerVisible ? "Hide player" : "Show player"}
            >
              {playerVisible ? "\u25b6" : "\u25c0"}
            </button>
          </div>
          <Binder />
        </div>
      </Panel>

      <Separator className="brink-resize-handle" />

      {!fullscreen && (
        <Panel id="editor" minSize="200px">
          <EditorPane>{editorSlot}</EditorPane>
        </Panel>
      )}

      {playerVisible && (
        <>
          {!fullscreen && <Separator className="brink-resize-handle" />}
          <Panel id="player" minSize="200px">
            <PlayerPane />
          </Panel>
        </>
      )}

      <Toast />
    </Group>
  );
}

export { App };
