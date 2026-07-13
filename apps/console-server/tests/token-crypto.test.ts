import assert from "node:assert/strict";
import { test } from "node:test";

import { TokenCrypto } from "../src/token-crypto.ts";

test("encrypt round trips in the rototo-console-token-v1 format", () => {
    const crypto = TokenCrypto.generate();
    const encrypted = crypto.encrypt("gho_example_token");
    assert.ok(encrypted.startsWith("rototo-console-token-v1."));
    const parts = encrypted.split(".");
    assert.equal(parts.length, 4);
    // Nonce is 12 bytes, tag 16, base64url without padding.
    assert.equal(Buffer.from(parts[1]!, "base64url").length, 12);
    assert.equal(Buffer.from(parts[2]!, "base64url").length, 16);
    assert.equal(crypto.decrypt(encrypted), "gho_example_token");
});

test("decrypt rejects other keys and garbage", () => {
    const crypto = TokenCrypto.generate();
    const other = TokenCrypto.generate();
    const encrypted = crypto.encrypt("gho_example_token");
    assert.throws(() => other.decrypt(encrypted));
    assert.throws(() => crypto.decrypt("not-an-encrypted-token"));
    // Tampering with the ciphertext breaks the GCM tag.
    const parts = encrypted.split(".");
    const tampered = Buffer.from(parts[3]!, "base64url");
    tampered[0] = tampered[0]! ^ 0xff;
    parts[3] = tampered.toString("base64url");
    assert.throws(() => crypto.decrypt(parts.join(".")));
});

test("key forms decode the way the Rust console accepted them", () => {
    const crypto = TokenCrypto.generate();
    const keyBase64 = crypto.keyBase64();
    assert.ok(keyBase64.startsWith("base64:"));
    const round = TokenCrypto.fromEnvValue(keyBase64);
    assert.equal(round.decrypt(crypto.encrypt("tok")), "tok");

    // Bare 64-char hex is accepted; short hex is not a 32-byte key.
    assert.ok(TokenCrypto.fromEnvValue("a".repeat(64)));
    assert.throws(() => TokenCrypto.fromEnvValue("hex:00ff"));
    assert.throws(() => TokenCrypto.fromEnvValue(""));
    assert.throws(() => TokenCrypto.fromEnvValue("base64:!!!"));
});
