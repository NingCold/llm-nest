const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const ts = require('typescript');
const React = require('react');
const source = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/Devices.tsx'), 'utf8'), {compilerOptions: {jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022}}).outputText;
const device = {deviceId:'local', trusted:true, master:'master', fingerprint:'ed', curve:'curve', hasRecoverySecrets:true};
const other = {deviceId:'other', name:'Other device', fingerprint:'other-ed', curve:'other-curve', master:'other-master', trusted:false};
function harness(status, saved, known = !!status, desktop = false, options = {}) {
  const info = options.info || {supported:true, enabled:false, paused:false};
  const values = [status, false, '', false, 'human', [], null, false, false, '', '', '', null, false, 0, info, '', false];
  const calls = [];
  let cursor = 0;
  const exports = {};
  vm.runInNewContext(source, {exports, window:{}, FormData:class {constructor(form){this.form=form;}get(name){return this.form.data[name];}}, require:name => name === 'react' ? {...React, useEffect(){}, useState(initial){ const i=cursor++; if (values[i] === undefined) values[i]=initial; return [values[i], value => {values[i]=typeof value==='function'?value(values[i]):value;}]; }} : name === '@/lib/desktop' ? {CUSTOM_WINDOW_CHROME:desktop,IS_DESKTOP:desktop} : name === './engine' ? {hasCryptoDevice:()=>known, recoveryConfirmed:()=>saved,cryptoDevice:()=>status,deviceStatus:async()=>status,lockCrypto:()=>calls.push(['lock']),unlockCrypto:async(...args)=>calls.push(['create',...args])} : name === './auto-unlock' ? {autoUnlockInfo:async()=>info,enableAutoUnlock:async(...args)=>{calls.push(['enable',...args]);info.enabled=true;},disableAutoUnlock:async(...args)=>{calls.push(['disable',...args]);info.enabled=false;},unlockManually:async(...args)=>calls.push(['unlock',...args]),pauseAutoUnlock:(...args)=>calls.push(['pause',...args])} : name.endsWith('.css') || name === '../api' ? {} : require(name)});
  return {values, calls, render(){cursor=0; return exports.CryptoDevices({accountId:'human',onLock:options.onLock});}};
}
function nodes(tree) { if (!tree || typeof tree !== 'object') return []; return [tree, ...React.Children.toArray(tree.props?.children).flatMap(nodes)]; }
function text(tree) { return React.Children.toArray(tree?.props?.children).map(x=>typeof x==='object'?text(x):String(x)).join(''); }
function button(tree, label) {return nodes(tree).find(x=>x.type==='button' && text(x)===label);}
const first=harness(null,false).render();
assert.ok(button(first,'确认创建独立设备'));
assert.ok(!button(first,'确认建立首次加密身份'));
const setup=harness({...device, master:null, trusted:false, hasRecoverySecrets:false},false).render();
assert.ok(button(setup,'确认建立首次加密身份'));
const locked=harness(null,true,true).render();
assert.ok(button(locked,'解锁本机资料'));
assert.ok(nodes(locked).some(x=>x.type==='h3' && text(x)==='解锁本机资料'));
assert.ok(!nodes(locked).some(x=>x.type==='ol'), 'known locked devices must not restart first-time steps');
assert.ok(!button(locked,'确认创建独立设备'));
const complete=harness(device,true).render();
assert.ok(nodes(complete).some(x=>x.props?.className==='crypto-eyebrow' && text(x)==='加密已就绪'));
for (const label of ['查看完整设备指纹','核对与管理其他设备','从恢复文件授权此设备']) {
  const disclosure=nodes(complete).find(x=>x.type==='details' && nodes(x).some(n=>n.type==='summary' && text(n)===label));
  assert.ok(disclosure && !disclosure.props.open, `${label} starts collapsed`);
}
const recovery=harness(device,false);
recovery.values[12]={format:'test'}; recovery.values[13]=true;
assert.equal(button(recovery.render(),'确认恢复文件已保存').props.disabled,true,'download alone does not confirm preservation');
recovery.values[8]=true;
assert.equal(button(recovery.render(),'确认恢复文件已保存').props.disabled,false);
const trust=harness(device,false);
trust.values[6]=other; trust.values[8]=true; trust.values[9]='other-ed'; trust.values[10]='other-curve'; trust.values[11]='other-master';
assert.equal(button(trust.render(),'确认信任此设备').props.disabled,true,'recovery checkbox cannot authorize a device');
trust.values[7]=true;
assert.equal(button(trust.render(),'确认信任此设备').props.disabled,false);
for (const i of [9,10,11]) {const original=trust.values[i]; trust.values[i]='wrong'; assert.equal(button(trust.render(),'确认信任此设备').props.disabled,true,'each fingerprint must match'); trust.values[i]=original;}
function headings(tree) {return nodes(tree).filter(x=>x.type==='h3').map(text);}
assert.ok(headings(setup).indexOf('建立加密身份') < headings(setup).indexOf('本机设备'));
const recoveryOrder=headings(recovery.render());
assert.ok(recoveryOrder.indexOf('恢复文件') < recoveryOrder.indexOf('本机设备'), 'unfinished recovery precedes device details in DOM and keyboard order');
assert.ok(headings(complete).indexOf('本机设备') < headings(complete).indexOf('恢复文件'), 'ready state keeps everyday device controls first');
for (const [status, saved, known] of [[null,false,false],[null,true,true],[device,true,true]]) {
  const tree=harness(status,saved,known,true).render();
  const header=nodes(tree).find(node=>node.type==='header');
  assert.equal(header.props['data-tauri-drag-region'],'deep','every device state exposes the native header drag and double-click region');
  assert.deepEqual(nodes(tree).filter(node=>node.props?.['data-tauri-drag-region']).map(node=>node.type),['header'],'fingerprints, recovery controls and fields remain outside all drag regions');
}
assert.ok(!nodes(complete).some(node=>node.props?.['data-tauri-drag-region']),'web device pages do not expose native drag regions');
console.log('PASS: crypto device setup, compact ready state, recovery confirmation and independent three-fingerprint trust');
async function autoUnlockInteractions() {
  const waiting=harness(device,false);
  assert.equal(button(waiting.render(),'确认开启 30 天').props.disabled,true,'recovery must be preserved before enabling auto unlock');
  const untrusted=harness({...device,trusted:false},true);
  assert.equal(button(untrusted.render(),'确认开启 30 天').props.disabled,true,'only trusted devices can enable auto unlock');
  const unsupported=harness(device,true,true,false,{info:{supported:false,enabled:false,paused:false}});
  assert.match(text(unsupported.render()),/此平台暂不支持自动解锁/);
  assert.ok(!button(unsupported.render(),'确认开启 30 天'));
  const enabled=harness(device,true);
  const tree=enabled.render();
  const form=nodes(tree).find(n=>n.type==='form' && n.props.className.includes('crypto-auto-form'));
  let resets=0;
  form.props.onSubmit({preventDefault(){},currentTarget:{data:{autoPassphrase:'existing-local-passphrase'},reset(){resets++;}}});
  await new Promise(setImmediate);
  assert.deepEqual(enabled.calls,[['enable','human','existing-local-passphrase']]);
  assert.equal(resets,1);
  button(enabled.render(),'关闭自动解锁').props.onClick();
  await new Promise(setImmediate);
  assert.deepEqual(enabled.calls[1],['disable','human']);
  assert.ok(button(enabled.render(),'确认开启 30 天'));
  const locked=harness(null,true,true);
  nodes(locked.render()).find(n=>n.type==='form').props.onSubmit({preventDefault(){},currentTarget:{data:{passphrase:'local-passphrase'},reset(){}}});
  await new Promise(setImmediate);
  assert.deepEqual(locked.calls,[['unlock','human','local-passphrase']],'existing devices use the manual unlock path');
  const first=harness(null,false,false);
  nodes(first.render()).find(n=>n.type==='form').props.onSubmit({preventDefault(){},currentTarget:{data:{passphrase:'local-passphrase'},reset(){}}});
  await new Promise(setImmediate);
  assert.deepEqual(first.calls,[['create','human','local-passphrase',true]],'first setup keeps identity creation explicit');
  const local=harness(device,true);
  button(local.render(),'锁定本机资料').props.onClick();
  assert.deepEqual(local.calls,[['pause','human'],['lock']]);
  let lockRequests=0;
  const delegated=harness(device,true,true,false,{onLock(){lockRequests++;}});
  button(delegated.render(),'锁定本机资料').props.onClick();
  assert.equal(lockRequests,1); assert.deepEqual(delegated.calls,[],'parent owns draft persistence before locking');
  const expiry=Date.UTC(2026,10,28);
  const active=harness(device,true,true,false,{info:{supported:true,enabled:true,paused:true,expiresAt:expiry}}).render();
  assert.equal(nodes(active).find(n=>n.type==='time').props.dateTime,new Date(expiry).toISOString());
  assert.match(text(active),/下次输入本机口令解锁后/);
  console.log('PASS: auto unlock readiness, explicit consent, platform support, expiry, manual unlock and parent-owned lock');
}
autoUnlockInteractions().catch(e=>{console.error(e);process.exitCode=1;});
if (process.argv[2]) {
  const path = require('node:path');
  const {renderToStaticMarkup} = require('react-dom/server');
  const assets = path.resolve(__dirname, '../dist/assets');
  const base = fs.readdirSync(assets).filter(name=>name.startsWith('index-') && name.endsWith('.css')).map(name=>fs.readFileSync(path.join(assets,name),'utf8')).join('');
  const css = base + fs.readFileSync(require.resolve('../src/chat/social.css'),'utf8') + fs.readFileSync(require.resolve('../src/cloud/cloud.css'),'utf8') + fs.readFileSync(require.resolve('../src/cloud/crypto/devices.css'),'utf8');
  const pending = harness(device,false); pending.values[12]={format:'fixture'}; pending.values[13]=true; pending.values[8]=true;
  const automatic = harness(device,true,true,true,{info:{supported:true,enabled:true,paused:false,expiresAt:Date.UTC(2026,10,28)}}).render();
  const pages = [['ready', complete], ['recovery', pending.render()], ['first', first], ['locked', locked], ['automatic', automatic]];
  fs.mkdirSync(process.argv[2],{recursive:true});
  for (const theme of ['light','dark']) for (const [name,tree] of pages) fs.writeFileSync(path.join(process.argv[2],name+'-'+theme+'.html'), '<!doctype html><html class="'+theme+'"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><style>body{margin:0;font-family:Arial,sans-serif;height:100vh} .social-shell{width:min(100%,800px);margin:auto}'+css+'</style><div class="social-shell cloud-workspace font-sans">'+renderToStaticMarkup(tree)+'</div></html>');
}
