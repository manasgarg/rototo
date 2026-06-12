import { Github, TriangleAlert } from "lucide-react";
import { redirect } from "next/navigation";
import { RototoMark } from "@/components/rototo-mark";
import { currentUser } from "@/lib/auth";
import { tokenEncryptionConfigError } from "@/lib/token-encryption";

export default async function LoginPage() {
  const user = await currentUser();
  if (user) {
    redirect("/app");
  }
  const missingConfig = [
    !process.env.GITHUB_CLIENT_ID ? "GITHUB_CLIENT_ID" : null,
    !process.env.GITHUB_CLIENT_SECRET ? "GITHUB_CLIENT_SECRET" : null,
    !process.env.ROTOTO_ADMIN_TOKEN_ENCRYPTION_KEY
      ? "ROTOTO_ADMIN_TOKEN_ENCRYPTION_KEY"
      : null,
  ].filter(Boolean);
  const tokenEncryptionError = process.env.ROTOTO_ADMIN_TOKEN_ENCRYPTION_KEY
    ? tokenEncryptionConfigError()
    : null;
  const configured = missingConfig.length === 0 && !tokenEncryptionError;

  return (
    <main className="login-page">
      <section className="login-panel">
        <div className="brand">
          <span className="brand-mark">
            <RototoMark size={30} />
          </span>
          <span className="brand-name">rototo</span>
          <span className="brand-tag label">admin</span>
        </div>
        <div className="section-header-text">
          <h1 className="login-title">Sign in</h1>
          <p className="hint">
            rototo admin reads workspaces from the GitHub repositories your account can
            already access. Edits land on draft branches and ship as pull requests —
            nothing merges without review.
          </p>
        </div>
        {configured ? (
          <a className="btn btn-primary" href="/api/auth/github/start">
            <Github aria-hidden size={16} />
            Continue with GitHub
          </a>
        ) : (
          <div className="banner banner-warn">
            <TriangleAlert aria-hidden size={16} />
            <span>
              OAuth is not configured.{" "}
              {missingConfig.length > 0 ? (
                <>
                  Set <code>{missingConfig.join(", ")}</code> in{" "}
                  <code>apps/admin/.env.local</code>.
                </>
              ) : (
                tokenEncryptionError
              )}
            </span>
          </div>
        )}
      </section>
    </main>
  );
}
