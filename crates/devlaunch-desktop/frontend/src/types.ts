export interface ProjectInfo {
  name: string
  path: string
  project_type: string
  services: ServiceInfo[]
}

export interface ServiceInfo {
  name: string
  command: string
  working_dir: string | null
  target: string
}

export interface ServiceStateDto {
  service_name: string
  status: 'RUNNING' | 'STOPPED' | 'STARTING' | 'FAILED' | 'STOPPING'
  pid: number | null
  started_at: string | null
  url: string | null
}

export interface DependencyStatusDto {
  dep_type: string
  status: 'Ok' | 'Missing' | 'NotRunning' | 'NeedsSetup' | 'Unknown'
  version: string | null
  details: string[]
  fix_hint: string | null
}

export interface DiagramResult {
  architecture: string
  api_routes: string | null
  db_models: string | null
  warnings: string[]
}

export interface AnalysisResultDto {
  pattern: string
  confidence: number
  layers: LayerDto[]
  anti_patterns: AntiPatternDto[]
  top_complex: ComplexFunctionDto[]
  coverage: CoverageDto | null
  module_count: number
  total_loc: number
  markdown: string
}

export interface LayerDto {
  name: string
  count: number
}

export interface AntiPatternDto {
  kind: string
  description: string
  affected: string[]
  severity: string
  suggestion: string
}

export interface ComplexFunctionDto {
  file: string
  name: string
  line: number
  complexity: number
}

export interface CoverageDto {
  tool: string
  percent: number
  covered: number
  total: number
}
