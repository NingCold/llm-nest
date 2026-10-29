const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { readFileSync } = require('node:fs');
const { generate } = require('./web-notices.cjs');
const directory = path.join(__dirname, '../packaging/licenses');
const overrides = JSON.parse(readFileSync(path.join(directory, 'sources.json')));
const packages = { MIT: [{ name: 'fastdom', versions: ['1.0.12'], license: 'MIT', paths: [directory] }] };
test('version-pinned supplemental text includes copyright and exact source', () => {
  const text = generate(packages, overrides, directory);
  assert.match(text, /Copyright \(c\) 2016 Wilson Page/);
  assert.ok(text.includes(overrides['fastdom@1.0.12'].source));
  assert.throws(() => generate(packages, { ...overrides, 'fastdom@1.0.12': { ...overrides['fastdom@1.0.12'], sha256: '0'.repeat(64) } }, directory), /SHA256 mismatch/);
});
test('missing license text blocks output instead of listing only a license name', () => {
  assert.throws(() => generate(packages, {}, directory), /Missing license text/);
});

test('KaTeX bundled font notices preserve OFL and reserved font names', () => {
  const text = generate({ MIT: [{ name: 'katex', versions: ['0.18.4'], license: 'MIT', paths: [directory] }] }, overrides, directory);
  assert.match(text, /SIL OPEN FONT LICENSE Version 1.1/);
  assert.match(text, /Reserved Font Name KaTeX_Main/);
  assert.match(text, /Design Science/);
});

test('Matrix WASM includes its pinned internal Rust notices and blocks unreviewed upgrades', () => {
  const package = { name: '@matrix-org/matrix-sdk-crypto-wasm', versions: ['18.9.0'], license: 'Apache-2.0', paths: [directory] };
  const text = generate({ 'Apache-2.0': [package] }, overrides, directory);
  assert.match(text, /Mozilla Public License/);
  assert.match(text, /eyeball/);
  assert.match(text, /wasm32-unknown-unknown/);
  assert.throws(() => generate({ 'Apache-2.0': [{ ...package, versions: ['19.0.0'] }] }, overrides, directory), /Missing license text/);
});
