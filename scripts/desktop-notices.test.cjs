const { test } = require('node:test');
const assert = require('node:assert/strict');
const { completeRustNotices, copyrightNotices } = require('./desktop-notices.cjs');

test('native license output passes through without changing text or line endings', () => {
  const original = 'Header\r\n' + '='.repeat(72) + '\nMIT License\nUsed by:\ndpi 0.1.2\nCopyright 2026 Actual Owner\n' + '='.repeat(100) + '\r\nAppendix: <year> and [fullname] are examples.\r\nCopyright [yyyy] [name of copyright owner]\n';
  assert.equal(completeRustNotices(original), original);
});

test('actual placeholder attribution blocks a silently failed cargo-about clarification', () => {
  for (const line of ['Copyright (c) <year> <copyright holders>', 'Copyright (c) <year> <owner>.', 'Copyright (c) [year] [fullname]']) {
    assert.throws(() => completeRustNotices('Header\n' + line + '\r\n'), /Unresolved placeholder/);
  }
});

test('known sigchld gap is identified as metadata, not an invented copyright holder', () => {
  const prefix = '='.repeat(72) + '\nMIT License\nUsed by:\nsigchld 0.2.4\nSource: https://static.crates.io/crates/sigchld/sigchld-0.2.4.crate\nRepository: https://github.com/oconnor663/sigchld.rs\n\nMIT License\n\n';
  const tail = '\n\nPermission body stays unchanged.\r\n' + '='.repeat(100) + '\r\nAppendix';
  const output = completeRustNotices(prefix + 'Copyright (c) <year> <copyright holders>' + tail);
  assert.ok(output.startsWith(prefix));
  assert.ok(output.endsWith(tail));
  assert.match(output, /not a verified copyright-holder statement/);
  assert.match(output, /upstream attribution remains unconfirmed/);
});

test('target attribution includes only its actual fixed-version crate COPYRIGHT files', () => {
  const output = copyrightNotices('utf8_iter 1.0.4\nSource: archive\nRepository: upstream\n');
  assert.match(output, /Copyright Mozilla Foundation/);
  assert.match(output, /Utf8CharIndices/);
  assert.doesNotMatch(output, /Component attribution: gtk/);
  assert.equal(copyrightNotices('unrelated 1.0\nSource: archive\nRepository: upstream\n'), '');
  assert.throws(() => copyrightNotices('utf8_iter 2.0.0\nSource: archive\nRepository: upstream\n'), /Review COPYRIGHT/);
});

test('shared GTK and Rand attribution text is emitted once with every applicable source', () => {
  const entry = (name, version) => `${name} ${version}\nSource: archive\nRepository: upstream\n`;
  const input = entry('gtk', '0.18.2') + entry('glib', '0.18.5') + entry('rand', '0.9.5') + entry('rand_core', '0.9.5');
  const output = copyrightNotices(input);
  const fs = require('node:fs'), path = require('node:path');
  for (const file of ['gtk-rs-COPYRIGHT.txt', 'rand-COPYRIGHT.txt']) {
    const body = fs.readFileSync(path.join(__dirname, 'licenses', file), 'utf8');
    assert.equal(output.split(body).length - 1, 1);
  }
  for (const [name, version] of [['gtk', '0.18.2'], ['glib', '0.18.5'], ['rand', '0.9.5'], ['rand_core', '0.9.5']]) {
    assert.match(output, new RegExp(`Component attribution: ${name} ${version}`));
    assert.ok(output.includes(`https://static.crates.io/crates/${name}/${name}-${version}.crate#COPYRIGHT`));
  }
});
