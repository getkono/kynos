# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/getkono/kynos/compare/kynos-openapi-v0.4.0...kynos-openapi-v0.5.0) - 2026-10-24

### Added

- *(openapi)* add PathItem::operations_mut over every operation ([#886](https://github.com/getkono/kynos/pull/886))

### Documentation

- correct the prose a third audit pass found drifting ([#962](https://github.com/getkono/kynos/pull/962))
- correct the rustdoc a second audit pass found drifting from its code ([#949](https://github.com/getkono/kynos/pull/949))
- align kynos-openapi and kynos-macros rustdoc with the code ([#909](https://github.com/getkono/kynos/pull/909))
- correct claims the code contradicts or has outgrown ([#872](https://github.com/getkono/kynos/pull/872))

### Fixed

- *(macros)* [**breaking**] declare a problem's extension members in its response ([#1006](https://github.com/getkono/kynos/pull/1006))
- *(containment)* close the gate's false negatives and correct the feature and profile prose ([#950](https://github.com/getkono/kynos/pull/950))
- build on the declared MSRV without let-chains ([#937](https://github.com/getkono/kynos/pull/937))
- *(openapi)* report a tag parent cycle only at the tags on it ([#933](https://github.com/getkono/kynos/pull/933))
- *(router)* [**breaking**] refuse two tag types describing one name differently ([#896](https://github.com/getkono/kynos/pull/896))
- *(router)* [**breaking**] refuse a policy set on a nested or merged router ([#895](https://github.com/getkono/kynos/pull/895))
- *(openapi)* stop the discriminator memo keeping verdicts reached through a cut cycle ([#864](https://github.com/getkono/kynos/pull/864))
- *(openapi)* check the Discriminator Object's MUSTs ([#855](https://github.com/getkono/kynos/pull/855))
- *(openapi)* resolve a Link's operationId and local operationRef ([#853](https://github.com/getkono/kynos/pull/853))
- *(openapi)* report a not-authoritative stamp on an authoritative document ([#852](https://github.com/getkono/kynos/pull/852))
- *(openapi)* require each OAuth flow's REQUIRED URLs ([#849](https://github.com/getkono/kynos/pull/849))
- *(openapi)* enforce the Media Type and Encoding Object MUSTs at every level ([#846](https://github.com/getkono/kynos/pull/846))
- *(openapi)* reject unrecognised non-x- fields under 3.2 as under 3.1 ([#845](https://github.com/getkono/kynos/pull/845))
- *(openapi)* resolve a same-document security scheme pointer in requirements ([#843](https://github.com/getkono/kynos/pull/843))
- *(openapi)* refuse additionalOperations entries for fixed-field methods ([#840](https://github.com/getkono/kynos/pull/840))
- *(openapi)* check Server Variable MUSTs on every Server Object ([#836](https://github.com/getkono/kynos/pull/836))
- *(openapi)* refuse response keys the model cannot read back ([#834](https://github.com/getkono/kynos/pull/834))
- *(openapi)* refuse a Callback item keyed like an extension ([#832](https://github.com/getkono/kynos/pull/832))
- *(openapi)* refuse to emit as 3.1 a document 3.1 cannot express ([#829](https://github.com/getkono/kynos/pull/829))
- *(openapi)* name a parameter style as the description spells it in violations ([#828](https://github.com/getkono/kynos/pull/828))
- *(openapi)* run the content rules over default and component responses and request bodies ([#820](https://github.com/getkono/kynos/pull/820))
- *(openapi)* keep unknown fields when rewriting the opaque annotations ([#822](https://github.com/getkono/kynos/pull/822))
- *(openapi)* translate ECMA-262's \0 instead of calling it a backreference ([#812](https://github.com/getkono/kynos/pull/812))
- *(openapi)* admit ? and # inside a path template expression ([#817](https://github.com/getkono/kynos/pull/817))
- *(openapi)* report violation and blocker locations as escaped JSON Pointers ([#802](https://github.com/getkono/kynos/pull/802))
- *(openapi)* refuse \p{...} property names ECMA-262 does not spell ([#644](https://github.com/getkono/kynos/pull/644))

### Other

- *(openapi)* [**breaking**] gate YAML emission on one pub mod yaml ([#940](https://github.com/getkono/kynos/pull/940))
- *(router)* [**breaking**] move the router's own violations out of SpecError ([#928](https://github.com/getkono/kynos/pull/928))
- *(openapi)* [**breaking**] model xml nodeType as an enum excluding attribute and wrapped ([#927](https://github.com/getkono/kynos/pull/927))
- *(openapi)* [**breaking**] remove SpecError::EmptyDocument, which nothing raises ([#887](https://github.com/getkono/kynos/pull/887))

## [0.4.0](https://github.com/getkono/kynos/compare/kynos-openapi-v0.3.0...kynos-openapi-v0.4.0) - 2026-10-13

### Added

- *(schema)* [**breaking**] enforce a map key's pattern ([#528](https://github.com/getkono/kynos/pull/528))

### Documentation

- *(security)* say a scheme's default scopes are demanded by every guard naming it ([#578](https://github.com/getkono/kynos/pull/578))
- badge every feature-gated item with its feature on docs.rs ([#518](https://github.com/getkono/kynos/pull/518))
- *(features)* drop the manifests' copies of the openapi32 rationale
- *(openapi)* document the edge-underscore trim in ComponentName::sanitized
- *(openapi)* state what the facade and Paths::is_empty actually cover

### Fixed

- *(validate)* check each reusable parameter once, where it is defined ([#519](https://github.com/getkono/kynos/pull/519))
- *(validate)* resolve a $ref'd parameter before path correspondence and uniqueness ([#486](https://github.com/getkono/kynos/pull/486))
- *(validate)* enforce the in: querystring parameter rules ([#496](https://github.com/getkono/kynos/pull/496))
- *(validate)* walk the operations inside an operation's inline callbacks ([#509](https://github.com/getkono/kynos/pull/509))
- *(emit)* refuse a 3.1 emission carrying unrecognised fields ([#491](https://github.com/getkono/kynos/pull/491))
- *(validate)* check document-level security requirements ([#494](https://github.com/getkono/kynos/pull/494))
- *(router)* report match-table refusals accurately, from every entry point
- *(openapi)* report a duplicate operation as SpecError::DuplicateOperation
- *(openapi)* [**breaking**] refuse non-canonical response status keys

### Other

- *(openapi)* [**breaking**] return an owned YamlError from Document::to_yaml
- *(openapi)* [**breaking**] gate the http conversions on one module line

## [0.2.0](https://github.com/getkono/kynos/compare/kynos-openapi-v0.1.0...kynos-openapi-v0.2.0) - 2026-09-24

### Added

- *(openapi)* union two narrowed problem responses on a shared status

### Documentation

- *(tests)* say which feature sets a calibration was read at
- *(openapi)* state the union's third case where the rule is documented
- *(openapi)* say what the unread arm actually knows
- *(openapi)* put the empty-oneOf note on the arm that answers it

### Fixed

- *(openapi)* report every unchecked schema a document holds, at its own pointer
- *(openapi)* adopt only a side that provably admits every document
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
