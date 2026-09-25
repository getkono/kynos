# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/getkono/kynos/compare/kynos-macros-v0.2.0...kynos-macros-v0.3.0) - 2026-09-25

### Documentation

- *(macros)* say the path capture lookup sits behind an ordered check
- *(macros)* link the QueryParams object-field limit to #216
- *(macros)* state the object-field limit of the QueryParams derive
- *(macros)* say the path parameter check is ordered
- *(macros)* state what the parameter derives actually check
- *(macros)* name the two shapes serde lends every key in the Schema rustdoc
- *(schema)* state the tag, transparent and two-refusal cases of ClosedFlatten
- *(schema)* state which flattened types a closed object admits
- *(macros)* list where the Schema derive refuses a wire-form override
- *(schema)* state the transparent override scan by each direction's single pick

### Fixed

- *(macros)* withhold ClosedFlatten from a struct carrying a serde tag
- *(schema)* [**breaking**] bound a closed object's flattened field by ClosedFlatten
- *(macros)* refuse a wire-form override on a flattened PhantomData serde reads
- *(macros)* scan a transparent struct only through the field serde picks per direction

### Other

- *(schema)* [**breaking**] move the flatten markers to kynos::schema::flatten

## [0.2.0](https://github.com/getkono/kynos/compare/kynos-macros-v0.1.0...kynos-macros-v0.2.0) - 2026-09-24

### Documentation

- *(macros)* scope the flattened-field refusal to fields the skip rule checks
- *(macros)* restate why positional_members reads skip attributes
- *(macros)* state the tuple-member skip exemptions
- *(macros)* carve the flattened and unwritten cases out of the skip rule
- *(macros)* state rule 30's open-field half by the AdmitsAny bound
- *(schema)* record how a variant name two variants share is described
- *(macros)* list the deny_unknown_fields refusals beside their snapshots
- *(macros)* list a skipped adjacently tagged payload as refused, with its snapshot
- *(macros)* bind the newtype comparison to the component name alone
- *(schema)* state what open takes and drops where it is written
- *(errors)* say what the branch title is, and where the dedup drops it

### Fixed

- *(schema)* let a map of Unchecked values sit open beside a field serde never reads
- *(macros)* bound an open field beside a field serde never reads by AdmitsAny
- *(schema)* let an Unchecked map payload be flattened as an open map
- *(macros)* describe a variant name two variants share under the first alone
- *(macros)* describe every name serde reads a variant under
- *(macros)* describe every name serde reads a field under
- *(macros)* close each externally tagged object branch to its variant key
- *(macros)* refuse only a flattened open field for being in a closed object
- *(macros)* close an object serde reads under deny_unknown_fields
- *(macros)* describe a PhantomData member as the null serde writes
- *(macros)* check an adjacent skipped payload on every described variant
- *(macros)* count a tuple's minItems only up to its last member serde does not default
- *(macros)* scan a transparent tuple only on the member serde picks
- *(macros)* honour a container default on a tuple struct
- *(macros)* refuse a skipped adjacently tagged newtype payload that is not an Option
- *(macros)* say the transparent refusal compares fields, not schemas
- *(macros)* refuse a transparent struct only where serde's two picks differ
- *(macros)* recognise a PhantomData a macro wraps in a group
- *(macros)* refuse a transparent struct by the field serde picks each way
- *(macros)* refuse a transparent struct serde may read and write through different fields
- *(macros)* describe a transparent struct by its one described field
- *(macros)* refuse a container serde reads or writes through another type
- *(macros)* name the working remedy for a flattened map and struct alike
- *(macros)* decide a flattened field by open alone, whatever default it carries
- *(macros)* let a flattened open map skip itself on write
- *(macros)* describe no Flatten claim for a variant serde never writes
- *(macros)* [**breaking**] hold a tagged newtype variant's payload to Flatten
- *(macros)* allow #[schema(open)] only over a map
- *(macros)* read serde's transparent before claiming Flatten
- *(macros)* read the `open` key without a let chain
- *(macros)* flatten only what names its own members
- *(kynos)* narrow each derived error response to the type it publishes
- *(macros)* unindent the paragraph rustdoc reads as a code block

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
