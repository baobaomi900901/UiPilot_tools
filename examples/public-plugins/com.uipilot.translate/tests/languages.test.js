import test from 'node:test'
import assert from 'node:assert/strict'
import { LANGUAGES, PROVIDERS, filterLanguages, sanitizePreferences } from '../package/dist/languages.js'
test('only published languages and provider are offered',() => {
  assert.deepEqual(LANGUAGES.map(x=>x.id),['en','zh-CHS']);
  assert.deepEqual(PROVIDERS.map(x=>x.id),['youdao']);
  assert.deepEqual(filterLanguages('',true).map(x=>x.id),['auto','en','zh-CHS']);
  assert.deepEqual(filterLanguages('英',false).map(x=>x.id),['en']);
  assert.equal(filterLanguages('auto',false).length,0)
})
test('preference whitelist excludes bodies and sanitizes invalid values',() => {
  assert.deepEqual(sanitizePreferences({sourceLanguage:'xx',targetLanguage:'auto',providerId:'constructor',text:'secret',translation:'secret'}),{sourceLanguage:'auto',targetLanguage:'zh-CHS',providerId:'youdao'});
  assert.deepEqual(sanitizePreferences({sourceLanguage:'zh-CHS',targetLanguage:'en',providerId:'youdao'}),{sourceLanguage:'zh-CHS',targetLanguage:'en',providerId:'youdao'});
  assert.deepEqual(sanitizePreferences(null),sanitizePreferences({}))})
