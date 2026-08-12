import { useState } from "react";
import Modal from "../../../components/Modal";

export default function CommandModal({ command, onClose }: { command: string; onClose: () => void }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  }

  return (
    <Modal title="Full command" onClose={onClose}>
      <pre className="log-viewer">{command}</pre>
      <div className="form-row">
        <button onClick={handleCopy}>{copied ? "Copied!" : "Copy"}</button>
      </div>
    </Modal>
  );
}
