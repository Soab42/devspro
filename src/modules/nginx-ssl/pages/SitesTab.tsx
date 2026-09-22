import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useConfirm } from "../../../app/ConfirmContext";
import AddSiteForm from "../components/AddSiteForm";
import NewProjectForm from "../components/NewProjectForm";
import NodeVersionModal from "../components/NodeVersionModal";
import PhpVersionModal from "../components/PhpVersionModal";
import SiteLimitsModal from "../components/SiteLimitsModal";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Column,
  CopyButton,
  DataTable,
  IconButton,
  Menu,
  Modal,
  Mono,
  Resource,
  Section,
  Stack,
  StatusPill,
  useResource,
} from "../../../ui";
import {
  ExternalLink,
  FileCode,
  FolderPlus,
  Globe,
  Hexagon,
  Plus,
  Power,
  ShieldCheck,
  SlidersHorizontal,
  Trash2,
  Wrench,
} from "../../../ui/icons";
import {
  detectTunnelCli,
  fixLaravelPermissions,
  reissueSiteCert,
  removeSite,
  setSiteEnabled,
  Site,
  tunnelList,
  tunnelStart,
  tunnelStop,
  TunnelStatus,
} from "../api";

const TUNNEL_POLL_MS = 2000;

function backendLabel(site: Site): string {
  if (site.backend.type === "port") return `:${site.backend.port}`;
  const version = site.backend.php_version
    ? `PHP ${site.backend.php_version}`
    : "PHP (system default)";
  return `${site.backend.project_path} — ${version}`;
}

