const { execFileSync } = require('node:child_process');
const { readFileSync, writeFileSync, mkdirSync, copyFileSync } = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
function generate(target, tool, webNotices, output) {
  if (execFileSync(tool, ['--version'], { encoding: 'utf8' }).trim() !== 'cargo-about 0.9.3') throw Error('Use the verified cargo-about 0.9.3 binary');
  const rust = path.join(root, 'target', `notices-rust-${target}.txt`);
  mkdirSync(path.dirname(rust), { recursive: true });
  execFileSync(tool, ['generate', '--manifest-path', path.join(root, 'desktop/src-tauri/Cargo.toml'), '--config', path.join(root, 'about.toml'), '--target', target, '--locked', '--fail', '--output-file', rust, path.join(root, 'about.hbs')], { cwd: root, stdio: 'inherit' });
  const texts = [readFileSync(webNotices, 'utf8'), readFileSync(rust, 'utf8'), 'Tauri Windows installer templates\n' + readFileSync(path.join(root, 'desktop/src-tauri/packaging/windows/LICENSE.tauri'), 'utf8')];
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, texts.join('\n\n'));
  for (const name of ['LICENSE', 'NOTICE']) copyFileSync(path.join(root, name), path.join(path.dirname(output), name + '.txt'));
}
exports.generate = generate;
if (require.main === module) {
  const target = process.env.TAURI_ENV_TARGET_TRIPLE || process.env.LLMN_AGENT_TARGET;
  if (!target) throw Error('Set LLMN_AGENT_TARGET to the desktop target triple');
  generate(target, process.env.CARGO_ABOUT || 'cargo-about', path.join(root, 'desktop/dist/THIRD_PARTY_NOTICES.txt'), path.join(root, 'desktop/src-tauri/resources/THIRD_PARTY_NOTICES.txt'));
}
