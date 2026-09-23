import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import { Select } from "./Select";

const options = Array.from({ length: 20 }, (_, i) => ({
  value: `opt-${i}`,
  label: `Option ${i}`,
}));

function open(props: Partial<React.ComponentProps<typeof Select>> = {}) {
  const onChange = vi.fn();
  render(
    <Select
      id="s"
      aria-label="Thing"
      value=""
      options={options}
      onChange={onChange}
      {...props}
    />,
  );
  fireEvent.click(screen.getByLabelText("Thing"));
  return { listbox: screen.getByRole("listbox"), onChange };
}

const optionLabels = () =>
  screen.queryAllByRole("option").map((o) => o.textContent);

describe("Select", () => {
  /**
   * Regression: the listbox has its own max-height and scrollbar, and the
   * close-on-scroll listener is in the capture phase — so scrolling the option
   * list closed it, making anything past the first few options unreachable.
   */
  it("stays open while its own option list is scrolled", () => {
    const { listbox } = open();
    fireEvent.scroll(listbox);
    expect(screen.queryByRole("listbox")).not.toBeNull();
  });

  it("still closes when an ancestor scrolls out from under it", () => {
    open();
    fireEvent.scroll(document, {});
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  describe("searchable", () => {
    it("has no search box unless asked for one", () => {
      open();
      expect(screen.queryByRole("combobox")).toBeNull();
    });

    it("narrows the options as you type", () => {
      open({ searchable: true });
      fireEvent.change(screen.getByRole("combobox"), {
        target: { value: "ion 1" },
      });
      // Option 1 and 10-19, not Option 2.
      expect(optionLabels()).toEqual([
        "Option 1",
        ...Array.from({ length: 10 }, (_, i) => `Option 1${i}`),
      ]);
    });

    it("says so rather than showing an empty box when nothing matches", () => {
      open({ searchable: true });
      fireEvent.change(screen.getByRole("combobox"), {
        target: { value: "zzz" },
      });
      expect(optionLabels()).toEqual([]);
      expect(screen.getByText("No matches")).toBeTruthy();
    });

    /** A space has to stay typeable — it is not "pick the active option" here. */
    it("keeps space as a character inside the search box", () => {
      const { onChange } = open({ searchable: true });
      const search = screen.getByRole("combobox");
      fireEvent.keyDown(search, { key: " " });
      expect(onChange).not.toHaveBeenCalled();
      expect(screen.queryByRole("listbox")).not.toBeNull();
    });

    it("picks what the arrow keys walk through, not the unfiltered list", () => {
      const { onChange } = open({ searchable: true });
      const search = screen.getByRole("combobox");
      fireEvent.change(search, { target: { value: "ion 19" } });
      fireEvent.keyDown(search, { key: "Enter" });
      expect(onChange).toHaveBeenCalledWith("opt-19");
    });
  });
});
