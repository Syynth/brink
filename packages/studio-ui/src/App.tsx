import { useRef, type ReactNode } from "react";
import { Group, Panel, Separator } from "react-resizable-panels";
import { Binder } from "./Binder.js";
import { EditorPane } from "./EditorPane.js";
import { PlayerPane } from "./PlayerPane.js";
import { StateView } from "./StateView.js";
import { ActivityBar, sidebarViewLabel } from "./ActivityBar.js";
import { Toast } from "./Toast.js";
import { useStudioStore } from "./StoreContext.js";
import { useTier } from "./useTier.js";

function App({ editorSlot }: { editorSlot: ReactNode }) {
  const fullscreen = useStudioStore((s) => s.playerFullscreen);
  const playerVisible = useStudioStore((s) => s.playerVisible);
  const togglePlayerVisible = useStudioStore((s) => s.togglePlayerVisible);
  const tier = useStudioStore((s) => s.tier);
  const binderDrawerOpen = useStudioStore((s) => s.binderDrawerOpen);
  const storyOpen = useStudioStore((s) => s.storyOpen);
  const toggleBinderDrawer = useStudioStore((s) => s.toggleBinderDrawer);
  const setBinderDrawerOpen = useStudioStore((s) => s.setBinderDrawerOpen);
  const setStoryOpen = useStudioStore((s) => s.setStoryOpen);
  const activeSidebarView = useStudioStore((s) => s.activeSidebarView);

  const rootRef = useRef<HTMLDivElement>(null);
  useTier(rootRef);

  // The active sidebar view (Binder or State), shared by the wide dock and the
  // compact drawer so the activity bar switches both the same way.
  const sidebarTitle = sidebarViewLabel(activeSidebarView);
  const sidebarBody = activeSidebarView === "binder" ? <Binder /> : <StateView />;

  const compact = tier !== "wide";
  const narrow = tier === "narrow";
  // The editor lives in one keyed panel in every non-fullscreen tier so it never
  // remounts. Binder docks only when wide; player docks when wide/medium. Both
  // relocate to overlays otherwise — safe, since their state lives in the store.
  const binderDocked = tier === "wide" && !fullscreen;
  const editorDocked = !fullscreen;
  const playerDocked = !narrow && playerVisible && !fullscreen;

  return (
    <div className="brink-studio" data-tier={tier} ref={rootRef}>
      {compact && !fullscreen && (
        <div className="studio-topbar">
          <button
            className="studio-iconbtn"
            onClick={toggleBinderDrawer}
            aria-label="Toggle binder"
            title="Binder"
          >
            {"☰"}
          </button>
          {narrow && (
            <div className="studio-segmented" role="tablist" aria-label="View">
              <button
                type="button"
                role="tab"
                aria-selected={!storyOpen}
                className={"studio-seg" + (!storyOpen ? " active" : "")}
                onClick={() => setStoryOpen(false)}
              >
                Editor
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={storyOpen}
                className={"studio-seg" + (storyOpen ? " active" : "")}
                onClick={() => setStoryOpen(true)}
              >
                Story
              </button>
            </div>
          )}
        </div>
      )}

      <div className="studio-body">
        {binderDocked && <ActivityBar />}
        <Group orientation="horizontal" id="brink-layout" className="studio-panes">
          {binderDocked && (
            <Panel id="binder" key="binder" defaultSize="220px" minSize="140px" maxSize="400px">
              <div className="binder-pane">
                <div className="header">
                  <span>{sidebarTitle}</span>
                  <button
                    className="brink-panel-toggle"
                    onClick={togglePlayerVisible}
                    title={playerVisible ? "Hide player" : "Show player"}
                  >
                    {playerVisible ? "▶" : "◀"}
                  </button>
                </div>
                {sidebarBody}
              </div>
            </Panel>
          )}
          {binderDocked && editorDocked && <Separator className="brink-resize-handle" />}

          {editorDocked && (
            <Panel id="editor" key="editor" minSize="200px">
              <EditorPane>{editorSlot}</EditorPane>
            </Panel>
          )}

          {playerDocked && editorDocked && <Separator className="brink-resize-handle" />}
          {playerDocked && (
            <Panel id="player" key="player" minSize="200px">
              <PlayerPane />
            </Panel>
          )}

          {fullscreen && (
            <Panel id="player" key="player-fs" minSize="200px">
              <PlayerPane />
            </Panel>
          )}
        </Group>

        {compact && (
          <>
            <div
              className={"studio-scrim" + (binderDrawerOpen ? " open" : "")}
              onClick={() => setBinderDrawerOpen(false)}
            />
            <aside
              className={"binder-drawer" + (binderDrawerOpen ? " open" : "")}
              aria-hidden={!binderDrawerOpen}
            >
              <div className="drawer-inner">
                <ActivityBar />
                <div className="binder-pane">
                  <div className="header">
                    <span>{sidebarTitle}</span>
                    <button
                      className="brink-panel-toggle"
                      onClick={() => setBinderDrawerOpen(false)}
                      title="Close"
                    >
                      {"×"}
                    </button>
                  </div>
                  {sidebarBody}
                </div>
              </div>
            </aside>
          </>
        )}

        {narrow && !fullscreen && (
          <div
            className={"player-overlay" + (storyOpen ? " open" : "")}
            aria-hidden={!storyOpen}
          >
            <PlayerPane />
          </div>
        )}
      </div>

      <Toast />
    </div>
  );
}

export { App };
