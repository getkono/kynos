# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/getkono/kynos/releases/tag/kynos-v0.1.0) - 2026-08-03

### Added

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

- *(server)* expose the bound server lifecycle
- *(router)* make runtime endpoints mountable
- *(security)* connect authenticators to extraction
- *(middleware)* make built-in policies composable
- *(response)* make content negotiation request-driven
- *(extract)* make marker-backed values constructible
- *(response)* complete typed response contracts
- *(extract)* complete typed extractor contracts

### Other

- document HTTP/3 as demand-driven roadmap work
- add a minimal HTTP/2 server example
- document the anti-patterns and the feature flags
- add feature-matrix, documentation and MSRV gates
- add workspace lints, release profile and shared dependencies
- specify Rust 1.85 as MSRV
- scaffold Kynos workspace and project tooling
