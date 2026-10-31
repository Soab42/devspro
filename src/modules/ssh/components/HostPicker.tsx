import { Badge, ChoiceList, Mono, Modal } from "../../../ui";
import { Plug, Server } from "../../../ui/icons";
import type { SshHost } from "../api";
import { hostLabel } from "../utils";

/**
 * The saved hosts as a list in a dialog, one row each; picking one closes it
 * and hands the host back. Shared by SFTP's and the Terminal's empty states,
 * so "connect to a server" looks the same wherever it starts.
 */
export default function HostPicker({
  hosts,
  description,
  onPick,
  onClose,
}: {
  hosts: SshHost[];
  /** What picking a host will do, e.g. "open an SFTP session". */
  description: string;
  onPick: (host: SshHost) => void;
  onClose: () => void;
}) {
  return (
    <Modal title="Connect to a host" description={description} icon={Plug} onClose={onClose}>
      <ChoiceList
        ariaLabel="Saved hosts"
        items={hosts.map((h) => ({
          key: h.id,
          icon: Server,
          title: h.name,
          detail: <Mono truncate>{hostLabel(h)}</Mono>,
          trailing: (
            <>
              {h.has_saved_password && <Badge tone="success">saved</Badge>}
              <Badge tone="neutral">{h.auth}</Badge>
            </>
          ),
          onSelect: () => onPick(h),
        }))}
      />
    </Modal>
  );
}
