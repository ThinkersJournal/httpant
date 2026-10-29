# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-10-29

- Slice 1b (provisional option A): `httpant-core` gains `Router` (`:name`/`*rest`, exact-method matching, 405 with `Allow`, no path normalisation), the `Handler` trait, Tower layer helpers; `BoxService` is now cloneable. New `httpant-server` with `Server`, the `ServerAdapter` trait and the hyper reference adapter. `httpant-conformance` now runs cases 1-7 over real sockets, each with known-bad mutants that it must fail on (9 mutants). The hello-world example runs in CI. Not published.

## [0.1.0] - 2026-10-29

- **Restart of the version line, taken knowingly (PM 2026-10-29).** The `httpant-*` crates are a new project, not a continuation of `web-server-abstraction` 1.0.x, so the number drops from 1.0.10 to 0.1.0; the old 1.0.x line stays end-of-life on crates.io, not yanked. The version-drop is deliberate.
- Slice 1a of HTTPAnt (provisional option A): workspace with `httpant-core` (http types, streaming `Body` with explicit limits, `Error`, `IntoResponse`, `Capabilities`, boxed Tower service type) and `httpant-conformance` (skeleton: case registry for cases 1-7, raw-TCP HTTP/1.1 client). Removed the old `web-server-abstraction` source tree from `main` (kept in git history); `cargo deny` is now green. Not published.

## [1.0.10] - 2026-10-29

- Added .github/CODEOWNERS and docs/PATENT-SWEEP.md (an honest "not yet performed" record; nothing may be published until it is replaced by a real sweep). Not published.

## [1.0.9] - 2026-10-29

- Added docs/HTTPANT-DESIGN.md: design for HTTPAnt (core, server half, client half, conformance suite, options A/B/C). Docs only, not published.

## [1.0.8] - 2026-10-28

- Added docs/BENCHMARKS.md (TechEmpower R23, Rust JSON and Fortunes) and docs/ACTIX-ADAPTER-NOTES.md (actix service model, actix-tower 0.1.4). Docs only, not published.

## [1.0.7] - 2026-10-28

- README: known-issues section for web-server-abstraction 1.0.2 (superseded by HTTPAnt), from the survey. Docs only, not published.

## [1.0.6] - 2026-10-28

- Added docs/OLD-CRATE-SURVEY.md: survey of this crate for HTTPAnt (what to keep, what to discard, first conformance cases). Docs only, not published.

## [1.0.5] - 2026-10-28

- Added CI (fmt, clippy -D warnings, build+test on ubuntu and windows, doc, cargo deny) and deny.toml, same shape as mcpant; committed Cargo.lock; cargo fmt; h2 updated past RUSTSEC-2026-0258. Not published.

## [1.0.3] - 2026-10-28

- Repository moved to ThinkersJournal/httpant (HTTPAnt); HANDOFF for the new lane. Docs only, not published.

## [1.0.2] - 2025-08-26

### Fixed

- **Documentation Accuracy**: Updated all documentation to reflect the true status of framework adapters
- **Version Consistency**: Corrected version references throughout documentation
- **Framework Status**: All adapters (Rocket, Salvo, Poem) are confirmed working and production-ready

### Changed

- **README.md**: Updated framework status table to show all adapters as complete
- **Copilot Instructions**: Corrected outdated status indicators for framework adapters
- **Cargo.toml**: Updated comments to reflect actual working status of all frameworks

## [1.0.1] - 2025-08-26

### Fixed

- Code quality improvements across all modules
- Resolved clippy warnings and style issues
- Fixed pattern matching and import optimization
- Enhanced error handling consistency
- Updated framework adapter documentation accuracy

### Changed

- Improved test coverage reporting (107/107 tests passing)
- Enhanced middleware performance optimizations
- Better dead code annotations for future features

## [1.0.0] - 2025-08-21

### Added

- Completed all framework adapters (Rocket, Salvo, Poem)
- Stable API with full backward compatibility guarantees
- Mountable interface support for modular applications
- Advanced authentication integration
- Full WebSocket support across all framework adapters
- Comprehensive documentation and examples

### Changed

- Refined middleware API for better ergonomics
- Improved error handling and reporting
- Enhanced performance in core components

### Fixed

- Compilation issues with Rocket adapter
- Memory leaks in long-running connection handling
- Thread safety issues in state management

## [0.3.0] - 2025-06-15

### Added

- Warp adapter implementation
- Advanced routing features (path parameters, wildcards)
- Basic WebSocket support
- HTTP method convenience functions (get, post, put, delete, patch)
- Scaffolding for Rocket, Salvo, and Poem adapters

### Changed

- Improved request/response handling
- Enhanced middleware composition
- More efficient path matching algorithm

## [0.2.0] - 2025-04-10

### Added

- Complete Axum adapter
- Actix-Web adapter
- Enhanced middleware ecosystem (9 middleware types)
- Performance benchmarks and profiling infrastructure
- Improved testing utilities

### Changed

- Refactored core abstractions for better performance
- Enhanced error handling with more context
- Improved documentation

## [0.1.0] - 2025-02-28

### Added

- Initial release
- Core abstractions and API design
- Mock adapter for testing
- Basic middleware system
- Documentation and examples
