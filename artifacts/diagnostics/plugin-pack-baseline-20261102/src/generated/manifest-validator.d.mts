export interface StandaloneValidationError {
  instancePath: string
  schemaPath: string
  keyword: string
  params: Record<string, unknown>
  message?: string
}

export interface StandaloneManifestValidator {
  (value: unknown): boolean
  errors?: StandaloneValidationError[] | null
}

declare const validate: StandaloneManifestValidator
export default validate
