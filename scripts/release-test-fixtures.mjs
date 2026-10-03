import {
  createHash,
  generateKeyPairSync,
  randomBytes,
  sign,
} from "node:crypto";

export function signerFixture(message, algorithm = "ED", version = "0.5.21") {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const keyId = randomBytes(8);
  const publicBytes = publicKey
    .export({ type: "spki", format: "der" })
    .subarray(-32);
  const record = Buffer.concat([Buffer.from("Ed"), keyId, publicBytes]);
  const publicText = `untrusted comment: test public key\n${record.toString("base64")}\n`;
  const payload =
    algorithm === "ED"
      ? createHash("blake2b512").update(message).digest()
      : message;
  const signature = sign(null, payload, privateKey);
  const trustedComment = `timestamp:1\tfile:test.exe\tversion:${version}`;
  const global = sign(
    null,
    Buffer.concat([signature, Buffer.from(trustedComment)]),
    privateKey,
  );
  const signatureRecord = Buffer.concat([
    Buffer.from(algorithm),
    keyId,
    signature,
  ]);
  const signatureText = `untrusted comment: test signature\n${signatureRecord.toString("base64")}\ntrusted comment: ${trustedComment}\n${global.toString("base64")}\n`;
  return {
    publicKey: Buffer.from(publicText).toString("base64"),
    signature: Buffer.from(signatureText).toString("base64"),
  };
}
