import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { forgetRememberedState, useRemembered } from "./pageMemory";

function Counter() {
  const [n, setN] = useRemembered("test:counter", 0);
  return <button onClick={() => setN(n + 1)}>{`count ${n}`}</button>;
}

describe("useRemembered", () => {
  it("gives a page back what it held when it was left", () => {
    forgetRememberedState();
    const first = render(<Counter />);
    fireEvent.click(screen.getByRole("button"));
    fireEvent.click(screen.getByRole("button"));
    first.unmount();

    render(<Counter />);
    expect(screen.getByRole("button").textContent).toBe("count 2");
  });
});
