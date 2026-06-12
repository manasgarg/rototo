import {
  createCipheriv,
  createDecipheriv,
  randomBytes,
} from "node:crypto";

const TOKEN_FORMAT = "rototo-admin-token-v1";
const KEY_ENV = "ROTOTO_ADMIN_TOKEN_ENCRYPTION_KEY";
const NONCE_BYTES = 12;

export function encryptToken(token: string): string {
  const key = tokenEncryptionKey();
  const nonce = randomBytes(NONCE_BYTES);
  const cipher = createCipheriv("aes-256-gcm", key, nonce);
  cipher.setAAD(Buffer.from(TOKEN_FORMAT));
  const ciphertext = Buffer.concat([cipher.update(token, "utf8"), cipher.final()]);
  const tag = cipher.getAuthTag();
  return [
    TOKEN_FORMAT,
    nonce.toString("base64url"),
    tag.toString("base64url"),
    ciphertext.toString("base64url"),
  ].join(".");
}

export function decryptToken(encryptedToken: string): string {
  const [format, nonce, tag, ciphertext] = encryptedToken.split(".");
  if (format !== TOKEN_FORMAT || !nonce || !tag || !ciphertext) {
    throw new Error("GitHub token is not stored in the supported encrypted format");
  }

  const decipher = createDecipheriv(
    "aes-256-gcm",
    tokenEncryptionKey(),
    Buffer.from(nonce, "base64url"),
  );
  decipher.setAAD(Buffer.from(TOKEN_FORMAT));
  decipher.setAuthTag(Buffer.from(tag, "base64url"));
  return Buffer.concat([
    decipher.update(Buffer.from(ciphertext, "base64url")),
    decipher.final(),
  ]).toString("utf8");
}

export function tokenEncryptionConfigError(): string | null {
  try {
    tokenEncryptionKey();
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : "GitHub token encryption is not configured";
  }
}

function tokenEncryptionKey(): Buffer {
  const raw = process.env[KEY_ENV]?.trim();
  if (!raw) {
    throw new Error(`${KEY_ENV} is required before GitHub sign-in`);
  }

  const key = decodeKey(raw);
  if (key.length !== 32) {
    throw new Error(`${KEY_ENV} must decode to exactly 32 bytes`);
  }
  return key;
}

function decodeKey(raw: string): Buffer {
  if (raw.startsWith("base64:")) {
    return Buffer.from(raw.slice("base64:".length), "base64");
  }
  if (raw.startsWith("hex:")) {
    return Buffer.from(raw.slice("hex:".length), "hex");
  }
  if (/^[0-9a-f]{64}$/i.test(raw)) {
    return Buffer.from(raw, "hex");
  }
  return Buffer.from(raw, "base64");
}
