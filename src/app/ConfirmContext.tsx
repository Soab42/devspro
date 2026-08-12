import {
  createContext,
  ReactNode,
  useCallback,
  useContext,
  useRef,
  useState,
} from "react";
import { Button, Modal, ModalFooter } from "../ui";
import { TriangleAlert } from "../ui/icons";
import "./ConfirmContext.css";

export interface ConfirmOptions {
  title?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

type ConfirmFn = (message: ReactNode, options?: ConfirmOptions) => Promise<boolean>;

const ConfirmContext = createContext<ConfirmFn | null>(null);

interface PendingConfirm extends ConfirmOptions {
  message: ReactNode;
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

  // Every dismissal path — Cancel, Escape, the close button, a backdrop click —
  // routes through here, so none of them can accidentally resolve true.
  function respond(value: boolean) {
    setPending(null);
    resolver.current?.(value);
    resolver.current = undefined;
  }

  return (
    <ConfirmContext.Provider value={confirmAction}>
      {children}
      {pending && (
        <Modal
          size="sm"
          title={pending.title ?? "Are you sure?"}
          icon={pending.danger ? TriangleAlert : undefined}
          tone={pending.danger ? "danger" : "default"}
          onClose={() => respond(false)}
          footer={
            <ModalFooter>
              <Button variant="secondary" onClick={() => respond(false)}>
                {pending.cancelLabel ?? "Cancel"}
              </Button>
              <Button
                variant={pending.danger ? "danger-solid" : "primary"}
                onClick={() => respond(true)}
                /* Deliberately not autofocused for destructive prompts: a
                   stray Enter keypress should not delete a database. */
                autoFocus={!pending.danger}
              >
                {pending.confirmLabel ?? "Confirm"}
              </Button>
            </ModalFooter>
          }
        >
          <div className="confirm-body">{pending.message}</div>
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
