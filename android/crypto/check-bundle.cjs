const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const output = path.resolve(__dirname, '../app/build/generated/cryptoAssets/crypto');
const runtime = fs.readFileSync(path.join(output, 'runtime.js'), 'utf8');
// A Windows alias separator mismatch once duplicated this module and orphaned every SDK response.
assert.equal((runtime.match(/window\.cryptoReply\s*=/g) ?? []).length, 1, 'Bundle must contain exactly one native API reply/pending-map owner');
assert.equal((runtime.match(/NativeCrypto\.context\(/g) ?? []).length, 1, 'Bundle must use one immutable account/origin context');
const html = fs.readFileSync(path.join(output, 'index.html'), 'utf8');
assert.match(html, /default-src 'none'/);
assert.match(html, /script-src 'self' 'wasm-unsafe-eval'/);
assert.ok(fs.statSync(path.join(output, 'matrix_sdk_crypto_wasm_bg.wasm')).size > 1024 * 1024, 'Official WASM is a packaged local resource');
console.log('PASS: Android bundle has one account-bound API owner, strict CSP and local Matrix WASM');
