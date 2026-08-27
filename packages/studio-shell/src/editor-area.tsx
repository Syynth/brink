/**
 * @brink/studio-shell — editor area: groups + per-group tab bars (spec §7.8).
 *
 * Renders the editor region purely from the editor-groups store plus the
 * document-type registry: one vertical column per group (splitters reuse the
 * dock pattern), a tab bar per group, and the active tab's registered
 * component mounted in the group body. The shell never knows what a document
 * *is* — it mounts `DocumentViewProps` components by type id (§7.2).
 *
 * Only each group's active tab is mounted; backgrounded tabs keep their state
 * in whatever machinery the document component owns (keyed by docId+groupId).
 * Tab interactions go straight to the groups store (structural edits, like
 * splitter drags) — commands cover the keyboard/palette surface.
 */

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type WheelEvent as ReactWheelEvent,
} from "react";
import { Group, Panel, Separator, type PanelSize } from "react-resizable-panels";
import { useDocumentTypes, useEditorGroups, useShell } from "./shell-context.js";
import { documentKey, type DocumentTypeDescriptor } from "./document.js";
import type { EditorGroup, EditorTab } from "./editor-groups.js";
import { useTabDrag, type TabDragController } from "./tab-drag.js";

// ── Tab bar ─────────────────────────────────────────────────────────

interface GroupTabBarProps {
  group: EditorGroup;
  drag: TabDragController;
}

/**
 * One tab strip for a group. Click activates (and focuses the group),
 * double-click pins a preview tab, the close glyph closes, dragging reorders
 * or moves the tab (#142 — see tab-drag.ts). Class names are kept from the
 * old FileTabBar so existing styling and tests carry over.
 */
function GroupTabBar({ group, drag }: GroupTabBarProps) {
  const { editorGroups, documentIcon: DocumentIcon } = useShell();
  const scrollRef = useRef<HTMLDivElement>(null);
  // Which scroll-affordance chevrons to show. The bar is a horizontal scroller
  // (overflow-x: auto) with a hidden scrollbar, so without these the overflow
  // tabs are unreachable — the "lost to the void" problem (#278).
  const [overflow, setOverflow] = useState({ left: false, right: false });

  const updateOverflow = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const max = el.scrollWidth - el.clientWidth;
    setOverflow({ left: el.scrollLeft > 1, right: el.scrollLeft < max - 1 });
  }, []);

  // Recompute on size changes (group resized) and whenever the tab set changes.
  useEffect(() => {
    const el = scrollRef.current;
    if (el === null) return undefined;
    updateOverflow();
    const ro = new ResizeObserver(updateOverflow);
    ro.observe(el);
    return () => ro.disconnect();
  }, [updateOverflow, group.tabs.length]);

  // Keep the active tab visible — opening/activating a tab that sits off-screen
  // scrolls it into view rather than leaving it hidden past the edge.
  useEffect(() => {
    const active = scrollRef.current?.querySelector<HTMLElement>(".brink-tab.active");
    active?.scrollIntoView({ block: "nearest", inline: "nearest" });
    updateOverflow();
  }, [group.activeKey, updateOverflow]);

  const scrollTabs = useCallback((dir: -1 | 1) => {
    const el = scrollRef.current;
    if (el === null) return;
    el.scrollBy({ left: dir * Math.max(160, el.clientWidth * 0.6), behavior: "smooth" });
  }, []);

  // Most mice only scroll the Y axis; redirect that to horizontal so the wheel
  // reaches overflow tabs. Trackpads (which emit deltaX) are left untouched.
  const onWheel = useCallback((event: ReactWheelEvent<HTMLDivElement>) => {
    const el = scrollRef.current;
    if (el === null || event.deltaX !== 0) return;
    if (el.scrollWidth <= el.clientWidth) return;
    el.scrollLeft += event.deltaY;
  }, []);

  return (
    <div className="brink-tab-strip">
      {overflow.left && (
        <button
          type="button"
          className="brink-tab-scroll left"
          aria-label="Scroll tabs left"
          tabIndex={-1}
          onClick={() => scrollTabs(-1)}
        >
          {"‹"}
        </button>
      )}
      <div
        className="brink-file-tabs"
        role="tablist"
        data-group={group.id}
        ref={scrollRef}
        onScroll={updateOverflow}
        onWheel={onWheel}
      >
        {group.tabs.map((tab) => {
        const key = documentKey(tab.ref);
        const active = key === group.activeKey;
        return (
          <div
            key={key}
            role="tab"
            aria-selected={active}
            className={
              "brink-tab" + (active ? " active" : "") + (tab.pinned ? "" : " unpinned")
            }
            title={tab.ref.title}
            {...drag.handlersFor(group.id, tab)}
            onClick={() => {
              // A click that ends a drag is not an activation (the
              // pointerdown already activated anyway — strip-drag parity).
              if (drag.consumeClickSuppression()) return;
              editorGroups.getState().setActiveTab(group.id, key);
            }}
            onDoubleClick={(e) => {
              e.preventDefault();
              if (!tab.pinned) editorGroups.getState().pinTab(group.id, key);
            }}
          >
            {DocumentIcon && <DocumentIcon doc={tab.ref} />}
            <span className="brink-tab-label">{tab.ref.title}</span>
            <span
              className="brink-tab-close"
              title="Close"
              onClick={(e) => {
                e.stopPropagation();
                editorGroups.getState().closeTab(group.id, key);
              }}
            >
              {"×"}
            </span>
          </div>
        );
      })}
      </div>
      {overflow.right && (
        <button
          type="button"
          className="brink-tab-scroll right"
          aria-label="Scroll tabs right"
          tabIndex={-1}
          onClick={() => scrollTabs(1)}
        >
          {"›"}
        </button>
      )}
    </div>
  );
}

