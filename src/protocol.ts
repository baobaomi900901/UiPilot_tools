export interface ResultItem {
  resultId: string
  title: string
  subtitle?: string
  icon?: string
}

export interface SearchResponse {
  requestId: string
  items: ResultItem[]
}
