// Encrypts stored GitHub tokens at rest, byte-compatible with the Rust
// console's token_crypto: AES-256-GCM, ciphertext rows shaped
// `rototo-console-token-v1.<nonce>.<tag>.<ciphertext>` with base64url
// (unpadded) parts and the format string as associated data. Same key
// semantics: `ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY` accepts `base64:`,
// `hex:`, bare 64-char hex, or bare base64, decoding to exactly 32 bytes.

import { createCipheriv, createDecipheriv, randomBytes } from "node:crypto";

import { TOKEN_ENCRYPTION_KEY_ENV } from "./config.ts";

const TOKEN_FORMAT = "rototo-console-token-v1";
const NONCE_LEN = 12;
const TAG_LEN = 16;

export class TokenCrypto {
    private readonly key: Buffer;

    private constructor(key: Buffer) {
        this.key = key;
    }

    static fromEnvValue(raw: string): TokenCrypto {
        return new TokenCrypto(decodeKey(raw.trim()));
    }

    static generate(): TokenCrypto {
        return new TokenCrypto(randomBytes(32));
    }

    keyBase64(): string {
        return `base64:${this.key.toString("base64")}`;
    }

    encrypt(token: string): string {
        const nonce = randomBytes(NONCE_LEN);
        const cipher = createCipheriv("aes-256-gcm", this.key, nonce);
        cipher.setAAD(Buffer.from(TOKEN_FORMAT));
        const ciphertext = Buffer.concat([
            cipher.update(token, "utf8"),
            cipher.final(),
        ]);
        const tag = cipher.getAuthTag();
        return [
            TOKEN_FORMAT,
            nonce.toString("base64url"),
            tag.toString("base64url"),
            ciphertext.toString("base64url"),
        ].join(".");
    }

    decrypt(encrypted: string): string {
        const parts = encrypted.split(".");
        if (parts.length !== 4) {
            throw new Error(
                "GitHub token is not stored in the supported encrypted format",
            );
        }
        const [format, noncePart, tagPart, ciphertextPart] = parts as [
            string,
            string,
            string,
            string,
        ];
        if (
            format !== TOKEN_FORMAT ||
            noncePart.length === 0 ||
            tagPart.length === 0 ||
            ciphertextPart.length === 0
        ) {
            throw new Error(
                "GitHub token is not stored in the supported encrypted format",
            );
        }
        const nonce = Buffer.from(noncePart, "base64url");
        const tag = Buffer.from(tagPart, "base64url");
        if (nonce.length !== NONCE_LEN || tag.length !== TAG_LEN) {
            throw new Error("stored token nonce or tag has the wrong length");
        }
        const decipher = createDecipheriv("aes-256-gcm", this.key, nonce);
        decipher.setAAD(Buffer.from(TOKEN_FORMAT));
        decipher.setAuthTag(tag);
        try {
            return Buffer.concat([
                decipher.update(Buffer.from(ciphertextPart, "base64url")),
                decipher.final(),
            ]).toString("utf8");
        } catch {
            throw new Error("stored token failed to decrypt");
        }
    }
}

function decodeKey(raw: string): Buffer {
    if (raw.length === 0) {
        throw new Error(
            `${TOKEN_ENCRYPTION_KEY_ENV} is required before GitHub sign-in`,
        );
    }
    let bytes: Buffer;
    if (raw.startsWith("base64:")) {
        bytes = decodeBase64(raw.slice("base64:".length));
    } else if (raw.startsWith("hex:")) {
        bytes = decodeHex(raw.slice("hex:".length));
    } else if (raw.length === 64 && /^[0-9a-fA-F]+$/.test(raw)) {
        bytes = decodeHex(raw);
    } else {
        bytes = decodeBase64(raw);
    }
    if (bytes.length !== 32) {
        throw new Error(
            `${TOKEN_ENCRYPTION_KEY_ENV} must decode to exactly 32 bytes`,
        );
    }
    return bytes;
}

function decodeBase64(raw: string): Buffer {
    const bytes = Buffer.from(raw, "base64");
    // Buffer.from silently tolerates garbage; require a round trip.
    if (
        bytes.toString("base64").replace(/=+$/, "") !== raw.replace(/=+$/, "")
    ) {
        throw new Error(`${TOKEN_ENCRYPTION_KEY_ENV} is not valid base64`);
    }
    return bytes;
}

function decodeHex(raw: string): Buffer {
    if (raw.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(raw)) {
        throw new Error(`${TOKEN_ENCRYPTION_KEY_ENV} is not valid hex`);
    }
    return Buffer.from(raw, "hex");
}
