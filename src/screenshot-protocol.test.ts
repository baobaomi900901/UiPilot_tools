import { describe, expect, it } from 'vitest'
import { parseScreenshotSnapshot } from './screenshot-protocol'
const valid = {sessionId:'1',generation:'1',windowInstance:'2',revision:'9007199254740993',phase:'preview',mode:'region',width:100,height:50,error:null,theme:'dark'}
describe('screenshot wire boundary',()=>{
 it('preserves canonical u64 identities without number rounding',()=>expect(parseScreenshotSnapshot(valid)).toEqual(valid))
 it('rejects malformed ownership, dimensions, phases and themes',()=>{
  for(const patch of [{sessionId:'01'},{generation:1},{revision:'18446744073709551616'},{width:-1},{height:1.5},{theme:'system'},{phase:'idle'},{width:null}]) expect(parseScreenshotSnapshot({...valid,...patch})).toBeNull()
 })
})
