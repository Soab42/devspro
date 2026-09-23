import { createContext, useContext } from "react";

/**
 * What the window around a Page offers it. A popped-out module window has a
 * top bar with the module's name already in it, so the page's header would
 * only repeat that; the shell hands the page a slot in that bar instead, and
 * Page moves its meta and actions there. The main window provides nothing
 * and pages render their own header as usual.
 */
export interface PageChrome {
  /** Where a page's meta and actions go instead of its own header. */
  actionsSlot: HTMLElement | null;
}

export const PageChromeContext = createContext<PageChrome | null>(null);

export function usePageChrome(): PageChrome | null {
  return useContext(PageChromeContext);
}
