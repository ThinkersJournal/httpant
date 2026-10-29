# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
