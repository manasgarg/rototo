import { useCallback } from "react";
import { useNavigate } from "react-router";

import { useRefresh } from "./refresh";

/* next/navigation's useRouter surface, mapped onto react-router: push
   navigates, refresh re-fetches the active screen's data through the
   RefreshContext the screen provides. */
export function useRouter(): {
    push: (href: string) => void;
    refresh: () => void;
} {
    const navigate = useNavigate();
    const refresh = useRefresh();
    const push = useCallback((href: string) => void navigate(href), [navigate]);
    return { push, refresh };
}
