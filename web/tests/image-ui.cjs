// Real selection/composer and image reader with controlled browser primitives.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict'),ts=require('typescript');
const compile=file=>ts.transpileModule(fs.readFileSync(path.resolve(__dirname,'../src/chat',file),'utf8'),{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
const flush=()=>new Promise(resolve=>setImmediate(resolve));
const all=n=>!n||typeof n!=='object'?[]:[n,...[n.props?.children].flat(Infinity).flatMap(all)];
(async()=>{
  let dimensions=32, decodingFails=false, reads=0;
  const images={};
  vm.runInNewContext(compile('images.ts'),{exports:images,crypto:require('node:crypto').webcrypto,
    FileReader:class {readAsDataURL(file){reads++;this.result=`data:${file.type};base64,AQID`;this.onload()}},
    Image:class {get naturalWidth(){return dimensions}get naturalHeight(){return dimensions}async decode(){if(decodingFails)throw new Error('decode failed')}},
  });
  const file={type:'image/png',size:3,name:'中文.png'};
  const selected=await images.readImages([file],[]);
  assert.equal(selected[0].data,'AQID');assert.equal(selected[0].name,'中文.png');
  await assert.rejects(images.readImages([file],Array(4).fill(selected[0])),/最多 4/);
  await assert.rejects(images.readImages([{...file,size:5*1024*1024}],[]),/单张/);
  await assert.rejects(images.readImages([{...file,type:'image/svg+xml'}],[]),/PNG/);
  dimensions=4097;await assert.rejects(images.readImages([file],[]),/4096/);dimensions=32;
  decodingFails=true;await assert.rejects(images.readImages([file],[]),/无法识别/);decodingFails=false;
  await assert.rejects(images.readImages([{...file,size:0}],[]),/为空/);
  await assert.rejects(images.readImages(Array(3).fill({...file,size:3*1024*1024}),[]),/合计/);
  assert.equal(images.imageUrl(selected[0]),'data:image/png;base64,AQID');
  console.log('PASS: current chat image reader checks formats, count, sizes, dimensions, decode failures and base64');
})().catch(error=>{console.error(error);process.exitCode=1});
