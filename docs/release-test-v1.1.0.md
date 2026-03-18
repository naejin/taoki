# Taoki v1.1.0 Release Test Report

**Date:** 2026-03-18
**Binary:** `target/release/taoki` (built from commit `d865f12`)
**Test method:** Direct JSON-RPC to the release binary (not via plugin, to test the exact artifact)
**Tests:** 187 unit tests passing, 0 clippy warnings

---

## Test Matrix

12 real-world open-source repos across all 5 supported languages, plus 1 mixed-language repo. Each repo tested with all 3 tools (radar, xray, ripple).

| # | Repo | Language | Stars | Size | Category |
|---|------|----------|-------|------|----------|
| 1 | [axum](https://github.com/tokio-rs/axum) | Rust | 20k+ | workspace, 59 src files | Web framework |
| 2 | [serde](https://github.com/serde-rs/serde) | Rust | 9k+ | workspace, 255 files | Serialization |
| 3 | [requests](https://github.com/psf/requests) | Python | 52k+ | single package, 25 files | HTTP client |
| 4 | [fastapi](https://github.com/tiangolo/fastapi) | Python | 80k+ | package + docs, 49 src files | Web framework |
| 5 | [vite](https://github.com/vitejs/vite) | TypeScript | 70k+ | monorepo, 274 src files | Build tool |
| 6 | [trpc](https://github.com/trpc/trpc) | TypeScript | 35k+ | monorepo, 135 server src files | RPC framework |
| 7 | [next.js](https://github.com/vercel/next.js) | TypeScript | 130k+ | mega monorepo, 578 server files | React framework |
| 8 | [gin](https://github.com/gin-gonic/gin) | Go | 80k+ | multi-package, 100 files | Web framework |
| 9 | [hugo](https://github.com/gohugoio/hugo) | Go | 78k+ | 191 packages, 1152 files | Static site gen |
| 10 | [jackson-databind](https://github.com/FasterXML/jackson-databind) | Java | 3.5k+ | Maven, 617 src files | JSON binding |
| 11 | [spring-boot](https://github.com/spring-projects/spring-boot) | Java | 76k+ | deep multi-module, 592 core files | App framework |
| 12 | [deno](https://github.com/denoland/deno) | Rust + TS | 100k+ | mega workspace, mixed languages | JS runtime |

---

## Radar Results

| Repo | Files Scanned | Grouping | Tags Detected | Status |
|------|---------------|----------|---------------|--------|
| axum | 59 (globs) | flat | `[barrel-file]`, `[data-models]`, `[error-types]`, `[module-root]` | PASS |
| serde | 255 (full) | grouped (>100) | `[entry-point]`, `[barrel-file]`, `[module-root]`, `[data-models]` | PASS |
| requests | 25 (full) | flat | `[data-models]`, `[error-types]`, `[entry-point]`, `[module-root]` | PASS |
| fastapi | 49 (globs) | flat | `[module-root]` | PASS |
| vite | 274 (globs) | grouped (>100) | `[barrel-file]`, `[data-models]`, `[interfaces]`, `[module-root]` | PASS |
| trpc | 135 (globs) | grouped (>100) | `[data-models]`, `[module-root]`, `[tests]` | PASS |
| next.js | 578 (globs) | grouped (>100) | `[tests]`, `[data-models]`, `[error-types]` | PASS |
| gin | 100 (full) | flat (=threshold) | `[tests]`, `[http-handlers]`, `[error-types]`, `[interfaces]` | PASS |
| hugo | 1152 (full) | grouped (>100) | All tag types represented | PASS |
| jackson-databind | 617 (globs) | grouped (>100) | `[interfaces]`, `[error-types]`, `[data-models]` | PASS |
| spring-boot | 592 (globs) | grouped (>100) | `[interfaces]`, `[error-types]`, `[data-models]` | PASS |
| deno | Mixed (globs) | depends on scope | `[entry-point]`, `[cli]`, `[module-root]` | PASS |

**Notes:**
- Glob filtering works correctly on all monorepos
- Directory grouping activates above 100 files as designed
- Heuristic tags are accurate across all languages
- No crashes, no empty results, no false positives observed

---

## Xray Results

| Repo | File | Source Lines | Xray Lines | Reduction | Status |
|------|------|-------------|------------|-----------|--------|
| axum | `axum/src/routing/mod.rs` | 597 | ~40 | 93% | PASS |
| serde | `serde_core/src/de/mod.rs` | 2,392 | 290 | 88% | PASS |
| requests | `src/requests/sessions.py` | 835 | ~40 | 95% | PASS |
| fastapi | `fastapi/routing.py` | 4,956 | 3,412 | 31% | PASS* |
| vite | `packages/vite/src/node/server/index.ts` | ~1,200 | ~50 | 96% | PASS |
| trpc | `packages/server/src/@trpc/server/index.ts` | 145 | 3 | 98% | PASS |
| next.js | `packages/next/src/server/next-server.ts` | 2,170 | 504 | 77% | PASS |
| gin | `gin.go` | ~800 | ~50 | 94% | PASS |
| hugo | `hugolib/hugo_sites.go` | 816 | 187 | 77% | PASS |
| jackson-databind | `ObjectMapper.java` | 2,848 | ~50 | 98% | PASS |
| spring-boot | `SpringApplication.java` | 1,876 | 223 | 88% | PASS |
| deno | `cli/args/flags.rs` | 15,307 | ~50 | 99.7% | PASS |

*`fastapi/routing.py` has low reduction because it's heavily decorated with complex type annotations that are structurally significant — xray correctly preserves them.

**Notes:**
- Token reduction ranges from 31% to 99.7%, averaging ~85%
- The claimed 70-90% reduction holds for typical source files
- Doc comment extraction works for all languages (Rust `///`, Python docstrings, Go comments, Java `/**`, TS `/**`)
- Body insights (`-> calls:`, `-> methods:`, `-> match:`, `-> errors:`) render correctly in all languages
- Line number ranges are accurate for jumping to source

---

## Ripple Results

### Rust

| Repo | File | depends_on | used_by | external | Status |
|------|------|-----------|---------|----------|--------|
| axum | `routing/mod.rs` | 8 (incl. cross-crate `axum-core`) | 80+ (examples, tests, macros crate) | 8 | PASS |
| serde | `de/mod.rs` | 1 (`serde_core/src/lib.rs`) | 3 (`impls.rs`, `value.rs`, `ignored_any.rs`) | 4 | PASS |
| deno | `cli/args/flags.rs` | 7 (cross-crate: `cli/lib/`, `libs/config/`) | 0 | 10+ | PASS |

**Notes:**
- Cross-crate workspace resolution works (axum-core, serde_core)
- Custom `[[bin]] path` resolution works (deno has non-standard layout)
- Symbol extraction in depends_on/used_by entries is accurate

### Python

| Repo | File | depends_on | used_by | external | Status |
|------|------|-----------|---------|----------|--------|
| requests | `sessions.py` | 11 with symbols | 2 (`__init__.py`, `test_requests.py`) | 5 | PASS |
| fastapi | `routing.py` | 10 with symbols | 12 (docs, tests, `__init__.py`) | 23 | PASS |

**Notes:**
- Relative imports (`.adapters`, `._compat`) resolve correctly
- Absolute imports with `src/` layout resolve via `__init__.py` discovery
- Symbol extraction from `from X import Y, Z` is accurate

### TypeScript

| Repo | File | depends_on | used_by | external | Status |
|------|------|-----------|---------|----------|--------|
| vite | `server/index.ts` | 29+ internal | not shown (deep graph) | 10+ node: builtins | PASS |
| trpc | `@trpc/server/index.ts` | 0 (re-exports only) | 17 (adapters, tests) | 0 | PASS |
| next.js | `next-server.ts` | 38+ (deep dependency tree) | not shown (scoped) | many | PASS |

**Notes:**
- Relative import resolution works across monorepo package boundaries
- `type` imports and `* as` imports are handled correctly
- next.js with 578+ TypeScript files resolves without performance issues

### Go

| Repo | File | depends_on | used_by | co-package | external | Status |
|------|------|-----------|---------|------------|----------|--------|
| gin | `gin.go` | 3 (`internal/bytesconv`, `internal/fs`, `render`) | 0 | n/a (has internal deps) | 11 | PASS |
| hugo | `hugo_sites.go` | 21 (cross-package: `cache/`, `common/`, `config/`, `deps/`, etc.) | 0 | n/a (has internal deps) | 13 | PASS |

**Notes:**
- Cross-package resolution works on both small (gin, 100 files) and large (hugo, 1152 files) projects
- Go module map correctly resolves `github.com/gohugoio/hugo/...` paths
- `co-package:` section is correctly suppressed when internal deps exist (both gin and hugo have them)
- `co-package:` was verified on cobra (single-package library) in earlier testing — lists 13 sibling files

### Java

| Repo | File | depends_on | used_by | external | Status |
|------|------|-----------|---------|----------|--------|
| jackson-databind | `ObjectMapper.java` | 12 with symbols | 16+ (tests, implementations) | 5 | PASS |
| spring-boot | `SpringApplication.java` | 10 (`Banner`, `BootstrapRegistry`, `Binder`, etc.) | 28+ (build plugins, autoconfigure, tests) | 65 | PASS |

**Notes:**
- Universal suffix-based matching resolves all internal imports regardless of source root layout
- jackson-databind: `tools.jackson.databind.cfg.BaseSettings` resolves to `src/main/java/tools/jackson/databind/cfg/BaseSettings.java`
- spring-boot: `org.springframework.boot.Banner.Mode` resolves to `core/spring-boot/src/main/java/org/springframework/boot/Banner.java` (deep nested layout)
- Static imports resolve correctly (progressive segment stripping)
- Wildcard imports (`import org.springframework.boot.*`) resolve to a file in the matching package

---

## Summary

| Tool | Rust | Python | TypeScript | Go | Java | Overall |
|------|------|--------|------------|-----|------|---------|
| **Radar** | PASS (3/3) | PASS (2/2) | PASS (3/3) | PASS (2/2) | PASS (2/2) | **12/12** |
| **Xray** | PASS (3/3) | PASS (2/2) | PASS (3/3) | PASS (2/2) | PASS (2/2) | **12/12** |
| **Ripple** | PASS (3/3) | PASS (2/2) | PASS (3/3) | PASS (2/2) | PASS (2/2) | **12/12** |
| **Total** | **9/9** | **6/6** | **9/9** | **6/6** | **6/6** | **36/36** |

### Key Improvements Since v1.0.0

1. **Java resolution**: Replaced hardcoded source root prefixes with universal suffix-based matching. Now works on any project layout (guava, spring-boot, jackson, Maven, flat).
2. **Rust resolution**: Added source directory detection for non-standard `[[bin]]/[lib]` paths in Cargo.toml. Custom binary layouts (ripgrep, deno) now resolve correctly.
3. **Go context**: Added `co-package:` section for single-package libraries where no file-level imports exist. Cross-package resolution was already working.
4. **Cache invalidation**: Two-layer incremental deps cache with per-file blake3 hashes and fingerprint over file list + workspace config. Stale caches are automatically detected and rebuilt.
5. **Xray pruning**: Dead entries removed during radar calls, preventing unbounded cache growth.
6. **Unified versioning**: Single `CACHE_VERSION` in `src/cache.rs` for all cache formats.

### Known Limitations

- **Go intra-package**: File-level dependencies within a single Go package cannot be traced (Go has no file-level imports within a package). The `co-package:` section provides context for this case.
- **Rust `pub use` re-exports**: Re-exports like `pub use self::method_routing::get` appear in the external section rather than being traced to the defining file.
- **TypeScript path aliases**: `tsconfig.json` path aliases (e.g., `@/components/...`) are not resolved — only relative imports are traced.
