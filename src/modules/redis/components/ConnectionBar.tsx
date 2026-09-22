import { useState } from "react";
import { Button, Field, Form, FormGrid, Select, TextInput } from "../../../ui";
import { PlugZap, Unplug } from "../../../ui/icons";
import type { RedisConn } from "../api";
import { parsePort } from "../utils";

const DB_OPTIONS = Array.from({ length: 16 }, (_, i) => ({ value: String(i), label: `db ${i}` }));

/**
 * Host / port / password / db plus Connect. The form is uncontrolled by the
 * page until Connect: editing the host while connected must not re-target the
 * polling in flight, so the page only sees a new `RedisConn` on submit.
 */
export default function ConnectionBar({
  initial,
  connected,
  busy,
  onConnect,
  onDisconnect,
}: {
  initial: RedisConn;
  connected: boolean;
  busy: boolean;
  onConnect: (conn: RedisConn) => void;
  onDisconnect: () => void;
}) {
  const [host, setHost] = useState(initial.host);
  const [port, setPort] = useState(String(initial.port));
  const [password, setPassword] = useState(initial.password ?? "");
  const [db, setDb] = useState(String(initial.db));

  const portValue = parsePort(port);
  const hostValue = host.trim();
  const valid = portValue !== null && hostValue.length > 0;

  function submit() {
    if (!valid || portValue === null) return;
    onConnect({
      host: hostValue,
      port: portValue,
      db: Number(db),
      password: password || undefined,
    });
  }

  return (
    <Form onSubmit={submit}>
      <FormGrid columns={3}>
        <Field label="Host" htmlFor="redis-host">
          <TextInput
            id="redis-host"
            mono
            value={host}
            onChange={(e) => setHost(e.target.value)}
            placeholder="127.0.0.1"
            autoComplete="off"
          />
        </Field>
        <Field
          label="Port"
          htmlFor="redis-port"
          error={port && portValue === null ? "1-65535" : undefined}
        >
          <TextInput
            id="redis-port"
            mono
            inputMode="numeric"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            placeholder="6379"
            invalid={port !== "" && portValue === null}
          />
        </Field>
        <Field label="Database" htmlFor="redis-db">
          <Select id="redis-db" value={db} onChange={setDb} options={DB_OPTIONS} />
        </Field>
        <Field
          label="Password"
          htmlFor="redis-password"
          hint="Optional. Kept in memory only, never saved."
        >
          <TextInput
            id="redis-password"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="off"
          />
        </Field>
        <Field label="&nbsp;">
          {connected ? (
            <Button variant="secondary" icon={Unplug} onClick={onDisconnect}>
              Disconnect
            </Button>
          ) : (
            <Button type="submit" variant="primary" icon={PlugZap} disabled={!valid} loading={busy}>
              Connect
            </Button>
          )}
        </Field>
      </FormGrid>
    </Form>
  );
}
