import { memo, useCallback, useEffect, useRef } from "react";
import { useStudioStore } from "./StoreContext.js";

function ToastInner() {
  const message = useStudioStore((s) => s.toastMessage);
  const undoAction = useStudioStore((s) => s.toastUndoAction);
  const dismissToast = useStudioStore((s) => s.dismissToast);

  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!message) return;
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => {
      dismissToast();
      timerRef.current = null;
    }, 5000);
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [message, dismissToast]);

  const handleUndo = useCallback(() => {
    if (undoAction) undoAction();
  }, [undoAction]);

  if (!message) return null;

  return (
    <div className="brink-toast">
      <span className="brink-toast-message">{message}</span>
      {undoAction && (
        <button className="brink-toast-undo" onClick={handleUndo}>
          Undo
        </button>
      )}
      <button className="brink-toast-dismiss" onClick={dismissToast}>
        &times;
      </button>
    </div>
  );
}

export const Toast = memo(ToastInner);
