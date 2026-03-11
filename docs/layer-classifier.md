# Clasificador de Capas Arquitectonicas

## Descripcion General

El clasificador de capas (`classify_layer`) asigna automaticamente cada archivo de codigo fuente a una capa arquitectonica (Controller, Service, Repository, Model, Utility, Config, Test) utilizando un **sistema de scoring ponderado** basado en multiples senales.

A diferencia de enfoques estaticos que dependen de nombres de directorios hardcoded, este clasificador analiza **que hace el codigo** para determinar su capa, lo que lo hace funcionar con cualquier estructura de proyecto.

## Capas Arquitectonicas

| Capa | Descripcion | Ejemplos |
|------|-------------|----------|
| **Controller** | Puntos de entrada HTTP, CLI, UI, RPC | Handlers, routes, pages, widgets, commands |
| **Service** | Logica de negocio, orquestacion | Servicios, use cases, BLoCs, providers |
| **Repository** | Acceso a datos, ORM, SQL | DAOs, repos, queries, migrations |
| **Model** | Estructuras de datos, DTOs, schemas | Structs, interfaces, entities, schemas |
| **Utility** | Helpers, utilidades generales | Utils, helpers, middleware, lib |
| **Config** | Configuracion del sistema | .env, config files, settings |
| **Test** | Tests unitarios e integracion | test_*, *.spec.*, tests/ |

## Arquitectura del Clasificador

```
                    Archivo de codigo
                         |
                         v
              +--------------------+
              | 1. Path overrides  |  Test? Config? main.rs?
              | (deterministicos)  |  → Si: retorna inmediato
              +--------------------+
                         |
                         v
              +--------------------+
              | 2. Content scoring |  Escanea patrones en el contenido
              | (senales + pesos)  |  Cada match suma puntos a una capa
              +--------------------+
                         |
                         v
              +--------------------+
              | 3. Directory bonus |  +2 pts por convenciones universales
              | (secundario)       |  controllers/, services/, models/, etc.
              +--------------------+
                         |
                         v
              +--------------------+
              | 4. Fan-in/Fan-out  |  Solo para archivos aun Unknown
              | (grafo de deps)    |  Usa posicion en grafo de imports
              +--------------------+
                         |
                         v
                  Capa con mayor score
```

## Nivel 1: Overrides Deterministicos (Path)

Estos se aplican **antes** del scoring. Si matchean, no hay ambiguedad:

| Patron | Capa | Ejemplo |
|--------|------|---------|
| Path contiene `test` o `spec` | Test | `tests/test_users.py`, `user.spec.ts` |
| Termina en `config.*` o contiene `.env` | Config | `src/config.py`, `.env.production` |
| Termina en `main.rs`, `lib.rs`, `build.rs` | Utility | Entry points del compilador |

## Nivel 2: Scoring por Contenido (Principal)

El motor escanea el contenido del archivo buscando **patrones de texto** (senales). Cada senal tiene un peso (1-5 puntos) y una capa asociada. Los pesos se acumulan y la capa con mas puntos gana.

### Pesos

| Peso | Significado | Ejemplo |
|------|-------------|---------|
| **5** | Senal inequivoca del framework | `@Component(` (Angular) |
| **4** | Anotacion/decorator de framework | `@RestController`, `#[tauri::command]`, `CallToolResult` |
| **3** | Patron fuerte del framework | `@Get(`, `router.get(`, `APIRouter`, `gin.Context` |
| **2** | Patron moderado | `useState(`, `#[derive(Serialize`, `export interface` |
| **1** | Patron debil (ubiquo) | `pub struct`, `impl `, `function `, `def ` |

### Lenguajes y Frameworks Soportados

