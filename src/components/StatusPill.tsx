export default function StatusPill({ ok, label }: { ok: boolean; label: string }) {
  return <span className={`status-pill ${ok ? "status-ok" : "status-bad"}`}>{label}</span>;
}
