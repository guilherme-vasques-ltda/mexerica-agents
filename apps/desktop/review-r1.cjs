const test = require('node:test');
const assert = require('node:assert/strict');
const { buildSync } = require('esbuild');
const { runInNewContext } = require('node:vm');
const path = require('node:path');
const { webcrypto } = require('node:crypto');
const disk = new Map();
const storage = { getItem:k=>disk.get(k)??null, setItem:(k,v)=>disk.set(k,v), removeItem:k=>disk.delete(k) };
function load(entry) {
  const source=buildSync({entryPoints:[path.resolve(__dirname,entry)],bundle:true,platform:'node',format:'cjs',write:false,external:['react'],tsconfig:path.resolve(__dirname,'tsconfig.json')}).outputFiles[0].text;
  const module={exports:{}};
  runInNewContext(source,{module,exports:module.exports,require,localStorage:storage,window:{codync:{appVersion:'2.20.3'}},URLSearchParams,URL,crypto:webcrypto,setTimeout,clearTimeout,setInterval,clearInterval,queueMicrotask,console});
  return module.exports;
}
const pause=()=>new Promise(r=>setTimeout(r,20));
const space='00000000-0000-4000-8000-000000000001';
const snap=label=>({version:1,personalSpaceId:space,revision:1,spaces:[{id:space,kind:'personal',name:'Pessoal',position:0,revision:1,tile:null,appearanceConfigured:false,appearance:{wallpaper:'none',blur:0,transparency:50,veilLight:0.18,veilDark:0.32},owner:{name:label,initials:'R1',photo:null}}]});
const conv=label=>({id:'11111111-1111-4111-8111-111111111111',spaceId:space,botId:'22222222-2222-4222-8222-222222222222',title:label,revision:1,stateRevision:1,primary:true,closed:false,readRevision:0,engineAccountId:null,engineSelection:null,accountRevision:null,continuity:null});
function hello(id) {return {hostId:id,name:'same-label',version:'2.20.3',minApp:'2.20.1',spacesContract:1,spaceId:space,spaces:snap(id),conversations:[conv(id)],rev:1,usage:{providers:[]},screen:null};}
test('R1 real BotSync rejects stale hello from replaced transport',async()=>{
  const {BotSync}=load('src/renderer/store/bot-sync.ts');
  let oldResolve;let newEvent;
  const old={call:async(method)=>method==='hello'?new Promise(r=>oldResolve=r):{},stream:()=>()=>{}};
  const next={call:async(method)=>method==='hello'?hello('R1_REAL_HUB_B'):{},stream:(_p,on)=>{newEvent=on;return ()=>{}}};
  const transports=[old,next];
  const s=new BotSync({id:'same-computer-id',name:'same-label'},false,()=>transports.shift(),'2.20.3','same-context',space);
  s.start();s.restartStream();await pause();
  assert.equal(typeof newEvent,'function');
  newEvent(JSON.stringify({type:'hello',...hello('R1_REAL_HUB_B')}));
  assert.equal(s.conversations.get('11111111-1111-4111-8111-111111111111').title,'R1_REAL_HUB_B');
  oldResolve(hello('R1_REAL_HUB_A'));await pause();
  const actual={liveHub:s.hostId,helloHub:s.hello.hostId,conversation:s.conversations.get('11111111-1111-4111-8111-111111111111'),spaces:s.spaces};
  console.log('R1_STALE_HELLO='+JSON.stringify(actual));s.retire();
  assert.equal(actual.conversation.title,'R1_REAL_HUB_B');
});
test('R1 wrapper binds real Hub and excludes disposed responses',async()=>{
  const {SpaceClient}=load('src/shared/fork-spaces-client.ts');
  const client=new SpaceClient('R1_REAL_HUB_B',space,{call:async()=>({hubId:'R1_REAL_HUB_A',spaceId:space,conversations:[conv('A')]})});
  await assert.rejects(client.conversations(),/cross_hub_response/);
  let finish;const c=new SpaceClient('R1_REAL_HUB_B',space,{call:async()=>new Promise(r=>finish=r)});const a=c.conversation('a');
  const p=a.history();a.dispose();finish({hubId:'R1_REAL_HUB_B',context:{spaceId:space,conversationId:'a'},entries:[]});
  await assert.rejects(p,/stale_conversation_context/);c.dispose();client.dispose();
});

