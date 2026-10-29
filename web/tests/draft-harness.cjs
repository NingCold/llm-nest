// Direct access to the current chat draft repository; no retired runtime or store.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),ts=require('typescript');
const {IDBFactory}=require('fake-indexeddb');
const compile=name=>ts.transpileModule(fs.readFileSync(path.resolve(__dirname,'../src/chat',name),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
function harness({factory=new IDBFactory(),values=new Map()}={}) {
 const storage={getItem:key=>values.get(key)??null,setItem:(key,value)=>values.set(key,value),removeItem:key=>values.delete(key)};
 const persistence={};vm.runInNewContext(compile('draft-storage.ts'),{exports:persistence,Blob,Uint8Array,atob,btoa,indexedDB:factory,localStorage:storage});
 const database=new persistence.DraftDatabase(factory,storage);
 return {database,persistence,factory,values,storage,reopen:()=>harness({factory,values})};
}
module.exports={harness,compile};
