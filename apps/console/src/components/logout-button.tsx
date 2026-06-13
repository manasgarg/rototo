import { LogOut } from "lucide-react";

import { apiFetch } from "@/lib/api";

export function LogoutButton() {
  const logout = async () => {
    try {
      await apiFetch("/api/auth/logout", { method: "POST", body: "{}" });
    } finally {
      // Full reload so every in-memory session state resets.
      window.location.href = "/login";
    }
  };
  return (
    <button className="btn btn-ghost btn-icon" onClick={logout} title="Sign out" type="button">
      <LogOut aria-hidden size={15} />
    </button>
  );
}
