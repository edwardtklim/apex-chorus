// Dependency-free UI contract tests. Provider replies are fixtures; no network/API billing.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const path = require('node:path');
class Element {
  constructor(){this.value='';this.textContent='';this.children=[];this.disabled=false;}
  replaceChildren(...nodes){this.children=nodes;}
  append(...nodes){this.children.push(...nodes);}
  add(node){this.children.push(node);}
}
const nodes = new Map();
const element = id => {if(!nodes.has(id))nodes.set(id,new Element());return nodes.get(id);};
const context = vm.createContext({console,Date,Number,Error,Promise,AbortSignal,encodeURIComponent,setInterval:()=>{},
  Option:function(text,value){this.textContent=text;this.value=value;},
  document:{getElementById:element,createElement:()=>new Element()}});
const html=fs.readFileSync(path.join(__dirname,'../site/index.html'),'utf8');
const source=html.slice(html.indexOf('var systemCollected='),html.indexOf('// ---------------- Repair'));
vm.runInContext(source,context);
(async()=>{
  context.chatId='test';element('conversation-provider').value='gpt';context.chatModels={gpt:'fixture-model'};
  context.chatKeys={gpt:false};context.chatPolicies={gpt:{cloud_allowed:true}};
  context.updateChatGate();assert.equal(element('conversation-send').disabled,true);assert.match(element('conversation-preview').textContent,/키 없음/);
  context.chatKeys.gpt=true;context.chatPolicies.gpt.cloud_allowed=false;
  context.updateChatGate();assert.equal(element('conversation-send').disabled,true);assert.match(element('conversation-preview').textContent,/전송 동의 없음/);
  context.chatPolicies.gpt.cloud_allowed=true;context.updateChatGate();assert.equal(element('conversation-send').disabled,false);
  context.chatBusy=true;context.updateChatGate();assert.equal(element('conversation-send').disabled,true);context.chatBusy=false;
  const attack='<img src=x onerror=alert(1)>';
  context.renderConversation({messages:[{role:'assistant',provider:'gpt',model:'fixture',at:'now',text:attack}]});
  assert.equal(element('conversation-messages').children[0].children[1].textContent,attack);
  context.systemCollected='2000-01-01T00:00:00Z';context.systemConnected=true;context.systemAge();assert.match(element('system-status').textContent,/오래된 값/);
  context.systemConnected=false;context.systemAge();assert.match(element('system-status').textContent,/연결 끊김/);
  const originalLoad=context.loadChat;context.loadChat=async()=>{};
  context.checkedJSON=async()=>({id:'flattened-core-id',messages:[]});
  await context.newConversation();assert.equal(context.chatId,'flattened-core-id');
  element('conversation-text').value='fixture question';context.chatId='test';context.updateChatGate();
  context.checkedJSON=async()=>({conversation:{messages:[{role:'assistant',provider:'gpt',model:'fixture',at:'now',text:'fixture answer'}]},answer:'fixture answer',warning:null});
  await context.sendConversation();assert.equal(element('conversation-text').value,'');assert.match(element('conversation-status').textContent,/저장했습니다/);
  element('conversation-text').value='keep on failure';context.checkedJSON=async()=>{throw new Error('다음 행동: 연결을 확인하세요.');};
  await context.sendConversation();assert.equal(element('conversation-text').value,'keep on failure');assert.match(element('conversation-status').textContent,/다음 행동/);
  context.loadChat=originalLoad;
  console.log('PASS: no-key, no-consent, ready/busy, escaped text, stale/offline, flattened conversation, success/failure UI fixtures.');
})().catch(e=>{console.error(e);process.exitCode=1;});
