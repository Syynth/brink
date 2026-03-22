import { useCallback, useRef, useState } from "react";
import { useStudioStore } from "./StoreContext.js";
import type { TabInfo } from "@brink/studio-store";

function FileTabBar() {
  const tabs = useStudioStore((s) => s.tabs);
  const activeTabId = useStudioStore((s) => s.activeTabId);
  const openTab = useStudioStore((s) => s.openTab);
  const closeTab = useStudioStore((s) => s.closeTab);
  const pinTab = useStudioStore((s) => s.pinTab);
  const addFile = useStudioStore((s) => s.addFile);

  const [inputActive, setInputActive] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const handleTabClick = useCallback(
    (tab: TabInfo) => {
      if (tab.id === activeTabId) return;
      void openTab(tab.target, tab.pinned);
    },
    [activeTabId, openTab],
  );

  const handleTabDoubleClick = useCallback(
    (tab: TabInfo, e: React.MouseEvent) => {
      e.preventDefault();
      if (!tab.pinned) {
        pinTab(tab.id);
      }
    },
    [pinTab],
  );

  const handleClose = useCallback(
    (tabId: string, e: React.MouseEvent) => {
      e.stopPropagation();
      void closeTab(tabId);
    },
    [closeTab],
  );

  const handleNewClick = useCallback(() => {
    if (inputActive) return;
    setInputActive(true);
    // Focus after render
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [inputActive]);

  const cancelInput = useCallback(() => {
    setInputActive(false);
  }, []);

  const confirmInput = useCallback(() => {
    const input = inputRef.current;
    if (!input) return;
    let name = input.value.trim();
    if (!name) {
      setInputActive(false);
      return;
    }
    if (!name.includes(".")) {
      name += ".ink";
    }
    setInputActive(false);
    void addFile(name);
  }, [addFile]);

  const handleInputKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        confirmInput();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cancelInput();
      }
    },
    [confirmInput, cancelInput],
  );

  return (
    <div className="brink-file-tabs">
      {tabs.map((tab) => {
        const title =
          tab.target.kind === "file"
            ? tab.target.path
            : `${tab.target.name} in ${tab.target.path}`;

        return (
          <div
            key={tab.id}
            className={
              "brink-tab" +
              (tab.id === activeTabId ? " active" : "") +
              (tab.pinned ? "" : " unpinned")
            }
            title={title}
            onClick={() => handleTabClick(tab)}
            onDoubleClick={(e) => handleTabDoubleClick(tab, e)}
          >
            <span className="brink-tab-label">{tab.label}</span>
            {tabs.length > 1 && (
              <span
                className="brink-tab-close"
                title="Close"
                onClick={(e) => handleClose(tab.id, e)}
              >
                {"\u00d7"}
              </span>
            )}
          </div>
        );
      })}

      <div className="brink-tab-new" title="New file" onClick={handleNewClick}>
        +
      </div>

      {inputActive && (
        <div className="brink-tab-input-wrapper">
          <input
            ref={inputRef}
            className="brink-tab-input"
            type="text"
            placeholder="filename.ink"
            size={16}
            onKeyDown={handleInputKeyDown}
            onBlur={cancelInput}
          />
        </div>
      )}
    </div>
  );
}

export { FileTabBar };
