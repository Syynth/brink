/**
 * @brink/studio-shell — region components (spec §3, §5.1, §5.3).
 *
 * ShellFrame renders the dock/strip shell purely from the tool-window
 * registry plus the layout store: strips on the left/right/bottom edges,
 * edge docks with two sections each, and the editor area always center
 * (EditorArea — groups + tabs from the editor-groups store, §7.8).
 *
 * Strip icons are draggable to any of the six dock sections (§5.1, #87):
 * ShellFrame owns the drag controller (useStripDrag) and the ghost chip;
 * strips render the drop zones while a drag is active. See strip-drag.ts
 * for the gesture and the layout-store-vs-command design note.
 *
 * Tier presentation (spec §5.3): wide docks everything; medium turns the
 * side docks into slide-over drawers (bottom stays docked, strips stay);
 * narrow is editor-only with a compact topbar (left-dock drawer toggle +
 * an Editor/<window> segmented switcher) and full overlays.
 *
 * Invariant: the editor Panel exists with a stable key in every
 * non-fullscreen tier, so the editor never remounts across tier changes.
 * Tool windows keep their state in the store and may relocate/remount.
 */

import { useEffect, useMemo, type ReactNode } from "react";
import { Group, Panel, Separator, type PanelSize } from "react-resizable-panels";
import {
  useEditorGroups,
  useShell,
  useShellLayout,
  useStatusBarItems,
  useToolWindows,
} from "./shell-context.js";
import { EditorArea } from "./editor-area.js";
import { SingleFileView } from "./single-file-view.js";
import { formatChord } from "./keymap.js";
import { statusBarGroups, type StatusBarItemDescriptor } from "./statusbar.js";
import { viewToggleCommandId } from "./view-commands.js";
import { HamburgerMenu } from "./menu.js";
import { useStripDrag, type StripDragController } from "./strip-drag.js";
import {
  dockSectionId,
  type Dock,
  type DockSectionId,
  type Placement,
  type ToolWindowDescriptor,
} from "./toolwindow.js";


// ── Tool-window chrome ──────────────────────────────────────────────

/** Slim header (title + close-via-command) above a descriptor's component. */
function ToolWindowChrome({ descriptor }: { descriptor: ToolWindowDescriptor }) {
  const { commands } = useShell();
  const Body = descriptor.component;
  // Header controls are a component (see ToolWindowDescriptor.actions), so
  // they subscribe to the app's own store and stay live without the shell
  // knowing anything about it — same contract as `badge`.
  const Actions = descriptor.actions;
  return (
    <section className="shell-toolwindow" data-toolwindow={descriptor.id}>
      <div className="header">
        <span>{descriptor.title}</span>
        <span className="shell-toolwindow-header-actions">
          {Actions ? <Actions /> : null}
          <button
            type="button"
            className="brink-panel-toggle"
            onClick={() => commands.dispatch(viewToggleCommandId(descriptor.id))}
            title={`Close ${descriptor.title}`}
            aria-label={`Close ${descriptor.title}`}
          >
            {"×"}
          </button>
        </span>
      </div>
      <div className="shell-toolwindow-body">
        <Body />
      </div>
    </section>
  );
}

// ── Strips ──────────────────────────────────────────────────────────

interface StripProps {
  dock: Dock;
  items: ToolWindowDescriptor[];
  placements: Record<string, Placement>;
  open: Record<DockSectionId, string | null>;
  drag: StripDragController;
}

/**
 * One icon button per tool window placed on this dock, clustered by section:
 * start-section icons at the strip's start, end-section icons anchored at the
 * strip's far end (JetBrains-style halves), registration order within each.
 * Clicks dispatch the generated toggle command — never the store directly —
 * so the palette stays the source of truth.
 * Re-docking (drag, §5.1) is the exception: it goes through the drag
 * controller straight to the layout store, like a splitter drag.
 *
 * While a drag is active the strip enters drop mode: it renders even when it
 * has no icons (every dock section is a valid target), and overlays its two
 * sections as drop zones.
 */
