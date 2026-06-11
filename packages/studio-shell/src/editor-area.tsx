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

import { useMemo, type ReactNode } from "react";
import { Group, Panel, Separator, type PanelSize } from "react-resizable-panels";
import { useDocumentTypes, useEditorGroups, useShell } from "./shell-context.js";
import { documentKey, type DocumentTypeDescriptor } from "./document.js";
import type { EditorGroup, EditorTab } from "./editor-groups.js";

// ── Tab bar ─────────────────────────────────────────────────────────

interface GroupTabBarProps {
  group: EditorGroup;
}

/**
 * One tab strip for a group. Click activates (and focuses the group),
 * double-click pins a preview tab, the close glyph closes. Class names are
 * kept from the old FileTabBar so existing styling and tests carry over.
 */
function GroupTabBar({ group }: GroupTabBarProps) {
  const { editorGroups } = useShell();
  return (
    <div className="brink-file-tabs" role="tablist" data-group={group.id}>
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
            onClick={() => editorGroups.getState().setActiveTab(group.id, key)}
            onDoubleClick={(e) => {
              e.preventDefault();
              if (!tab.pinned) editorGroups.getState().pinTab(group.id, key);
            }}
          >
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
}

function EditorGroupView({ group, focused, types }: EditorGroupViewProps) {
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
      <GroupTabBar group={group} />
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
        />
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
        />
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
        />
      </Panel>,
    );
  });

  return (
    <div className="shell-editor-area">
      <Group orientation="horizontal" id="brink-editor-groups" className="shell-editor-groups">
        {children}
      </Group>
    </div>
  );
}
