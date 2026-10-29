import { createHash } from 'node:crypto';
import { readFile, writeFile, mkdir, chmod } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

export async function prepare(env = process.env, destination = fileURLToPath(new URL('../src-tauri/resources', import.meta.url))) {
  const { LLMN_AGENT_MANIFEST: manifestPath, LLMN_AGENT_VERSION: version, LLMN_AGENT_TARGET: target } = env;
  if (!manifestPath || !version || !target) throw new Error('Set LLMN_AGENT_MANIFEST, LLMN_AGENT_VERSION and LLMN_AGENT_TARGET to an exact version manifest and target triple. Build/package the Agent separately; no source build is performed.');
  if (!/^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$/.test(version) || !/^[a-zA-Z0-9_]+(?:-[a-zA-Z0-9_]+){2,}$/.test(target)) throw new Error('Agent version and target triple must be fixed and valid');
  const remote = manifestPath.startsWith('https://');
  async function download(url) {
    let result;
    try {
      result = await fetch(url, { redirect: 'follow' });
    } catch {
      throw new Error('Agent artifact download failed');
    }
    if (!result.url.startsWith('https://')) throw new Error('Agent artifact redirected to a non-HTTPS URL');
    if (!result.ok) throw new Error(`Agent artifact download failed: HTTP ${result.status}`);
    return Buffer.from(await result.arrayBuffer());
  }
  const manifest = JSON.parse((remote ? await download(manifestPath) : await readFile(manifestPath)).toString());
  if (manifest.schemaVersion !== 1 || manifest.agentVersion !== version) throw new Error('Agent manifest version mismatch');
  const entry = manifest.artifacts?.[target];
  if (!entry || !/^[a-f0-9]{64}$/.test(entry.sha256)) throw new Error(`Missing valid SHA256 artifact for ${target}`);
  const source = env.LLMN_AGENT_FILE || entry.url;
  if (!source) throw new Error('Set LLMN_AGENT_FILE for an explicitly built local Agent, or provide an HTTPS artifact URL');
  if (!env.LLMN_AGENT_FILE && !source.startsWith('https://')) throw new Error('Published Agent artifacts require HTTPS');
  const bytes = env.LLMN_AGENT_FILE ? await readFile(source) : await download(source);
  if (createHash('sha256').update(bytes).digest('hex') !== entry.sha256) throw new Error('Agent SHA256 mismatch');
  const name = `llmn-agent${target.includes('windows') ? '.exe' : ''}`;
  await mkdir(destination, { recursive: true });
  await writeFile(path.join(destination, name), bytes);
  if (!target.includes('windows')) await chmod(path.join(destination, name), 0o755);
  await writeFile(path.join(destination, 'agent-artifact.json'), JSON.stringify({ agentVersion: version, target, sha256: entry.sha256 }, null, 2) + '\n');
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  prepare().catch(error => { console.error(error.message); process.exitCode = 1; });
}
