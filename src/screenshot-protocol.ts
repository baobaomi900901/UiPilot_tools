export interface ScreenshotOwner { sessionId: string; generation: string; windowInstance: string }
export interface ScreenshotSnapshot extends ScreenshotOwner {
 revision: string
 phase: 'preparing' | 'selecting' | 'preview' | 'copying' | 'saving'
 mode: 'region' | 'window'
 width: number | null
 height: number | null
 error: string | null
 theme: 'light' | 'dark'
}
export function isScreenshotIdentity(value: unknown): value is string {
 return typeof value === 'string' && /^[1-9]\d{0,19}$/.test(value) && BigInt(value) <= BigInt('18446744073709551615')
}
export function parseScreenshotSnapshot(value: unknown): ScreenshotSnapshot | null {
 if (!value || typeof value !== 'object' || Array.isArray(value)) return null
 const v = value as Record<string, unknown>
 if (![v.sessionId,v.generation,v.windowInstance,v.revision].every(isScreenshotIdentity)) return null
 if (!['preparing','selecting','preview','copying','saving'].includes(v.phase as string) || !['region','window'].includes(v.mode as string) || !['light','dark'].includes(v.theme as string)) return null
 const dimension = (n: unknown) => typeof n === 'number' && Number.isInteger(n) && n > 0 && n <= 4294967295
 if (!((v.width === null && v.height === null) || (dimension(v.width) && dimension(v.height)))) return null
 if (['preview','copying','saving'].includes(v.phase as string) && v.width === null) return null
 if (v.error !== null && typeof v.error !== 'string') return null
 return {sessionId:v.sessionId as string,generation:v.generation as string,windowInstance:v.windowInstance as string,revision:v.revision as string,phase:v.phase as ScreenshotSnapshot['phase'],mode:v.mode as ScreenshotSnapshot['mode'],width:v.width as number|null,height:v.height as number|null,error:v.error as string|null,theme:v.theme as ScreenshotSnapshot['theme']}
}
export interface ScreenshotClient {
 listenState(handler: (value: unknown) => void): Promise<() => void>
 getState(): Promise<unknown>
 readImage(owner: ScreenshotOwner, transferId: string): Promise<ArrayBuffer>
 releaseImage(owner: ScreenshotOwner, transferId: string): Promise<void>
 copy(owner: ScreenshotOwner): Promise<{status:'copied'}>
 save(owner: ScreenshotOwner): Promise<{status:'saved'|'cancelled'}>
 retake(owner: ScreenshotOwner): Promise<void>
 close(owner: ScreenshotOwner): Promise<void>
}
