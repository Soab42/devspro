import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

import type {
  ConnectionInput,
  ContainerCredentials,
  SavedConnection,
} from "../api";

const listConnections = vi.fn<() => Promise<SavedConnection[]>>();
const saveConnection = vi.fn<(i: ConnectionInput) => Promise<unknown>>(
  async () => undefined,
);
const testConnection = vi.fn<(i: ConnectionInput) => Promise<string>>(
  async () => "connected",
);
const inspectContainer = vi.fn<(n: string) => Promise<ContainerCredentials>>();

vi.mock("../api", () => ({
  listConnections: () => listConnections(),
  saveConnection: (i: ConnectionInput) => saveConnection(i),
  deleteConnection: vi.fn(async () => undefined),
  testConnection: (i: ConnectionInput) => testConnection(i),
  inspectContainer: (n: string) => inspectContainer(n),
}));

const CONTAINERS = [
  { name: "app-web-1", image: "brokercore-web", state: "running" },
  { name: "app-db-1", image: "postgres:16", state: "running" },
  { name: "legacy-mysql-1", image: "mysql:8", state: "exited" },
  { name: "app-cache-1", image: "redis:7-alpine", state: "running" },
];

vi.mock("../../docker/api", () => ({
  listContainers: vi.fn(async () => CONTAINERS),
}));

vi.mock("../../../app/ConfirmContext", () => ({
  useConfirm: () => vi.fn(async () => true),
}));

import ConnectionsModal from "./ConnectionsModal";

const saved: SavedConnection = {
  id: "conn-1",
  name: "Staging",
  kind: "mysql",
  transport: "tcp",
  host: "db.example.com",
  port: 3306,
  container: "",
  username: "app",
  has_password: true,
  default_database: "shop",
  created_at: "2026-01-01T00:00:00Z",
};

function openForm() {
  render(<ConnectionsModal onClose={vi.fn()} onChanged={vi.fn()} />);
}

describe("ConnectionsModal", () => {
  beforeEach(() => {
    listConnections.mockReset().mockResolvedValue([saved]);
    saveConnection.mockClear();
    testConnection.mockClear();
    inspectContainer.mockReset().mockResolvedValue({
      kind: "postgres",
      username: "appuser",
      password: "from-the-container",
      database: "appdb",
    });
  });

  /** Opens the add form and switches it to the Docker source. */
  async function addViaDocker() {
    render(<ConnectionsModal onClose={vi.fn()} onChanged={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    fireEvent.click(screen.getByRole("radio", { name: /Docker/ }));
    return waitFor(() => expect(screen.getByLabelText(/^Container/)).toBeTruthy());
  }

  const containerOptions = () =>
    screen.queryAllByRole("option").map((o) => o.textContent ?? "");

  it("lists a saved connection with where it points, not just its name", async () => {
    openForm();
    expect(await screen.findByText("Staging")).toBeTruthy();
    expect(screen.getByText("db.example.com:3306")).toBeTruthy();
  });

  /**
   * The edit form starts with a blank password field, which means "keep the
   * stored one". Sending "" there instead of null would wipe a credential the
   * UI never showed and cannot restore.
   */
  it("sends no password when the field was never touched on an edit", async () => {
    openForm();
    fireEvent.click(await screen.findByLabelText("Edit connection"));
    fireEvent.click(await screen.findByRole("button", { name: "Save" }));

    await waitFor(() => expect(saveConnection).toHaveBeenCalled());
    const input = saveConnection.mock.calls[0][0];
    expect(input.password).toBeNull();
    expect(input.id).toBe("conn-1");
  });

  it("sends the typed password once the field is touched", async () => {
    openForm();
    fireEvent.click(await screen.findByLabelText("Edit connection"));
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "new-secret" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(saveConnection).toHaveBeenCalled());
    expect(saveConnection.mock.calls[0][0].password).toBe("new-secret");
  });

  it("tests the form as typed, before anything is saved", async () => {
    openForm();
    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    fireEvent.change(screen.getByLabelText(/^Name/), {
      target: { value: "Prod" },
    });
    fireEvent.change(screen.getByLabelText(/^Host/), {
      target: { value: "10.0.0.5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Test" }));

    await waitFor(() => expect(testConnection).toHaveBeenCalled());
    expect(testConnection.mock.calls[0][0]).toMatchObject({
      id: null,
      name: "Prod",
      host: "10.0.0.5",
      transport: "tcp",
    });
    expect(saveConnection).not.toHaveBeenCalled();
  });

  /** A new connection defaults to the engine's own port and admin user. */
  it("falls back to the engine defaults for port and username", async () => {
    openForm();
    fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
    fireEvent.change(screen.getByLabelText(/^Name/), {
      target: { value: "Prod" },
    });
    fireEvent.change(screen.getByLabelText(/^Host/), {
      target: { value: "10.0.0.5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => expect(saveConnection).toHaveBeenCalled());
    expect(saveConnection.mock.calls[0][0]).toMatchObject({
      port: 3306,
      username: "root",
    });
  });

  /**
   * Every container on the machine used to be offered, burying the one or two
   * that are actually databases.
   */
  it("offers only containers whose image looks like the chosen engine", async () => {
    await addViaDocker();
    fireEvent.click(screen.getByLabelText(/^Container/));

    await waitFor(() => expect(containerOptions().length).toBeGreaterThan(0));
    const labels = containerOptions().join(" ");
    expect(labels).toContain("legacy-mysql-1");
    expect(labels).not.toContain("app-web-1");
    expect(labels).not.toContain("app-cache-1");
    // MySQL is the default engine, so the postgres one is out too.
    expect(labels).not.toContain("app-db-1");
  });

  it("can be told to show every container, for a database in a custom image", async () => {
    await addViaDocker();
    fireEvent.click(screen.getByLabelText(/Show all containers/));
    fireEvent.click(screen.getByLabelText(/^Container/));

    await waitFor(() =>
      expect(containerOptions().length).toBe(CONTAINERS.length),
    );
    expect(containerOptions().join(" ")).toContain("app-web-1");
  });

  it("fills the credentials in from the container's own environment", async () => {
    await addViaDocker();
    fireEvent.click(screen.getByLabelText(/^Container/));
    fireEvent.click(await screen.findByText(/legacy-mysql-1/));

    await waitFor(() => expect(inspectContainer).toHaveBeenCalledWith("legacy-mysql-1"));
    await waitFor(() =>
      expect(screen.getByLabelText("Username")).toHaveProperty(
        "value",
        "appuser",
      ),
    );
    expect(screen.getByLabelText("Password")).toHaveProperty(
      "value",
      "from-the-container",
    );
    expect(screen.getByLabelText("Default database")).toHaveProperty(
      "value",
      "appdb",
    );
    // A blank name gets the container's, rather than staying empty.
    expect(screen.getByLabelText(/^Name/)).toHaveProperty(
      "value",
      "legacy-mysql-1",
    );
  });

  it("saves the auto-filled password rather than treating it as untouched", async () => {
    await addViaDocker();
    fireEvent.click(screen.getByLabelText(/^Container/));
    fireEvent.click(await screen.findByText(/legacy-mysql-1/));
    await waitFor(() => expect(inspectContainer).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(saveConnection).toHaveBeenCalled());
    expect(saveConnection.mock.calls[0][0]).toMatchObject({
      transport: "docker",
      container: "legacy-mysql-1",
      password: "from-the-container",
      kind: "postgres",
    });
  });
});