#### Rust
| Capa | Senales |
|------|---------|
| Controller | `#[tauri::command]`, `CallToolResult`, `#[get(`, `#[post(`, `#[put(`, `#[delete(`, `#[derive(Args`, `#[derive(Subcommand`, `#[command(`, `axum::Router`, `actix_web::web`, `HttpResponse` |
| Service | `#[async_trait]`, `impl ` |
| Repository | `diesel::`, `sqlx::`, `sea_orm::`, `#[derive(Queryable`, `#[derive(Insertable` |
| Model | `#[derive(Serialize`, `#[derive(Deserialize`, `pub struct `, `pub enum ` |

#### Python (Django, Flask, FastAPI)
| Capa | Senales |
|------|---------|
| Controller | `@app.route(`, `@app.get(`, `@app.post(`, `@router.`, `APIRouter`, `@api_view`, `def get(self, request`, `def post(self, request` |
| Service | `class ` |
| Repository | `session.query(`, `cursor.execute(`, `objects.filter(`, `objects.get(`, `objects.create(`, `objects.all(`, `tortoise.models` |
| Model | `class Meta:`, `(BaseModel):`, `(models.Model):`, `@dataclass`, `@attr.s`, `Field(` |
| Config | `os.environ` |

#### JavaScript / TypeScript (Express, NestJS, Next.js, React, Vue, Angular)
| Capa | Senales |
|------|---------|
| Controller | **Express**: `app.get(`, `app.post(`, `router.get(`, `fastify.get(`<br>**NestJS**: `@Controller(`, `@Get(`, `@Post(`, `@Put(`, `@Delete(`, `@Patch(`<br>**Next.js**: `getServerSideProps`, `getStaticProps`, `export async function GET(`, `NextRequest`, `NextResponse`<br>**React**: `useState(`, `useEffect(`, `return (<`, `React.FC`, `JSX.Element`<br>**Vue**: `defineComponent(`, `<script setup`, `defineProps(`, `<template>`, `createApp(`<br>**Angular**: `@Component(`, `@NgModule(`, `@Directive(`, `@Pipe(` |
| Service | **NestJS/Angular**: `@Injectable(`, `providedIn:`<br>**React**: `export function use`, `export const use` (custom hooks) |
| Repository | `prisma.`, `sequelize.`, `mongoose.`, `mongoose.model(`, `knex(`, `typeorm`, `drizzle(`, `getRepository(`, `@Entity(` |
| Model | `z.object(`, `z.string(`, `export interface `, `export type `, `new Schema(`, `@Schema(`, `@Prop(` |
| Config | `process.env.`, `defineConfig(`, `vite.config`, `next.config`, `nuxt.config`, `astro.config`, `angular.json` |

#### Go (Gin, Echo, Fiber, Chi, stdlib)
| Capa | Senales |
|------|---------|
| Controller | `http.HandleFunc(`, `gin.Context`, `echo.Context`, `fiber.Ctx`, `chi.Router`, `http.Handler`, `func (w http.ResponseWriter` |
| Service | `type Service struct` |
| Repository | `sql.DB`, `gorm.`, `gorm.Model`, `sqlx.DB` |
| Model | `` `json:"`` ``, `` `xml:"`` ``, `` `db:"`` `` |

#### Dart / Flutter (Shelf, dart_frog, Riverpod, Bloc, Drift)
| Capa | Senales |
|------|---------|
| Controller | `extends StatelessWidget`, `extends StatefulWidget`, `Widget build(`, `Route(`, `shelf.Router`, `onRequest(` |
| Service | `extends ChangeNotifier`, `extends Bloc<`, `extends Cubit<`, `StateNotifierProvider`, `riverpod`, `FutureProvider`, `StreamProvider` |
| Repository | `extends DatabaseAccessor`, `@DriftDatabase(`, `@dao`, `FloorDatabase`, `openDatabase(`, `Isar.open(` |
| Model | `@JsonSerializable(`, `@freezed`, `factory `, `fromJson(`, `toJson(` |
| Config | `Platform.environment` |

#### Astro
| Capa | Senales |
|------|---------|
| Controller | `Astro.props`, `Astro.request`, `Astro.redirect(` |
| Config | `astro.config` |

## Nivel 3: Directory Bonus (Secundario)

