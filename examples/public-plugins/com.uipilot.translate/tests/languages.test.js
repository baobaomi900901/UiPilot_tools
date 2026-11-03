import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { LANGUAGES, PROVIDERS, filterLanguages, sanitizePreferences } from '../package/dist/languages.js'
const catalog = JSON.parse(readFileSync(new URL('../../../../docs/plugin-sdk/youdao-languages.json', import.meta.url), 'utf8'))

test('all official language entries, detection flags and names are offered once', () => {
  assert.equal(LANGUAGES.length, 112)
  assert.equal(new Set(LANGUAGES.map(x => x.id)).size, 112)
  assert.deepEqual([...LANGUAGES].sort((a,b)=>a.id.localeCompare(b.id)), [...catalog.languages].sort((a,b)=>a.id.localeCompare(b.id)))
  assert.equal(LANGUAGES.filter(x=>x.autoDetect).length, 83)
  assert.ok(LANGUAGES.every(Object.isFrozen))
  assert.deepEqual(PROVIDERS.map(x=>x.id), ['youdao'])
  assert.equal(filterLanguages('', true).length, 113)
  assert.equal(filterLanguages('', true)[0].id, 'auto')
  assert.equal(filterLanguages('auto', false).length, 0)
})

test('language search matches Chinese names, English names and provider codes', () => {
  for (const [query, id] of [['日语','ja'],[' Japanese ','ja'],['FR','fr'],['阿拉伯','ar'],['zh-CHT','zh-CHT'],['sr-Latn','sr-Latn'],['Cantonese','yue']]) {
    assert.ok(filterLanguages(query).some(x=>x.id===id), query)
  }
  assert.deepEqual(filterLanguages('英').map(x=>x.id), ['en'])
  assert.equal(filterLanguages('not-a-language').length, 0)
  assert.equal(LANGUAGES.find(x=>x.id==='yue').autoDetect, false)
})

test('all languages persist while auto remains source-only and bodies are excluded', () => {
  for (const language of LANGUAGES) {
    const preferences={sourceLanguage:language.id,targetLanguage:language.id,providerId:'youdao'}
    assert.deepEqual(sanitizePreferences({...preferences,text:'secret'}), preferences)
  }
  assert.deepEqual(sanitizePreferences({sourceLanguage:'xx',targetLanguage:'auto',providerId:'constructor',text:'secret',translation:'secret'}),{sourceLanguage:'auto',targetLanguage:'zh-CHS',providerId:'youdao'})
  assert.deepEqual(sanitizePreferences(null),sanitizePreferences({}))
})