function Strip({ dock, items, placements, open, drag }: StripProps) {
  const { commands, keymap, isMac } = useShell();
  const dropMode = drag.dragging !== null;
  if (items.length === 0 && !dropMode) return null;

  const renderButton = (d: ToolWindowDescriptor) => {
    const placement = placements[d.id];
    const active = placement !== undefined && open[dockSectionId(placement)] === d.id;
    const chord = keymap.bindingFor(viewToggleCommandId(d.id));
    const tooltip = chord ? `${d.title} (${formatChord(chord, isMac)})` : d.title;
    // Badge is a component (see ToolWindowDescriptor.badge): it subscribes
    // to the app's own store, so counts stay live without re-rendering
    // the strip itself.
    const Badge = d.badge;
    return (
      <button
        key={d.id}
        type="button"
        className={"shell-strip-btn" + (active ? " active" : "")}
        title={tooltip}
        aria-label={d.title}
        aria-pressed={active}
        onClick={() => {
          // The pointerup that ends a drag still fires a click on the
          // capturing button — a re-dock must not also toggle.
          if (drag.consumeClickSuppression()) return;
          commands.dispatch(viewToggleCommandId(d.id));
        }}
        {...drag.handlersFor(d)}
      >
        {d.icon}
        {Badge && <Badge />}
      </button>
    );
  };

  const startItems = items.filter((d) => placements[d.id]?.section !== "end");
  const endItems = items.filter((d) => placements[d.id]?.section === "end");
  return (
    <div
      className={`shell-strip shell-strip-${dock}` + (dropMode ? " drop-mode" : "")}
      role="toolbar"
      aria-label={`${dock} dock`}
      aria-orientation={dock === "bottom" ? "horizontal" : "vertical"}
    >
      <div className="shell-strip-section shell-strip-section-start">
        {startItems.map(renderButton)}
      </div>
      <div className="shell-strip-section shell-strip-section-end">
        {endItems.map(renderButton)}
      </div>
      {dropMode && (
        <>
          <div
            className="shell-strip-dropzone shell-strip-dropzone-start"
            data-zone={`${dock}.start`}
          />
          <div
            className="shell-strip-dropzone shell-strip-dropzone-end"
            data-zone={`${dock}.end`}
          />
        </>
      )}
    </div>
  );
}

// ── Status bar ──────────────────────────────────────────────────────

function StatusBarItem({ descriptor }: { descriptor: StatusBarItemDescriptor }) {
  const Segment = descriptor.component;
  return (
    <div className="shell-statusbar-item" data-statusbar-item={descriptor.id}>
      <Segment />
    </div>
  );
}

/**
 * The status bar region (spec §7.3): full shell width at the very bottom,
 * below docks and strips. Renders two segment groups purely from the
 * status-bar registry — left/right, each ordered by descending priority
 * (higher = further left within its group, ties by registration order; see
 * statusBarGroups). The shell never knows what the segments are.
 */
export function ShellStatusBar() {
  const items = useStatusBarItems();
  const groups = useMemo(() => statusBarGroups(items), [items]);
  return (
    <footer className="shell-statusbar" role="status" aria-label="Status bar">
      <div className="shell-statusbar-group shell-statusbar-left">
        {groups.left.map((d) => (
          <StatusBarItem key={d.id} descriptor={d} />
        ))}
      </div>
      <div className="shell-statusbar-group shell-statusbar-right">
        {groups.right.map((d) => (
          <StatusBarItem key={d.id} descriptor={d} />
        ))}
      </div>
    </footer>
  );
}

// ── Shell frame ─────────────────────────────────────────────────────

/**
 * The editor root area's single occupant (decision log 2026-08-26). Which
 * view is showing is layout state, so this is the one place that reads it —
 * every other region is unaware there is more than one.
 */
function EditorRoot() {
  const view = useShellLayout((s) => s.editorView);
  const { companionDocument } = useShell();
  if (view === "single") return <SingleFileView companion={companionDocument} />;
  return <EditorArea />;
}

