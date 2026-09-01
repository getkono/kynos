# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/getkono/kynos/compare/kynos-openapi-v0.1.0...kynos-openapi-v0.2.0) - 2026-09-01

### Fixed

- *(openapi)* compare a header parameter's name the way HTTP compares one

## [0.1.0](https://github.com/getkono/kynos/releases/tag/kynos-openapi-v0.1.0) - 2026-08-29

### Added

- *(openapi)* convert between a described method and an HTTP one
- *(openapi)* [**breaking**] keep `openapi32` additive for a downstream `match`
- *(kynos)* answer a multi-range request with every part it asked for
- *(openapi)* build an oauth2 scheme through the model's own constructors
- *(kynos)* let a short circuit name the statuses it answers with
- *(openapi)* let a violation be a standard error
- *(openapi)* report opaque operations and routes
- *(openapi)* give opacity a vocabulary both sides can agree on
- *(openapi)* parse a method from its wire spelling
- *(openapi)* add the OpenAPI 3.1 and 3.2 document model

### Documentation

- correct six claims the code does not support
- *(openapi)* demonstrate the model without a runtime

### Fixed

- *(router)* restore the feature gates the two splits displaced
- *(openapi)* refuse `encoding` beside `prefixEncoding` or `itemEncoding`
- *(openapi)* [**breaking**] let every extensible object carry its extensions
- *(openapi)* validate every operation the document describes
- *(openapi)* accept a 3.2 security requirement named by a URI
- *(openapi)* ask whether a response is declared, not whether anything is
- *(openapi)* check the key of every component, not five sections' worth
- *(openapi)* [**breaking**] put a response's description where the requirement is true
- *(openapi)* keep a JSON null that a description actually wrote
- *(openapi)* [**breaking**] seal the variants a 3.2 field is added to
- *(openapi)* read a Server Object wherever one hangs
- *(openapi)* always declare `paths`, so no document declares nothing
- *(openapi)* walk every container a 3.2 construct can sit in
- *(openapi)* refuse a downgrade the walk was blind to
- *(router)* hand an unchecked route what the matcher captured, and a name
- *(openapi)* report a security scheme only 3.2 can hold
- *(openapi)* report a media type violation where it lives
- *(openapi)* escape a media type in a blocker's location
- *(openapi)* default explode to true for the cookie style
- *(openapi)* report a Content-Type header the specification ignores
- *(openapi)* stop a violation repeating itself as its own cause
- *(openapi)* stop dropping a responses object and shuffling violations
- *(openapi)* report a paths key the grammar rejects
- *(openapi)* reject an empty path segment
- *(openapi)* stop the opacity record losing what it exists to keep
- *(openapi)* enforce the path grammar on literal segments

### Other

- *(openapi)* [**breaking**] let an encoding declare only a query style
- *(openapi)* [**breaking**] let a header declare only the style it may
- *(openapi)* [**breaking**] let a link name one target
- *(openapi)* [**breaking**] let a parameter carry one form of example
- *(openapi)* [**breaking**] let a media type carry one form of example
- *(openapi)* [**breaking**] let a parameter name one shape
- *(openapi)* [**breaking**] let an example carry the forms it may
- *(openapi)* [**breaking**] let a license name one link
- *(openapi)* [**breaking**] keep a path-template failure structured
- *(kynos)* [**breaking**] give middleware the operation it is covering
