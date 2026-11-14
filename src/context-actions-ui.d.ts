export type ContextActionIcon = 'search' | 'edit' | 'copy' | 'save' | 'plus' | 'trash' | 'arrow' | 'settings' | 'close' | 'pin' | 'eye' | 'arrow-right-to-line' | 'arrow-left-to-line'
export interface ContextAction {
  id: string
  title: string
  shortcut?: string
  icon?: ContextActionIcon
  disabledReason?: string
  run(): void | Promise<void>
}
export interface ContextActionGroups {
  context?: readonly ContextAction[]
  global?: readonly ContextAction[]
}
export interface ContextActionsMenu {
  readonly isOpen: boolean
  open(groups: ContextActionGroups, options?: { primaryModifier?: 'control' | 'meta'; origin?: Element | null; restoreFocus?: () => void; bottom?: number; onClose?: () => void }): void
  close(restore?: boolean): void
}
export function createContextActionsMenu(document: Document, onError?: () => void): ContextActionsMenu
