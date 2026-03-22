import { createContext, useContext, type ReactNode } from "react";
import { useStore } from "zustand";
import type { StudioState, StudioStore } from "@brink/studio-store";

const StoreContext = createContext<StudioStore | null>(null);

export function StoreProvider({ store, children }: { store: StudioStore; children: ReactNode }) {
  return <StoreContext.Provider value={store}>{children}</StoreContext.Provider>;
}

export function useStudioStore<T>(selector: (state: StudioState) => T): T {
  const store = useContext(StoreContext);
  if (!store) throw new Error("useStudioStore must be used within a StoreProvider");
  return useStore(store, selector);
}
