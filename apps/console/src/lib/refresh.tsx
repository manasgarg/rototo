import { type ReactNode, createContext, useContext } from "react";

/* Screens own their data; mutation components ask the nearest screen to
   reload after a save. This replaces Next's router.refresh(). */
const RefreshContext = createContext<() => void>(() => {});

export function RefreshScope({
  children,
  onRefresh,
}: {
  children: ReactNode;
  onRefresh: () => void;
}) {
  return <RefreshContext.Provider value={onRefresh}>{children}</RefreshContext.Provider>;
}

export function useRefresh(): () => void {
  return useContext(RefreshContext);
}
