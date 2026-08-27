export type PluginPlatform = 'windows' | 'macos'
export type SourceKind = 'directory' | 'archive' | 'unknown'
export type PluginOutputMode = 'mainResult' | 'window' | 'panel'

export const PLUGIN_CLI_HOST_VERSION = '0.3.2' as const
export type PluginCliHostVersion = typeof PLUGIN_CLI_HOST_VERSION

export const PLUGIN_VALIDATION_ISSUE_CODES = [
  'SOURCE_INVALID',
  'ARCHIVE_INVALID',
  'PACKAGE_LIMIT_EXCEEDED',
  'PATH_INVALID',
  'PATH_COLLISION',
  'RESOURCE_INVALID',
  'MANIFEST_MISSING',
  'MANIFEST_JSON_INVALID',
  'MANIFEST_SCHEMA_INVALID',
  'MANIFEST_SEMANTIC_INVALID',
  'PLATFORM_INCOMPATIBLE',
  'API_INCOMPATIBLE',
  'PERMISSION_UNSUPPORTED',
  'ENTRY_MISSING',
  'ICON_INVALID',
  'CSS_REFERENCE_INVALID',
] as const

export type PluginValidationIssueCode = (typeof PLUGIN_VALIDATION_ISSUE_CODES)[number]

export type PluginIssueLocation =
  | { kind: 'jsonPointer'; value: string }
  | { kind: 'byteOffset'; value: string }
  | { kind: 'name'; value: string }

export interface PluginValidationIssue {
  code: PluginValidationIssueCode
  path?: string
  location?: PluginIssueLocation
  message: string
}

export interface PluginValidationReportV1 {
  schemaVersion: 1
  valid: boolean
  source: {
    kind: SourceKind
    path: string
  }
  target: {
    platform: PluginPlatform
    hostVersion: PluginCliHostVersion
    apiVersion: 1
  }
  plugin?: {
    pluginId: string
    version: string
    outputMode: PluginOutputMode
  }
  truncated: boolean
  issues: PluginValidationIssue[]
}

export interface PluginCliErrorV1 {
  schemaVersion: 1
  error: {
    code: 'CLI_USAGE' | 'CLI_INTERNAL'
    message: string
  }
}

export interface ValidationRequest {
  source: string
  platform: PluginPlatform
}

export type ValidatePackage = (request: ValidationRequest) => Promise<PluginValidationReportV1>
