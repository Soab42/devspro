import { createContext, ReactNode, useCallback, useContext, useRef, useState } from "react";
import Modal from "../components/Modal";

interface ConfirmOptions {
  title?: string;
  confirmLabel?: string;
  danger?: boolean;
}

type ConfirmFn = (message: string, options?: ConfirmOptions) => Promise<boolean>;

const ConfirmContext = createContext<ConfirmFn | null>(null);

interface PendingConfirm extends ConfirmOptions {
  message: string;
}

// window.confirm()/alert() don't reliably show anything in this app's WebKitGTK
// webview on Linux (Tauri doesn't wire up native JS dialogs there) — this renders
// a real React modal instead and resolves a promise the same way confirm() would.
export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<PendingConfirm | null>(null);
  const resolver = useRef<((value: boolean) => void) | undefined>(undefined);

  const confirmAction: ConfirmFn = useCallback((message, options) => {
    setPending({ message, ...options });
    return new Promise<boolean>((resolve) => {
      resolver.current = resolve;
    });
  }, []);

  function respond(value: boolean) {
    setPending(null);
    resolver.current?.(value);
  }

  return (
    <ConfirmContext.Provider value={confirmAction}>
      {children}
      {pending && (
        <Modal title={pending.title ?? "Confirm"} onClose={() => respond(false)}>
          <p>{pending.message}</p>
          <div className="form-row">
            <button className="secondary" onClick={() => respond(false)}>
              Cancel
            </button>
            <button className={pending.danger ? "danger" : ""} onClick={() => respond(true)}>
              {pending.confirmLabel ?? "Confirm"}
            </button>
          </div>
        </Modal>
      )}
    </ConfirmContext.Provider>
  );
}

export function useConfirm(): ConfirmFn {
  const ctx = useContext(ConfirmContext);
  if (!ctx) throw new Error("useConfirm must be used within ConfirmProvider");
  return ctx;
}