export function ShellFrame() {
  const { layout, editorGroups } = useShell();
  const descriptors = useToolWindows();
  const tier = useShellLayout((s) => s.tier);
  const placements = useShellLayout((s) => s.placements);
  const open = useShellLayout((s) => s.open);
  const drawers = useShellLayout((s) => s.drawers);
  const narrowView = useShellLayout((s) => s.narrowView);
  const maximized = useShellLayout((s) => s.maximized);
  // Editor-group maximize (spec §5.4): the maximized group takes the whole
  // editor area (EditorArea hides its siblings) and the open docks collapse
  // here — purely presentational, so restore touches nothing else.
  const groupMaximized = useEditorGroups((s) => s.maximizedGroupId) !== null;

  // Strip-icon drag-to-re-dock (§5.1, #87). Transient interaction state —
  // lives here, not in the layout store; only the drop touches the store.
  const drag = useStripDrag(layout);

  // Maximize restore (spec §5.4): Escape anywhere brings the layout back —
  // tool-window maximize and group maximize alike (at most one is active;
  // the commands enforce mutual exclusion, and a stray double restores the
  // tool window first). Capture-phase so it wins over focused widgets.
  useEffect(() => {
    if (maximized === null && !groupMaximized) return;
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      if (maximized !== null) {
        layout.getState().toggleMaximize(maximized);
      } else {
        editorGroups.getState().toggleMaximizeGroup();
      }
    };
    // DISMISS-NET-EXEMPT: restores tool-window/editor-group maximize (layout
    // store state), not a floating menu/popover/modal surface — the
    // dismiss-registry-enrolment.test.ts scan requires this marker or a
    // registerDismissible() enrolment for every document-level keydown /
    // pointerdown listener it finds.
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [maximized, groupMaximized, layout, editorGroups]);

  const byId = useMemo(
    () => new Map(descriptors.map((d) => [d.id, d])),
    [descriptors],
  );

  // Strip items per dock: section start→end, registration order within each
  // (descriptors is registry order — deterministic).
  const stripItems = useMemo(() => {
    const forDock = (dock: Dock): ToolWindowDescriptor[] => {
      const inDock = descriptors.filter((d) => placements[d.id]?.dock === dock);
      return [
        ...inDock.filter((d) => placements[d.id].section === "start"),
        ...inDock.filter((d) => placements[d.id].section === "end"),
      ];
    };
    return { left: forDock("left"), right: forDock("right"), bottom: forDock("bottom") };
  }, [descriptors, placements]);

  const renderToolWindow = (id: string | null): ReactNode => {
    const descriptor = id !== null ? byId.get(id) : undefined;
    return descriptor ? <ToolWindowChrome descriptor={descriptor} /> : null;
  };

  // A dock's open sections, rendered stacked (vertical split in side docks,
  // horizontal in the bottom dock). Null when neither section is open — the
  // dock collapses to its strip.
  const dockContent = (dock: Dock): ReactNode => {
    const startId = open[dockSectionId({ dock, section: "start" })];
    const endId = open[dockSectionId({ dock, section: "end" })];
    if (startId === null && endId === null) return null;
    if (startId === null || endId === null) {
      return renderToolWindow(startId ?? endId);
    }
    const horizontal = dock === "bottom";
    return (
      <Group
        orientation={horizontal ? "horizontal" : "vertical"}
        id={`brink-dock-${dock}`}
        className="shell-dock-sections"
      >
        <Panel id={`${dock}-start`} key="start" minSize="80px">
          {renderToolWindow(startId)}
        </Panel>
        <Separator
          className={
            horizontal
              ? "brink-resize-handle"
              : "brink-resize-handle brink-resize-handle-h"
          }
        />
        <Panel id={`${dock}-end`} key="end" minSize="80px">
          {renderToolWindow(endId)}
        </Panel>
      </Group>
    );
  };

  // Maximized (spec §5.4): only that tool window, full bleed. The editor
  // unmounts here, matching the old playerFullscreen behavior; Escape (or
  // re-dispatching view.maximize) restores the previous layout untouched.
  if (maximized !== null && byId.has(maximized)) {
    return (
      <div className="studio-body">
        <div className="shell-fullscreen">{renderToolWindow(maximized)}</div>
      </div>
    );
  }

  const narrow = tier === "narrow";
  const compact = tier !== "wide";
  const dockHasOpen = (dock: Dock): boolean =>
    open[dockSectionId({ dock, section: "start" })] !== null ||
    open[dockSectionId({ dock, section: "end" })] !== null;

  const showLeftDock = tier === "wide" && dockHasOpen("left") && !groupMaximized;
  const showRightDock = tier === "wide" && dockHasOpen("right") && !groupMaximized;
  const showBottomDock = !narrow && dockHasOpen("bottom") && !groupMaximized;

  // Dock sizes are read imperatively: Panel only consumes defaultSize at
  // mount (docks remount via the `open` change, which already re-renders),
  // and subscribing would re-render the whole frame on every splitter drag.
  const dockSizes = layout.getState().dockSizes;
  const onDockResize =
    (dock: Dock) =>
    (size: PanelSize): void => {
      layout.getState().setDockSize(dock, size.inPixels);
    };

  const leftDrawerVisible = compact && drawers.left && dockHasOpen("left");
  const rightDrawerVisible = tier === "medium" && drawers.right && dockHasOpen("right");

  // narrow: open non-left windows are overlay candidates, in section order.
  const overlayCandidates: ToolWindowDescriptor[] = [];
  if (narrow) {
    const order: DockSectionId[] = ["right.start", "right.end", "bottom.start", "bottom.end"];
    for (const key of order) {
      const id = open[key];
      const descriptor = id !== null ? byId.get(id) : undefined;
      if (descriptor) overlayCandidates.push(descriptor);
    }
  }
  const overlayId =
    narrow && narrowView !== null && overlayCandidates.some((d) => d.id === narrowView)
      ? narrowView
      : null;

  return (
    <>
      {narrow && (
        <div className="studio-topbar">
          <button
            type="button"
            className="studio-iconbtn"
            onClick={() => layout.getState().setDrawerOpen("left", !drawers.left)}
            aria-label="Toggle tool windows"
            title="Tool windows"
          >
            {"☰"}
          </button>
          <div className="studio-segmented" role="tablist" aria-label="View">
            <button
              type="button"
              role="tab"
              aria-selected={overlayId === null}
              className={"studio-seg" + (overlayId === null ? " active" : "")}
              onClick={() => layout.getState().setNarrowView(null)}
            >
              Editor
            </button>
            {overlayCandidates.map((d) => (
              <button
                key={d.id}
                type="button"
                role="tab"
                aria-selected={overlayId === d.id}
                className={"studio-seg" + (overlayId === d.id ? " active" : "")}
                onClick={() => layout.getState().setNarrowView(d.id)}
              >
                {d.title}
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="studio-body">
        {!narrow && (
          <div className="shell-rail-left">
            <HamburgerMenu />
            <Strip
              dock="left"
              items={stripItems.left}
              placements={placements}
              open={open}
              drag={drag}
            />
          </div>
        )}

        <div className="shell-main">
          <Group orientation="vertical" id="brink-shell-rows" className="shell-rows">
            <Panel id="main-row" key="main-row" minSize="120px">
              {/* The editor lives in one keyed panel in every non-fullscreen
                  tier so it never remounts; docks join/leave around it. */}
              <Group orientation="horizontal" id="brink-layout" className="studio-panes">
                {showLeftDock && (
                  <Panel
                    id="dock-left"
                    key="dock-left"
                    defaultSize={`${dockSizes.left}px`}
                    minSize="140px"
                    maxSize="520px"
                    onResize={onDockResize("left")}
                  >
                    <div className="shell-dock shell-dock-left">{dockContent("left")}</div>
                  </Panel>
                )}
                {showLeftDock && <Separator className="brink-resize-handle" />}

                <Panel id="editor" key="editor" minSize="200px">
                  <EditorRoot />
                </Panel>

                {showRightDock && <Separator className="brink-resize-handle" />}
                {showRightDock && (
                  <Panel
                    id="dock-right"
                    key="dock-right"
                    defaultSize={`${dockSizes.right}px`}
                    minSize="160px"
                    onResize={onDockResize("right")}
                  >
                    <div className="shell-dock shell-dock-right">{dockContent("right")}</div>
                  </Panel>
                )}
              </Group>
            </Panel>

            {showBottomDock && (
              <Separator className="brink-resize-handle brink-resize-handle-h" />
            )}
            {showBottomDock && (
              <Panel
                id="dock-bottom"
                key="dock-bottom"
                defaultSize={`${dockSizes.bottom}px`}
                minSize="100px"
                onResize={onDockResize("bottom")}
              >
                <div className="shell-dock shell-dock-bottom">{dockContent("bottom")}</div>
              </Panel>
            )}
          </Group>

          {!narrow && (
            <Strip
              dock="bottom"
              items={stripItems.bottom}
              placements={placements}
              open={open}
              drag={drag}
            />
          )}
        </div>

        {!narrow && (
          <Strip
            dock="right"
            items={stripItems.right}
            placements={placements}
            open={open}
            drag={drag}
          />
        )}

        {compact && (
          <>
            <div
              className={
                "studio-scrim" + (leftDrawerVisible || rightDrawerVisible ? " open" : "")
              }
              onClick={() => layout.getState().closeDrawers()}
            />
            <aside
              className={"shell-drawer shell-drawer-left" + (leftDrawerVisible ? " open" : "")}
              aria-hidden={!leftDrawerVisible}
            >
              {dockContent("left")}
            </aside>
            {tier === "medium" && (
              <aside
                className={
                  "shell-drawer shell-drawer-right" + (rightDrawerVisible ? " open" : "")
                }
                aria-hidden={!rightDrawerVisible}
              >
                {dockContent("right")}
              </aside>
            )}
          </>
        )}

        {narrow &&
          overlayCandidates.map((d) => (
            <div
              key={d.id}
              className={"shell-overlay-view" + (overlayId === d.id ? " open" : "")}
              aria-hidden={overlayId !== d.id}
            >
              <ToolWindowChrome descriptor={d} />
            </div>
          ))}

        {/* Drag ghost (§5.1): icon + title chip following the cursor.
            Fixed-positioned but rendered in the shell tree (not portaled),
            like Overlay, so the .brink-studio design tokens apply; the
            controller moves it imperatively on pointermove. */}
        {drag.dragging !== null && (
          <div className="shell-drag-ghost" ref={drag.setGhostElement} aria-hidden="true">
            <span className="shell-drag-ghost-icon">{drag.dragging.icon}</span>
            <span>{drag.dragging.title}</span>
          </div>
        )}
      </div>

      {/* Status bar (spec §3, §7.3): the bottom-most shell region, full
          width — below the docks, strips, and editor row above. */}
      <ShellStatusBar />
    </>
  );
}
