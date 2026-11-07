import { useState } from "react";
import { CodeBlock, CopyButton, SegmentedControl, Stack } from "../../../ui";
import { Braces, MessageSquare } from "../../../ui/icons";
import { asMessages, isEmptyValue, prettyValue } from "../utils";
import "./LlmTraces.css";

/**
 * An input, output or metadata value. A chat (in any of the shapes `asMessages`
 * knows) reads as messages with a role each; anything else as JSON. Either can
 * be switched to raw JSON and copied.
 */
export function Payload({ label, value }: { label: string; value: unknown }) {
  const messages = asMessages(value);
  const [view, setView] = useState<"chat" | "json">("chat");
  const raw = prettyValue(value);

  if (isEmptyValue(value)) {
    return (
      <div className="llmt-payload">
        <div className="llmt-payload-head">
          <span className="llmt-label">{label}</span>
        </div>
        <span className="u-muted">None</span>
      </div>
    );
  }

  return (
    <div className="llmt-payload">
      <div className="llmt-payload-head">
        <span className="llmt-label">{label}</span>
        <Stack direction="row" gap={2} align="center">
          {messages && (
            <SegmentedControl<"chat" | "json">
              ariaLabel={`${label} view`}
              size="sm"
              value={view}
              onChange={setView}
              options={[
                { value: "chat", label: "Chat", icon: MessageSquare },
                { value: "json", label: "JSON", icon: Braces },
              ]}
            />
          )}
          <CopyButton value={raw} label={`Copy ${label.toLowerCase()}`} size="sm" />
        </Stack>
      </div>
      {messages && view === "chat" ? (
        <div className="llmt-messages">
          {messages.map((m, i) => (
            <div key={i} className={`llmt-message llmt-message-${roleClass(m.role)}`}>
              <div className="llmt-message-role">{m.role}</div>
              {m.text && <div className="llmt-message-text">{m.text}</div>}
              {m.toolCalls !== undefined && <CodeBlock maxHeight="30vh">{prettyValue(m.toolCalls)}</CodeBlock>}
            </div>
          ))}
        </div>
      ) : (
        <CodeBlock maxHeight="45vh">{raw}</CodeBlock>
      )}
    </div>
  );
}

function roleClass(role: string): string {
  switch (role) {
    case "user":
    case "human":
      return "user";
    case "assistant":
    case "ai":
      return "assistant";
    case "system":
    case "developer":
      return "system";
    default:
      return "other";
  }
}