Los nombres de directorio que siguen convenciones universales (MVC, Clean Architecture, DDD) aportan **+2 puntos** como bonus. Estos NO son determinantes — solo desempatan cuando las senales de contenido son ambiguas.

| Directorio | Capa | Bonus |
|------------|------|-------|
| `controllers/`, `routes/`, `handlers/`, `views/`, `endpoints/`, `api/`, `commands/`, `tools/`, `pages/` | Controller | +2 |
| `services/`, `service/`, `usecases/`, `use_cases/`, `domain/`, `business/`, `logic/` | Service | +2 |
| `repositories/`, `repos/`, `dao/`, `data/`, `db/`, `database/`, `persistence/`, `migrations/` | Repository | +2 |
| `models/`, `entities/`, `schemas/`, `types/`, `dto/`, `proto/` | Model | +2 |
| `utils/`, `helpers/`, `common/`, `shared/`, `lib/`, `middleware/` | Utility | +2 |
| `config/`, `configuration/`, `settings/` | Config | +2 |

## Nivel 4: Fan-in/Fan-out (Refinamiento por Grafo)

Para archivos que aun quedan como **Unknown** despues del scoring, el clasificador analiza su posicion en el **grafo de dependencias** del proyecto:

| Metrica | Significado | Capa asignada |
|---------|-------------|---------------|
| **Alto fan-in, bajo fan-out** | Muchos me importan, yo no importo a nadie | **Model** (dato fundacional) |
| **Alto fan-in, alto fan-out** | Ampliamente usado y usa otros | **Utility** (infraestructura) |
| **Alto fan-out, bajo fan-in** | Yo importo muchos, nadie me importa | **Controller** (orquestador) |
| **fan-out > fan-in (moderado)** | Coordina mas de lo que expone | **Service** (coordinador) |
| **Sin edges** | Modulo aislado | **Unknown** (genuinamente inclasificable) |

El threshold de "alto" es **adaptativo**: se calcula como `max(mediana_fan_in * 2, 3)`, lo que lo ajusta automaticamente al tamano y densidad del proyecto.

### Referencia tecnica

Esta tecnica se basa en:
- **ArchUnit** (TNG): clasificacion por convenciones de paquetes + validacion de dependencias
- **Lattix**: Dependency Structure Matrix (DSM) + particionamiento block-triangular
- **Investigacion academica**: Ego Networks para Layer Recovery (arxiv:2106.03040)
- **Fan-in/Fan-out heuristic**: modulos fundacionales tienen alto fan-in, orquestadores alto fan-out

## Resultados

En void-stack (Rust, 130+ archivos):
- **Antes**: 83% Unknown
- **Despues**: 0.7% Unknown (1 archivo de 66 LOC)

## Tests

- 36 tests unitarios cubriendo:
  - Rust (Actix, Tauri, SQLx, Diesel)
  - Python (FastAPI, Django, SQLAlchemy)
  - JS/TS (NestJS, Next.js, React, Vue, Angular, Prisma, Zod, Express, Mongoose)
  - Go (Gin, GORM)
  - Dart/Flutter (Widgets, Bloc, json_serializable)
  - Astro, MCP
  - Fan-in/Fan-out refinamiento
  - Scoring engine (acumulacion, bonus, content vs path)
  - Edge cases (archivos vacios, aislados, ya clasificados)

## Archivos

- **Motor**: `crates/void-stack-core/src/analyzer/imports/mod.rs`
  - `classify_layer()` — entry point
  - `compute_layer_scores()` — calcula puntajes
  - `refine_unknown_by_graph()` — fan-in/fan-out post-scoring
  - `CONTENT_SIGNALS` — tabla de senales (~120 patrones)
  - `DIR_BONUS` — tabla de bonificacion por directorio
- **Service detection**: `crates/void-stack-core/src/diagram/service_detection.rs`
  - `detect_service_info()` — detecta tipo de servicio (Frontend/Backend/Worker)
  - `extract_port()` — extrae puerto de comandos