export default function SitesTab({ sites }: { sites: Resource<Site[]> }) {
  const confirmAction = useConfirm();

  const [showAddModal, setShowAddModal] = useState(false);
  const [showNewProjectModal, setShowNewProjectModal] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [phpModalSite, setPhpModalSite] = useState<Site | null>(null);
  const [nodeModalSite, setNodeModalSite] = useState<Site | null>(null);
  const [limitsModalSite, setLimitsModalSite] = useState<Site | null>(null);

  const [tunnels, setTunnels] = useState<Record<string, TunnelStatus>>({});
  const tunnelCli = useResource(detectTunnelCli);

  useEffect(() => {
    let active = true;

    async function refreshTunnels() {
      let list: TunnelStatus[];
      try {
        list = await tunnelList();
      } catch {
        return; // best-effort background poll — a transient failure shouldn't blank the UI
      }
      if (!active) return;
      const exited = list.filter((t) => t.exited);
      if (exited.length > 0) {
        setError(
          `Tunnel for ${exited[0].hostname} stopped unexpectedly: ${
            exited[0].last_log ?? "unknown error"
          }`,
        );
      }
      setTunnels(Object.fromEntries(list.map((t) => [t.site_id, t])));
    }

    void refreshTunnels();
    const interval = setInterval(refreshTunnels, TUNNEL_POLL_MS);
    return () => {
      active = false;
      clearInterval(interval);
    };
  }, []);

  async function handleStartTunnel(site: Site) {
    setError(null);
    try {
      const status = await tunnelStart(site.id);
      setTunnels((prev) => ({ ...prev, [site.id]: status }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleStopTunnel(site: Site) {
    setError(null);
    try {
      await tunnelStop(site.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setTunnels((prev) => {
      const next = { ...prev };
      delete next[site.id];
      return next;
    });
  }

  async function handleToggle(site: Site) {
    setError(null);
    try {
      await setSiteEnabled(site.id, !site.enabled);
      await sites.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleFixPermissions(site: Site) {
    setError(null);
    setMessage(null);
    try {
      setMessage(await fixLaravelPermissions(site.id));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleReissueCert(site: Site) {
    setError(null);
    setMessage(null);
    try {
      setMessage(await reissueSiteCert(site.id));
      await sites.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleRemove(site: Site) {
    const ok = await confirmAction(
      `"${site.name}" (${site.hostname}) will lose its nginx config and certificate.`,
      { title: "Delete site?", confirmLabel: "Delete", danger: true },
    );
    if (!ok) return;
    setError(null);
    try {
      await removeSite(site.id);
      await sites.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  const rows = sites.data ?? [];

  const columns: Column<Site>[] = [
    {
      key: "name",
      header: "Site",
      priority: 1,
      wrap: true,
      sortValue: (s) => s.name,
      render: (s) => (
        <div className="dt-cell-stack">
          <span className="dt-strong">{s.name}</span>
          <Mono className="dt-cell-sub">{s.hostname}</Mono>
        </div>
      ),
    },
    {
      key: "status",
      header: "Status",
      priority: 1,
      sortValue: (s) => (s.enabled ? 1 : 0),
      render: (s) => (
        <StatusPill
          variant={s.enabled ? "ok" : "neutral"}
          label={s.enabled ? "enabled" : "disabled"}
        />
      ),
    },
    {
      key: "tunnel",
      header: "Tunnel",
      priority: 2,
      render: (s) => {
        const tunnel = tunnels[s.id];
        if (tunnel?.public_url) {
          return (
            <span className="dt-cell-inline">
              <Mono truncate={180}>{tunnel.public_url}</Mono>
              <CopyButton value={tunnel.public_url} label="Copy tunnel URL" />
              <IconButton
                icon={Power}
                label="Stop tunnel"
                variant="danger"
                onClick={() => handleStopTunnel(s)}
              />
            </span>
          );
        }
        if (tunnel) {
          return (
            <span className="dt-cell-inline">
              <StatusPill variant="pending" label="starting" />
              <IconButton
                icon={Power}
                label="Stop tunnel"
                variant="danger"
                onClick={() => handleStopTunnel(s)}
              />
            </span>
          );
        }
        const blocked = !s.enabled || tunnelCli.data?.found === false;
        return (
          <Button
            size="sm"
            variant="secondary"
            disabled={blocked}
            title={
              !s.enabled
                ? "Enable this site first"
                : tunnelCli.data?.found === false
                  ? // Now actionable: cloudflared is in the Apps catalog.
                    "cloudflared was not found — install it from the Apps page"
                  : "Start a public tunnel to this site"
            }
            onClick={() => handleStartTunnel(s)}
          >
            Start
          </Button>
        );
      },
    },
    {
      key: "backend",
      header: "Backend",
      priority: 3,
      wrap: true,
      sortValue: (s) => backendLabel(s),
      render: (s) => (
        <span className="dt-cell-inline">
          <Badge tone="neutral">{s.backend.type}</Badge>
          <Mono truncate={280}>{backendLabel(s)}</Mono>
        </span>
      ),
    },
    {
      key: "ssl",
      header: "SSL expires",
      priority: 3,
      mono: true,
      sortValue: (s) => s.ssl_not_after ?? "",
      render: (s) => s.ssl_not_after ?? "—",
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (s) => (
        /* Was up to six inline buttons — the widest action column in the app.
           Open and Enable stay; the rest move into the overflow. */
        <ActionGroup>
          <IconButton
            icon={ExternalLink}
            label="Open in browser"
            onClick={() => void openUrl(`https://${s.hostname}`)}
          />
          <IconButton
            icon={Power}
            label={s.enabled ? "Disable" : "Enable"}
            onClick={() => handleToggle(s)}
          />
          <Menu
            items={[
              ...(s.backend.type === "laravel"
                ? [
                    {
                      key: "perms",
                      label: "Fix permissions",
                      icon: Wrench,
                      onClick: () => handleFixPermissions(s),
                    },
                    {
                      key: "php",
                      label: "PHP version…",
                      icon: FileCode,
                      onClick: () => setPhpModalSite(s),
                    },
                    {
                      key: "node",
                      label: "Node version…",
                      icon: Hexagon,
                      onClick: () => setNodeModalSite(s),
                    },
                  ]
                : []),
              /* Outside the laravel-only group: a proxied site still gets
                 client_max_body_size, the read timeout and raw directives. */
              {
                key: "limits",
                label: "Limits…",
                icon: SlidersHorizontal,
                onClick: () => setLimitsModalSite(s),
              },
              {
                key: "reissue",
                label: "Reissue certificate",
                icon: ShieldCheck,
                onClick: () => handleReissueCert(s),
              },
              {
                key: "delete",
                label: "Delete site",
                icon: Trash2,
                danger: true,
                separatorBefore: true,
                onClick: () => handleRemove(s),
              },
            ]}
          />
        </ActionGroup>
      ),
    },
  ];

  return (
    <Stack gap={5}>
      {message && (
        <Callout tone="success" onDismiss={() => setMessage(null)}>
          {message}
        </Callout>
      )}
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}

      <Section
        title="Sites"
        padding="none"
        actions={
          <>
            <Button
              variant="secondary"
              icon={FolderPlus}
              onClick={() => setShowNewProjectModal(true)}
            >
              New project
            </Button>
            <Button variant="primary" icon={Plus} onClick={() => setShowAddModal(true)}>
              Add site
            </Button>
          </>
        }
        isEmpty={!sites.loading && rows.length === 0}
        empty={{
          icon: Globe,
          title: "No sites yet",
          description:
            "Add a site to get an nginx vhost and a locally-trusted HTTPS certificate.",
          action: (
            <Button variant="primary" icon={Plus} onClick={() => setShowAddModal(true)}>
              Add site
            </Button>
          ),
        }}
      >
        <DataTable
          columns={columns}
          rows={rows}
          rowKey={(s) => s.id}
          loading={sites.loading}
          defaultSort={{ key: "name", dir: "asc" }}
          minWidth={620}
          caption="Configured nginx sites"
        />
      </Section>

      {showAddModal && (
        <Modal size="md" title="Add a site" icon={Plus} onClose={() => setShowAddModal(false)}>
          <AddSiteForm
            onAdded={() => {
              setShowAddModal(false);
              void sites.refresh();
            }}
            onCancel={() => setShowAddModal(false)}
          />
        </Modal>
      )}

      {showNewProjectModal && (
        <Modal
          size="md"
          title="New Laravel project"
          icon={FolderPlus}
          onClose={() => setShowNewProjectModal(false)}
        >
          <NewProjectForm
            onCreated={() => {
              setShowNewProjectModal(false);
              void sites.refresh();
            }}
            onCancel={() => setShowNewProjectModal(false)}
          />
        </Modal>
      )}

      {phpModalSite && (
        <PhpVersionModal
          site={phpModalSite}
          onClose={() => setPhpModalSite(null)}
          onSaved={() => {
            setPhpModalSite(null);
            void sites.refresh();
          }}
        />
      )}

      {limitsModalSite && (
        <SiteLimitsModal
          site={limitsModalSite}
          onClose={() => setLimitsModalSite(null)}
          onSaved={() => {
            setLimitsModalSite(null);
            void sites.refresh();
          }}
        />
      )}

      {nodeModalSite && (
        <NodeVersionModal
          site={nodeModalSite}
          onClose={() => setNodeModalSite(null)}
          onWritten={(msg) => {
            setNodeModalSite(null);
            setMessage(msg);
          }}
        />
      )}
    </Stack>
  );
}
