import { describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useResource } from "./useResource";

describe("useResource", () => {
  it("reports loading only until the first result arrives", async () => {
    const { result } = renderHook(() => useResource(async () => "value"));
    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.data).toBe("value"));
    expect(result.current.loading).toBe(false);
  });

  /**
   * The regression this guards: every polling page previously flipped one
   * `loading` flag per tick, so Dashboard (2s), Workers (5s) and the Sites
   * tunnel poll (2s) tore their content down several times a minute. `loading`
   * must mean "nothing to show yet", never "a request is in flight".
   */
  it("stays out of loading across subsequent fetches", async () => {
    const fetcher = vi.fn(async () => "value");
    const { result } = renderHook(() => useResource(fetcher));
    await waitFor(() => expect(result.current.data).toBe("value"));

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.loading).toBe(false);
    expect(result.current.data).toBe("value");
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it("distinguishes refreshing from loading", async () => {
    let release!: (v: string) => void;
    const fetcher = vi
      .fn<() => Promise<string>>()
      .mockResolvedValueOnce("first")
      .mockImplementationOnce(() => new Promise((resolve) => (release = resolve)));

    const { result } = renderHook(() => useResource(fetcher));
    await waitFor(() => expect(result.current.data).toBe("first"));

    let pending!: Promise<void>;
    act(() => {
      pending = result.current.refresh();
    });
    await waitFor(() => expect(result.current.refreshing).toBe(true));
    // Data stays on screen while the refresh is in flight.
    expect(result.current.loading).toBe(false);
    expect(result.current.data).toBe("first");

    await act(async () => {
      release("second");
      await pending;
    });
    expect(result.current.refreshing).toBe(false);
    expect(result.current.data).toBe("second");
  });

  it("captures an error message on failure", async () => {
    const { result } = renderHook(() =>
      useResource(async () => {
        throw new Error("daemon unreachable");
      }),
    );
    await waitFor(() => expect(result.current.error).toBe("daemon unreachable"));
    expect(result.current.data).toBeNull();
  });

  it("keeps prior data when a later fetch fails", async () => {
    const fetcher = vi
      .fn<() => Promise<string>>()
      .mockResolvedValueOnce("good")
      .mockRejectedValueOnce(new Error("transient"));

    const { result } = renderHook(() => useResource(fetcher));
    await waitFor(() => expect(result.current.data).toBe("good"));

    await act(async () => {
      await result.current.refresh();
    });

    // A blip in a poll must not blank a working page.
    expect(result.current.data).toBe("good");
    expect(result.current.error).toBe("transient");
    expect(result.current.loading).toBe(false);
  });

  it("does not fetch while disabled", async () => {
    const fetcher = vi.fn(async () => "value");
    renderHook(() => useResource(fetcher, { enabled: false }));
    await new Promise((r) => setTimeout(r, 20));
    expect(fetcher).not.toHaveBeenCalled();
  });
});
