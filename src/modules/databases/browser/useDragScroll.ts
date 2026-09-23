import { useCallback, useEffect, useRef } from "react";

/**
 * Drag anywhere in the grid to scroll it sideways.
 *
 * A table with forty columns is mostly off-screen, and reaching the horizontal
 * scrollbar at the bottom of a tall grid means travelling the whole height of
 * it first. Dragging the content is how every map and canvas already works.
 *
 * Pointer events, not mouse events, so a trackpad or a pen behaves the same;
 * and `setPointerCapture` so a drag that leaves the element still tracks.
 */
export function useDragScroll<T extends HTMLElement>() {
  const ref = useRef<T | null>(null);
  const drag = useRef<{ id: number; x: number; left: number } | null>(null);

  const onPointerDown = useCallback((e: React.PointerEvent<T>) => {
    // Only a plain primary-button drag on empty space. A drag that starts on a
    // control is that control's business — a text selection in a cell, a
    // checkbox, the header's sort button.
    if (e.button !== 0 || e.pointerType === "touch") return;
    const target = e.target as HTMLElement;
    if (target.closest("button, a, input, select, textarea, [role='button']")) return;
    // A draggable header is being reordered, not panned.
    if (target.closest("[draggable='true']")) return;

    const element = ref.current;
    if (!element || element.scrollWidth <= element.clientWidth) return;

    drag.current = { id: e.pointerId, x: e.clientX, left: element.scrollLeft };
    element.setPointerCapture(e.pointerId);
    element.classList.add("is-dragging");
  }, []);

  const onPointerMove = useCallback((e: React.PointerEvent<T>) => {
    const state = drag.current;
    const element = ref.current;
    if (!state || !element || state.id !== e.pointerId) return;
    e.preventDefault();
    element.scrollLeft = state.left - (e.clientX - state.x);
  }, []);

  const end = useCallback((e: React.PointerEvent<T>) => {
    const element = ref.current;
    if (!drag.current || !element) return;
    if (element.hasPointerCapture(e.pointerId)) {
      element.releasePointerCapture(e.pointerId);
    }
    element.classList.remove("is-dragging");
    drag.current = null;
  }, []);

  // A pointer lost mid-drag (window blur, a devtools open) would otherwise
  // leave the grid stuck in its dragging state.
  useEffect(() => {
    const element = ref.current;
    return () => {
      element?.classList.remove("is-dragging");
      drag.current = null;
    };
  }, []);

  return {
    ref,
    handlers: {
      onPointerDown,
      onPointerMove,
      onPointerUp: end,
      onPointerCancel: end,
    },
  };
}
