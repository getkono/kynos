# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/getkono/kynos/releases/tag/kynos-v0.1.0) - 2026-08-10

### Added

- *(server)* implement production transport infrastructure
- *(codec)* support eight representation alternatives
- *(unchecked)* add tower service integration
- *(router)* add canonical trailing-slash redirects
- *(macros)* generate typed endpoint uris
- *(codec)* add protobuf bodies
- *(response)* add SSE keep-alive configuration
- *(response)* add streaming and multipart bodies
- *(response)* add typed response header contracts
- *(extract)* support optional and alternative request bodies
- gate JSON codecs behind a default feature
- make panic recovery a static policy
- *(macros)* add the procedural macro surface
- add unchecked escape hatches behind a feature
- add the in-process test client behind test-util
- add the public API surface of the framework
- *(openapi)* add the OpenAPI 3.1 and 3.2 document model

### Fixed

- *(kynos)* gate the alternative imports on having a codec at all
- *(server)* complete graceful shutdown lifecycle
- *(server)* expose the bound server lifecycle
- *(router)* make runtime endpoints mountable
- *(security)* connect authenticators to extraction
- *(middleware)* make built-in policies composable
- *(response)* make content negotiation request-driven
- *(extract)* make marker-backed values constructible
- *(response)* complete typed response contracts
- *(extract)* complete typed extractor contracts

### Other

- *(kynos)* deepen di, error, http, schema and security
- *(kynos)* split the server into transport, lifecycle, shutdown and tls
- *(kynos)* give each middleware its own module
- *(kynos)* split router into endpoint, group, policy and service
- *(kynos)* split response into status, headers, negotiation and streams
- *(kynos)* split extract into params, body and media
- *(kynos)* move macro support items into __private
- *(openapi)* group the object model under model
- clarify tokio-only runtime policy
- add architecture, middleware, and NFR design documents
- *(server)* add graceful shutdown example
- *(server)* expose graceful shutdown gaps
- assert per-test process isolation
- document HTTP/3 as demand-driven roadmap work
- add a minimal HTTP/2 server example
- document the anti-patterns and the feature flags
- add feature-matrix, documentation and MSRV gates
- add workspace lints, release profile and shared dependencies
- specify Rust 1.85 as MSRV
- scaffold Kynos workspace and project tooling
