import { LogOut } from "lucide-react";

export function LogoutButton() {
  return (
    <form action="/api/auth/logout" method="post">
      <button className="btn btn-ghost btn-icon" title="Sign out" type="submit">
        <LogOut aria-hidden size={15} />
      </button>
    </form>
  );
}