// ── Group body ──────────────────────────────────────────────────────

function activeTab(group: EditorGroup): EditorTab | null {
  if (group.activeKey === null) return null;
  return group.tabs.find((t) => documentKey(t.ref) === group.activeKey) ?? null;
}

interface EditorGroupViewProps {
  group: EditorGroup;
  focused: boolean;
  types: ReadonlyMap<string, DocumentTypeDescriptor>;
  drag: TabDragController;
}

function EditorGroupView({ group, focused, types, drag }: EditorGroupViewProps) {
  const { editorGroups } = useShell();
  const tab = activeTab(group);
  const descriptor = tab ? types.get(tab.ref.typeId) : undefined;

  let body: ReactNode;
  if (tab === null) {
    body = <div className="shell-editor-empty">No open editors</div>;
  } else if (descriptor === undefined) {
    body = (
      <div className="shell-editor-empty" data-unknown-document-type={tab.ref.typeId}>
        Unknown document type “{tab.ref.typeId}”
      </div>
    );
  } else {
    const Doc = descriptor.component;
    body = (
      <Doc
        key={documentKey(tab.ref)}
        doc={tab.ref}
        groupId={group.id}
        active={focused}
      />
    );
  }

  return (
    <section
      className={"editor-pane shell-editor-group" + (focused ? " focused" : "")}
      data-editor-group={group.id}
      data-focused={focused || undefined}
      // React's onFocus maps to focusin, which bubbles: focusing the CM6
      // editor (or anything else) inside a group focuses the group.
      onFocus={() => editorGroups.getState().focusGroup(group.id)}
    >
      <GroupTabBar group={group} drag={drag} />
      <div className="editor shell-editor-group-body">{body}</div>
    </section>
  );
}

// ── Editor area ─────────────────────────────────────────────────────

/**
 * The center region's content: all groups side by side with dock-style
 * splitters. Always rendered inside ShellFrame's stable editor Panel, so the
 * area itself never remounts across tier changes.
 */
export function EditorArea() {
  const { editorGroups } = useShell();
  const groups = useEditorGroups((s) => s.groups);
  const focusedGroupId = useEditorGroups((s) => s.focusedGroupId);
  const maximizedGroupId = useEditorGroups((s) => s.maximizedGroupId);
  const descriptors = useDocumentTypes();
  const types = useMemo(
    () => new Map(descriptors.map((d) => [d.id, d])),
    [descriptors],
  );
  const drag = useTabDrag(editorGroups);

  // Tab-drag ghost (#142): a tab-shaped chip following the cursor.
  // Fixed-positioned but rendered in the area's tree (not portaled) so the
  // .brink-studio design tokens apply; the controller moves it imperatively
  // on pointermove (strip-drag pattern).
  const ghost =
    drag.dragging !== null ? (
      <div className="shell-tab-drag-ghost" ref={drag.setGhostElement} aria-hidden="true">
        <span className="brink-tab-label">{drag.dragging.title}</span>
      </div>
    ) : null;

  // Group maximize (spec §5.4): only the maximized group renders — siblings
  // unmount (their tabs keep cached state like any backgrounded view) and
  // come back with their stored splitter sizes on restore.
  const maximizedGroup =
    maximizedGroupId !== null
      ? groups.find((g) => g.id === maximizedGroupId)
      : undefined;
  if (maximizedGroup) {
    return (
      <div className="shell-editor-area" data-maximized-group={maximizedGroup.id}>
        <EditorGroupView
          group={maximizedGroup}
          focused={maximizedGroup.id === focusedGroupId}
          types={types}
          drag={drag}
        />
        {ghost}
      </div>
    );
  }

  if (groups.length === 1) {
    return (
      <div className="shell-editor-area">
        <EditorGroupView
          group={groups[0]}
          focused={groups[0].id === focusedGroupId}
          types={types}
          drag={drag}
        />
        {ghost}
      </div>
    );
  }

  // Sizes are read imperatively, like dock sizes in ShellFrame: Panel only
  // consumes defaultSize at mount, and subscribing would re-render the whole
  // area on every splitter drag.
  const sizes = editorGroups.getState().groupSizes;
  const onGroupResize =
    (groupId: string) =>
    (size: PanelSize): void => {
      editorGroups.getState().setGroupSize(groupId, size.inPixels);
    };

  const children: ReactNode[] = [];
  groups.forEach((group, index) => {
    if (index > 0) {
      children.push(
        <Separator key={`sep-${group.id}`} className="brink-resize-handle" />,
      );
    }
    children.push(
      <Panel
        id={group.id}
        key={group.id}
        minSize="160px"
        defaultSize={sizes[group.id] !== undefined ? `${sizes[group.id]}px` : undefined}
        onResize={onGroupResize(group.id)}
      >
        <EditorGroupView
          group={group}
          focused={group.id === focusedGroupId}
          types={types}
          drag={drag}
        />
      </Panel>,
    );
  });

  return (
    <div className="shell-editor-area">
      <Group orientation="horizontal" id="brink-editor-groups" className="shell-editor-groups">
        {children}
      </Group>
      {ghost}
    </div>
  );
}
