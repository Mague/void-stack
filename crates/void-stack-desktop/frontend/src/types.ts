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
  docker_ports?: string[]
  docker_volumes?: string[]
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
  format: string
  saved_path: string | null
}

export interface ScanResultDto {
  services: ScannedServiceDto[]
  project_type: string
}

export interface ScannedServiceDto {
  name: string
  command: string
  working_dir: string
  detected_type: string
}

export interface SnapshotDto {
  timestamp: string
  label: string | null
  services: ServiceSnapshotDto[]
}

export interface ServiceSnapshotDto {
  name: string
  pattern: string
  total_modules: number
  total_loc: number
  anti_pattern_count: number
  avg_complexity: number
  max_complexity: number
  complex_functions: number
  coverage_percent: number | null
  god_classes: number
  circular_deps: number
  // Detail fields (only present in live analysis, absent in history)
  god_classes_detail?: GodClassDetailDto[]
  complex_functions_detail?: ComplexFunctionDetailDto[]
  anti_patterns_detail?: AntiPatternDetailDto[]
  circular_deps_detail?: CircularDepDetailDto[]
}

export interface GodClassDetailDto {
  file: string
  loc: number
  functions: number
  severity: string
}

export interface ComplexFunctionDetailDto {
  file: string
  name: string
  line: number
  complexity: number
}

export interface AntiPatternDetailDto {
  kind: string
  description: string
  affected: string[]
  severity: string
  suggestion: string
}

export interface CircularDepDetailDto {
  cycle: string[]
}

export interface DebtComparisonDto {
  previous: string
  current: string
  overall_trend: string
  services: ServiceComparisonDto[]
}

export interface ServiceComparisonDto {
  name: string
  loc_delta: number
  antipattern_delta: number
  complexity_delta: number
  coverage_delta: number | null
  god_class_delta: number
  circular_dep_delta: number
  trend: string
}

export interface AnalysisResultDto {
  pattern: string
  confidence: number
  layers: LayerDto[]
  anti_patterns: AntiPatternDto[]
  top_complex: ComplexFunctionDto[]
  coverage: CoverageDto | null
  coverage_hint: string | null
  module_count: number
  total_loc: number
  markdown: string
  best_practices: BestPracticesResultDto | null
}

export interface BestPracticesResultDto {
  overall_score: number
  tools_used: string[]
  tool_scores: ToolScoreDto[]
  findings: BpFindingDto[]
}

export interface ToolScoreDto {
  tool: string
  score: number
  finding_count: number
  native_score: number | null
}

export interface BpFindingDto {
  rule_id: string
  tool: string
  category: string
  severity: string
  file: string
  line: number | null
  col: number | null
  message: string
  fix_hint: string | null
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

// ── Docker Service Import ──

export interface DockerServicePreview {
  name: string
  image: string | null
  ports: string[]
  volumes: string[]
  env_vars: [string, string][]
  depends_on: string[]
  kind: string
  source: string // "compose" or "dockerfile"
  already_exists: boolean
}

// ── Docker Intelligence ──

export interface DockerAnalysisDto {
  has_dockerfile: boolean
  has_compose: boolean
  dockerfile: DockerfileInfoDto | null
  compose: ComposeProjectDto | null
  terraform: InfraResourceDto[]
  kubernetes: K8sResourceDto[]
  helm: HelmChartDto | null
}

export interface DockerfileInfoDto {
  stages: DockerStageDto[]
  exposed_ports: number[]
  entrypoint: string | null
  cmd: string | null
  workdir: string | null
}

export interface DockerStageDto {
  name: string | null
  base_image: string
}

export interface ComposeProjectDto {
  services: ComposeServiceDto[]
  networks: string[]
  volumes: string[]
}

export interface ComposeServiceDto {
  name: string
  image: string | null
  ports: PortMappingDto[]
  volumes: VolumeMountDto[]
  depends_on: string[]
  kind: string
  has_healthcheck: boolean
}

export interface PortMappingDto {
  host: number
  container: number
}

export interface VolumeMountDto {
  source: string
  target: string
  named: boolean
}

export interface InfraResourceDto {
  provider: string
  resource_type: string
  name: string
  kind: string
  details: string[]
}

export interface K8sResourceDto {
  kind: string
  name: string
  namespace: string | null
  images: string[]
  ports: number[]
  replicas: number | null
}

export interface HelmChartDto {
  name: string
  version: string
  dependencies: HelmDependencyDto[]
}

export interface HelmDependencyDto {
  name: string
  version: string
  repository: string
}

export interface DockerGenerateResultDto {
  dockerfile: string | null
  compose: string | null
  saved_paths: string[]
}

// ── AI Suggestions ──

export interface SuggestionResultDto {
  suggestions: SuggestionDto[]
  model_used: string
  raw_response: string
  fallback_context: string | null
}

export interface SuggestionDto {
  category: string
  title: string
  description: string
  affected_files: string[]
  priority: string
}