test('R2 real BotSync rejects stale hello through A-B-A and retired store',async()=>{
  disk.clear();
  const {BotSync}=load('src/renderer/store/bot-sync.ts');
  let resolveA;let resolveB;
  const events=[];
  const deferred=(set)=>({call:async(method)=>method==='hello'?new Promise(set):{},stream:(_p,on)=>{events.push(on);return ()=>{}}});
  const transports=[deferred(r=>resolveA=r),deferred(r=>resolveB=r),{call:async()=>hello('R2_HUB_A'),stream:(_p,on)=>{events.push(on);return ()=>{}}}];
  const s=new BotSync({id:'same-computer-id',name:'same-label'},false,()=>transports.shift(),'2.20.3','r2-aba',space);
  s.start();s.restartStream();s.restartStream();await pause();
  events.at(-1)(JSON.stringify({type:'hello',...hello('R2_HUB_A')}));
  resolveB(hello('R2_HUB_B'));resolveA(hello('R2_STALE_A'));await pause();s.saveCache();
  assert.equal(s.hostId,'R2_HUB_A');assert.equal(s.hello.hostId,'R2_HUB_A');assert.equal(s.spaces.spaces[0].owner.name,'R2_HUB_A');
  assert.equal(s.conversations.values().next().value.title,'R2_HUB_A');
  const cache=JSON.parse([...disk.entries()].find(([k])=>k === 'codync-mirror-'+JSON.stringify(['r2-aba','same-computer-id','R2_HUB_A',space]))[1]);
  assert.equal(cache.hostId,'R2_HUB_A');assert.equal(cache.conversations[0].title,'R2_HUB_A');
  s.retire();
  let retiredHello;
  const old=new BotSync({id:'same-computer-id',name:'same-label'},false,()=>deferred(r=>retiredHello=r),'2.20.3','r2-replacement',space);
  old.start();old.retire();
  const next=new BotSync({id:'same-computer-id',name:'same-label'},false,()=>({call:async()=>hello('R2_HUB_B'),stream:(_p,on)=>{events.push(on);return ()=>{}}}),'2.20.3','r2-replacement',space);
  next.start();await pause();events.at(-1)(JSON.stringify({type:'hello',...hello('R2_HUB_B')}));
  retiredHello(hello('R2_OLD_STORE'));await pause();next.saveCache();
  assert.equal(old.hello,null);assert.equal(next.hello.hostId,'R2_HUB_B');
  const replaced=JSON.parse([...disk.entries()].find(([k])=>k === 'codync-mirror-'+JSON.stringify(['r2-replacement','same-computer-id','R2_HUB_B',space]))[1]);
  assert.equal(replaced.hostId,'R2_HUB_B');assert.equal(replaced.conversations[0].title,'R2_HUB_B');next.retire();
});

test('R2 late loopback metadata cannot overwrite replacement client',async()=>{
  disk.clear();const {BotSync}=load('src/renderer/store/bot-sync.ts');
  let finishAccess;let currentEvent;
  const a={call:async(method)=>method==='hello'?hello('R2_A'):method==='accessRequests'?new Promise(r=>finishAccess=r):{source:'R2_A'},stream:()=>()=>{}};
  const b={call:async(method)=>method==='hello'?hello('R2_B'):method==='accessRequests'?{requests:[{id:'B'}]}:{source:'R2_B'},stream:(_p,on)=>{currentEvent=on;return ()=>{}}};
  const links=[a,b];const s=new BotSync({id:'same',name:'same'},true,()=>links.shift(),'2.20.3','r2-aux',space);
  s.start();await pause();s.restartStream();await pause();
  currentEvent(JSON.stringify({type:'hello',...hello('R2_B')}));
  finishAccess({requests:[{id:'A'}]});await pause();
  assert.equal(s.hello.hostId,'R2_B');assert.equal(s.cloud.source,'R2_B');assert.equal(s.accessRequests[0].id,'B');s.retire();
});
