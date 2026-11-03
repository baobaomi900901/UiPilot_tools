import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { onCommand } from '../package/dist/runtime.js'
for (const [text, from, to] of [['hello','en','zh-CHS'],['你好','zh-CHS','en']]) test(`translates ${from}`, async () => {
 const calls=[]
 const response=await onCommand({requestId:'test',input:text},{services:{youdao:{translate:async input=>{calls.push(input);return {translations:['translated','second']}}}}})
 assert.deepEqual(calls,[{text,from,to}])
 assert.deepEqual(response.results[0].defaultAction,{type:'copyText',text:'translated\nsecond'})
 assert.equal(response.results[0].subtitle, from === 'en' ? '英译中' : '中译英')
})
test('empty input avoids service', async()=>{
 const response=await onCommand({requestId:'test',input:'  '},{})
 assert.equal(response.results[0].title,'请输入需要翻译的内容')
})
for(const name of ['ServicePermissionDeniedError','CredentialDisabledError','CredentialNotConfiguredError','PermissionDeniedError']) test(name,async()=>{
 const response=await onCommand({requestId:'test',input:'hello'},{services:{youdao:{translate:async()=>{throw {name,message:'PRIVATE'}}}}})
 assert.match(response.results[0].subtitle,/设置/)
 assert.ok(!JSON.stringify(response).includes('PRIVATE'))
})
test('manifest and package contain only host business integration',()=>{
 const manifest=JSON.parse(readFileSync(new URL('../package/plugin.json',import.meta.url)))
 assert.equal(manifest.name,'翻译');assert.equal(manifest.version,'1.2.1');assert.equal(manifest.minimumHostVersion,'0.16.0')
 for(const permission of ['selection.read','credentials.youdaoTranslation','credentials.baiduOcr','credentials.deepseek','network.https','clipboard.write']) assert.ok(manifest.permissions.includes(permission))
 const source=readFileSync(new URL('../package/dist/runtime.js',import.meta.url),'utf8')
 assert.doesNotMatch(source,/DEVELOPMENT_ONLY_CREDENTIALS|appKey|appSecret|sha256|network\.request|https:\/\//)
})

test('preserves whitespace and safely handles inherited error names', async()=>{
 let seen
 await onCommand({requestId:'test',input:'  hello \n'},{services:{youdao:{translate:async input=>{seen=input.text;return {translations:['translated']}}}}})
 assert.equal(seen,'  hello \n')
 for(const name of ['constructor','__proto__','toString']) {
  const response=await onCommand({requestId:'test',input:'hello'},{services:{youdao:{translate:async()=>{throw {name}}}}})
  assert.equal(response.results[0].title,'翻译失败')
 }
})
