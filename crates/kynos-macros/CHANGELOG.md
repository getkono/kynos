# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/getkono/kynos/releases/tag/kynos-macros-v0.1.0) - 2026-08-29

### Added

- *(openapi)* [**breaking**] keep `openapi32` additive for a downstream `match`
- *(assets)* mint a validator per stored content coding
- *(kynos)* compile a directory into the binary as described operations
- *(kynos)* [**breaking**] let a scheme say where its credential is carried
- *(kynos)* [**breaking**] let a multipart form travel in both directions
- *(macros)* expand the Tag derive into its metadata
- *(macros)* expand the SecurityScheme derive into its scheme
- *(macros)* expand the Reply derive into a response
- *(macros)* expand the ApiError derive into its problem
- *(macros)* expand the Schema derive into a description
- *(macros)* expand each parameter derive into an implementation
- *(kynos)* [**breaking**] let conflicting interceptors fail to compile
- *(kynos)* let a short circuit name the statuses it answers with
- *(macros)* parse and validate the reply attribute
- *(macros)* [**breaking**] reject a format annotation on a field
- *(macros)* parse and validate the problem attribute
- *(kynos)* [**breaking**] make an async fn mountable
- *(macros)* derive one Provides implementation per context field
- *(macros)* expand every derive to a placeholder implementation
- *(macros)* generate typed endpoint uris
- *(response)* add typed response header contracts
- make panic recovery a static policy
- *(macros)* add the procedural macro surface
- *(openapi)* add the OpenAPI 3.1 and 3.2 document model

### Documentation

- correct what a v0.1.0 reader would be misled by
- *(macros)* let the OPTIONS attribute say what now answers a preflight
- *(macros)* stop the derive documentation promising what it does not do

### Fixed

- *(macros)* [**breaking**] refuse a 3.2-only member rather than dropping it
- *(assets)* keep a coding whose base is not a resource
- *(kynos)* send the charset RFC 7617 asks a basic challenge to name
- *(macros)* emit the oauth2 flows the attribute declared
- *(macros)* stop a route's tag being parsed and discarded
- run the compile-fail suite in CI, and stop three signatures freezing wrong
- *(openapi)* stop dropping a responses object and shuffling violations
- *(macros)* let the generic operation attribute keep its own argument
- *(macros)* correct three ways a derive misreads or mis-emits

### Other

- *(kynos)* [**breaking**] split the parameter groups' two directions into traits
- *(macros)* [**breaking**] name the typed uri for what it renders
- *(macros)* [**breaking**] name each parameter derive after its trait
- *(kynos)* [**breaking**] give each extractor its own rejection
- *(kynos)* [**breaking**] make the escape hatches usable and the schemes describable
