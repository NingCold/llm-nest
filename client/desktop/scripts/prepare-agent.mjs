import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root=fileURLToPath(new URL('../../../',import.meta.url));
const dev=process.argv.includes('--dev');
const windows=process.platform==='win32';
const target=process.env.CARGO_BUILD_TARGET || (windows?'x86_64-pc-windows-msvc':undefined);
// A pinned caller toolchain (including CI) takes precedence over the local Windows default.
const toolchain=windows && !process.env.RUSTUP_TOOLCHAIN?['+stable-x86_64-pc-windows-msvc']:[];
const args=[...toolchain,'build','-p','agent-node','--bin','llmn-agent','--locked',...(target?['--target',target]:[]),...(dev?[]:['--release'])];
execFileSync('cargo',args,{cwd:root,stdio:'inherit',windowsHide:true});
const name=`llmn-agent${windows?'.exe':''}`;
const source=path.join(path.resolve(root,process.env.CARGO_TARGET_DIR||'target'),...(target?[target]:[]),dev?'debug':'release',name);
const dest=path.join(root,'client/desktop/src-tauri/resources');
mkdirSync(dest,{recursive:true});
copyFileSync(source,path.join(dest,name));
