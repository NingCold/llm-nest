import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { prepare } from './prepare-agent.mjs';
test('requires explicit artifact coordinates; verifies version, target and bytes', async () => {
  const work = await mkdtemp(path.join(tmpdir(), 'llmn-agent-artifact-'));
  const binary = path.join(work, 'agent');
  const manifest = path.join(work, 'manifest.json');
  const bytes = Buffer.from('fixture');
  await writeFile(binary, bytes);
  const data = { schemaVersion: 1, agentVersion: '1.2.3', artifacts: { 'x86_64-pc-windows-msvc': { sha256: createHash('sha256').update(bytes).digest('hex') } } };
  await writeFile(manifest, JSON.stringify(data));
  const env = { LLMN_AGENT_MANIFEST: manifest, LLMN_AGENT_FILE: binary, LLMN_AGENT_VERSION: '1.2.3', LLMN_AGENT_TARGET: 'x86_64-pc-windows-msvc' };
  const output = path.join(work, 'resources');
  await assert.rejects(prepare({}, output), /Set LLMN/);
  await assert.rejects(prepare({ ...env, LLMN_AGENT_VERSION: '2.0.0' }, output), /version mismatch/);
  await assert.rejects(prepare({ ...env, LLMN_AGENT_TARGET: 'aarch64-unknown-linux-gnu' }, output), /SHA256 artifact/);
  await prepare(env, output);
  assert.deepEqual(await readFile(path.join(output, 'llmn-agent.exe')), bytes);
  await writeFile(binary, 'tampered');
  await assert.rejects(prepare(env, output), /SHA256 mismatch/);
});

test('HTTPS release redirects work for manifest and executable without leaking signed URLs', async (t) => {
  const work = await mkdtemp(path.join(tmpdir(), 'llmn-agent-redirect-'));
  const bytes = Buffer.from('downloaded fixture');
  const target = 'x86_64-pc-windows-msvc';
  const manifestUrl = 'https://github.example/releases/v1.2.3/manifest.json?token=private';
  const binaryUrl = 'https://github.example/releases/v1.2.3/agent.exe?token=private';
  const manifest = { schemaVersion: 1, agentVersion: '1.2.3', artifacts: { [target]: { url: binaryUrl, sha256: createHash('sha256').update(bytes).digest('hex') } } };
  const calls = [];
  let finalProtocol = 'https:';
  let status = 200;
  let fail = false;
  t.mock.method(globalThis, 'fetch', async (url, options) => {
    calls.push([url, options.redirect]);
    if (fail) throw new Error(`network failure ${url}`);
    return { url: `${finalProtocol}//cdn.example/signed?token=private`, ok: status === 200, status,
      arrayBuffer: async () => url === manifestUrl ? Buffer.from(JSON.stringify(manifest)) : bytes };
  });
  const env = { LLMN_AGENT_MANIFEST: manifestUrl, LLMN_AGENT_VERSION: '1.2.3', LLMN_AGENT_TARGET: target };
  await prepare(env, work);
  assert.deepEqual(calls, [[manifestUrl, 'follow'], [binaryUrl, 'follow']]);
  assert.deepEqual(await readFile(path.join(work, 'llmn-agent.exe')), bytes);
  finalProtocol = 'http:';
  await assert.rejects(prepare(env, work), /non-HTTPS URL/);
  finalProtocol = 'https:';
  status = 403;
  await assert.rejects(prepare(env, work), error => error.message.includes('403') && !error.message.includes('private'));
  fail = true;
  await assert.rejects(prepare(env, work), error => error.message === 'Agent artifact download failed');
});
