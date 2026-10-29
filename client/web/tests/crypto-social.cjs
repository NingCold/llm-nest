const assert = require("node:assert/strict"), fs = require("node:fs"), vm = require("node:vm"), ts = require("typescript"), sdk = require("@matrix-org/matrix-sdk-crypto-wasm");
function load(file, imports) {
  const exports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(require.resolve(file), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports, TextEncoder, TextDecoder, Uint8Array, DataView, Blob, atob, btoa, crypto: require("node:crypto").webcrypto, require: name => { if (name in imports) return imports[name]; throw Error(name); } });
  return exports;
}
(async () => {
  await sdk.initAsync();
  const engine = {
    encryptAttachment(bytes) { const result = sdk.Attachment.encrypt(bytes); return { data: Buffer.from(result.encryptedData).toString("base64"), mediaEncryptionInfo: result.mediaEncryptionInfo }; },
    decryptAttachment(bytes, info) { return sdk.Attachment.decrypt(new sdk.EncryptedAttachment(bytes, info)); },
  };
  let ciphertext, fetchedPath;
  const attachments = load("../src/cloud/crypto/attachments.ts", { "../api": { downloadBytes: async path => { fetchedPath = path; return ciphertext; } }, "./engine": engine, "../../chat/files": { validateFiles(files) { if (files.some(f => f.byteLength > 10 * 1024 * 1024)) throw Error("size"); } }, "@tauri-apps/api/core": {} });
  const prepared = attachments.prepareAttachments([], [{ id: "file", name: "private.txt", mime: "text/plain", byteLength: 3, data: "YWJj" }]);
  assert.equal(prepared.descriptors[0].name, "private.txt");
  assert.deepEqual(Object.keys(prepared.encryptedAttachments[0]).sort(), ["data", "index"]);
  ciphertext = new Uint8Array(Buffer.from(prepared.encryptedAttachments[0].data, "base64"));
  const message = { sealed: {}, attachments: prepared.descriptors };
  assert.equal(Buffer.from(await attachments.attachmentBytes(message, prepared.descriptors[0], "/cipher")).toString(), "abc");
  await attachments.attachmentBytes(message, prepared.descriptors[0], "/ai/ai/view/conversations/group/messages/message/attachments/0?revision=1&inline=true");
  assert.equal(fetchedPath, "/ai/ai/view/conversations/group/messages/message/attachments/0?revision=1", "sealed image previews fetch opaque ciphertext through the same authorized account view route without asking the server to inline encrypted metadata");
  ciphertext[0] ^= 1;
  await assert.rejects(attachments.attachmentBytes(message, prepared.descriptors[0], "/cipher"), undefined, "SDK hash rejects modified bytes");
  ciphertext[0] ^= 1;
  await assert.rejects(attachments.attachmentBytes(message, { ...prepared.descriptors[0], byteLength: 4 }, "/cipher"), /长度/);
  let descriptor = prepared.descriptors[0];
  const projection = load("../src/cloud/crypto/content.ts", { "../api": { server: "https://fixture.invalid" }, "./engine": { matrixRoom: id => `!${id}:llmn`, matrixUser: id => `@${id}:llmn`, unseal: async () => ({ type: "com.llmn.content.v1", content: { version: 1, server: "https://fixture.invalid", kind: "message", conversationId: "group", messageId: "message", senderId: "human", revision: "1", text: "file", replyTo: null, mentions: [], attachments: [descriptor] } }) } });
  const envelope = { version: 1, roomId: "!group:llmn", sender: "@human:llmn", deviceId: "device", eventId: "$message.1.final", content: { device_id: "device" } };
  const projectedMessage = { id: "message", conversationId: "group", senderId: "human", revision: "1", replyTo: null, mentions: [], contextReference: null, attachments: [{ index: 0, kind: "encrypted" }], sealed: envelope };
  const valid = { ...descriptor };
  for (const name of ["", "  ", "../private", "a\\b", "a:b", "a\u0000b", "a\u0085b", "CON.txt", "a.", "x".repeat(241)]) {
    descriptor = { ...valid, name };
    assert.match((await projection.projectMessage(projectedMessage)).cryptoError, /文件名或类型/, `reject filename ${JSON.stringify(name)}`);
  }
  for (const mime of ["", "x".repeat(256), "é".repeat(128), "text/\nplain", "text/\u0085plain"]) {
    descriptor = { ...valid, mime };
    assert.match((await projection.projectMessage(projectedMessage)).cryptoError, /文件名或类型/);
  }
  descriptor = { ...valid, byteLength: 0, name: undefined };
  assert.equal((await projection.projectMessage(projectedMessage)).cryptoError, undefined, "empty unnamed ordinary files remain supported");
  descriptor = { ...valid, kind: "image", mime: "image/png", byteLength: 0 };
  assert.match((await projection.projectMessage(projectedMessage)).cryptoError, /格式或大小/, "empty images are rejected at the decrypted metadata boundary");
  const calls = [], outputs = [];
  const pages = [{ messages: [{ id: "new", position: "2", text: "latest", attachments: [], senderName: "human", createdAt: "2027-01-02" }], hasMore: true }, { messages: [{ id: "old", position: "1", text: "older", attachments: [], senderName: "human", createdAt: "2027-01-01" }], hasMore: false }];
  const history = load("../src/cloud/crypto/history.ts", { "../api": { request: async path => { calls.push(path); return pages[path.includes("before=") ? 1 : 0]; }, saveBlob: (blob, name) => outputs.push({ blob, name }) }, "./content": { projectPage: async page => page }, "./attachments": attachments, "fflate": require("fflate") });
  const conversation = { id: "group", name: "group" };
  assert.equal(JSON.stringify((await history.completeHistory(conversation)).map(m => m.id)), '["old","new"]');
  assert.ok(calls[1].endsWith("before=2"));
  assert.equal(calls.some(path => path.includes("query=")), false, "complete local search uses unfiltered authorized pages");
  await assert.rejects(history.completeHistory(conversation, () => false), /取消/);
  await history.exportHistory(conversation, "json", true, true);
  assert.equal(outputs[0].name, "chat.zip");
  const zip = new DataView(await outputs[0].blob.arrayBuffer());
  assert.equal(zip.getUint32(0, true), 0x04034b50);
  assert.equal(zip.getUint32(zip.byteLength - 22, true), 0x06054b50);
  pages[1].messages[0].attachments = [{ index: 0, kind: 'file', name: 'private.txt', mime: 'text/plain', byteLength: 3, mediaEncryptionInfo: 'sdk-secret' }];
  await history.exportHistory(conversation, 'json', false, false);
  const metadataExport = JSON.parse(await outputs[1].blob.text());
  assert.equal(metadataExport.messages[0].attachments[0].name, 'private.txt', 'unselected attachment bytes do not remove metadata');
  assert.equal(metadataExport.messages[0].attachments[0].mediaEncryptionInfo, undefined, 'ordinary exports do not include attachment decryption material');
  pages[1].messages[0].cryptoError = "missing history key";
  await assert.rejects(history.exportHistory(conversation, "json", false, false), /无法解密/);
  assert.equal(outputs.length, 2, "incomplete decryptions cannot be exported as complete history");
  console.log("PASS: social ciphertext attachments round trip and reject tampering; local history scans all pages, cancels stale reads and blocks incomplete exports");
})().catch(error => { console.error(error); process.exitCode = 1; });
