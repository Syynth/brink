/**
 * A minimal `renderHook` for driving a React hook directly in a test,
 * without pulling in `@testing-library/react` (not a dependency anywhere in
 * this workspace — see `grep -r "testing-library/react"`, which finds
 * nothing). Every other hook-adjacent test in this suite either mounts a
 * real component (e.g. `editor-area-maximize-paint.test.tsx`) or drives
 * store/pure-function state directly; this file exists because
 * `useTabDrag`/`useStripDrag` (`dismiss-net-exempt-claims.test.tsx`, #2846)
 * are hooks with no component of their own to mount — a bare host
 * component that re-assigns a ref on every render is the whole
 * requirement, so a dependency is not worth adding for it.
 *
 * A plain module (no `.test.` suffix) per the house rule against importing
 * test files as fixtures (`packages/brink-studio/src/__tests__/no-test-file-imports.test.ts`).
 */

import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

export interface RenderHookResult<T> {
  /** Always current as of the last flushed render — read after an `act()`. */
  result: { current: T };
  unmount(): void;
}

/** Mount `hook` in a throwaway host component and expose its latest return value. */
export function renderHook<T>(hook: () => T): RenderHookResult<T> {
  const container = document.createElement("div");
  document.body.appendChild(container);
  let root: Root | null = createRoot(container);
  const result = { current: undefined as unknown as T };

  function Harness(): null {
    result.current = hook();
    return null;
  }

  act(() => {
    root!.render(createElement(Harness));
  });

  return {
    result,
    unmount(): void {
      if (root === null) return;
      act(() => root!.unmount());
      root = null;
      container.remove();
    },
  };
}
