# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/getkono/kynos/compare/kynos-v0.4.0...kynos-v0.5.0) - 2026-10-31

### Added

- *(range)* let a ranged delivery choose its media type at request time
- *(stream)* re-export the Stream trait the streaming responses are bounded by
- *(macros)* let #[derive(HeaderParams)] set DESCRIBED = false ([#1075](https://github.com/getkono/kynos/pull/1075))
- *(json)* [**breaking**] key a serde failure at the JSON Pointer where it happened ([#1017](https://github.com/getkono/kynos/pull/1017))
- *(multipart)* [**breaking**] read and write every ParamValue as a text part ([#1013](https://github.com/getkono/kynos/pull/1013))
- *(macros)* let a Reply variant hold a response type such as Created ([#1007](https://github.com/getkono/kynos/pull/1007))
- *(security)* let a guard whose scheme never forbids declare 401 alone ([#1010](https://github.com/getkono/kynos/pull/1010))
- *(security)* let a scope set apply to an AnyOf requirement ([#1003](https://github.com/getkono/kynos/pull/1003))
- *(extract)* open the body codec set to a user-implemented codec ([#997](https://github.com/getkono/kynos/pull/997))
- *(security)* let an authenticator answer 503 when its store is unavailable ([#995](https://github.com/getkono/kynos/pull/995))
- *(extract)* carry a UTF-8 text body under a declared text/* media type ([#994](https://github.com/getkono/kynos/pull/994))
- *(extract)* carry a typed JSON body under a declared +json media type ([#990](https://github.com/getkono/kynos/pull/990))
- *(security)* [**breaking**] challenge an api_key and mutual-TLS 401 ([#932](https://github.com/getkono/kynos/pull/932))
- *(openapi)* add PathItem::operations_mut over every operation ([#886](https://github.com/getkono/kynos/pull/886))
- *(cache)* judge which stored variant a put replaces with StoredResponse::same_variant ([#884](https://github.com/getkono/kynos/pull/884))
- *(server)* serve one connection over any AsyncRead + AsyncWrite ([#584](https://github.com/getkono/kynos/pull/584))

### Documentation

- *(ranged)* demonstrate a media type chosen per request in the example
- *(stream)* publish the Stream re-export as futures-core's item, not a kynos page
- *(kynos)* make doc comments match the code beneath them ([#1080](https://github.com/getkono/kynos/pull/1080))
- correct the prose a third audit pass found drifting ([#962](https://github.com/getkono/kynos/pull/962))
- correct the claims a second audit found the code contradicts ([#948](https://github.com/getkono/kynos/pull/948))
- correct the rustdoc a second audit pass found drifting from its code ([#949](https://github.com/getkono/kynos/pull/949))
- align response, router, extract and schema rustdoc with the code ([#939](https://github.com/getkono/kynos/pull/939))
- *(language)* say client range order wins over Lookup ([#908](https://github.com/getkono/kynos/pull/908))
- *(extract)* state JSON Lines fail-fast on malformed records as policy ([#906](https://github.com/getkono/kynos/pull/906))
- trim duplicated and narrated rationale from docs/ ([#903](https://github.com/getkono/kynos/pull/903))
- *(kynos)* make middleware, server, http, security, unchecked and test rustdoc match the code ([#891](https://github.com/getkono/kynos/pull/891))
- *(examples)* make example comments say what the examples do ([#892](https://github.com/getkono/kynos/pull/892))
- correct claims the code contradicts or has outgrown ([#872](https://github.com/getkono/kynos/pull/872))

### Fixed

- *(range)* hold a MediaRanges entry to the parameter grammar at build time
- *(macros)* name HeaderParams and CookieParams in their shape refusals ([#1112](https://github.com/getkono/kynos/pull/1112))
- *(security)* drop the claims and scope bounds from the traits on the built-in schemes, AnyOf and AllOf
- *(extract)* drop the media type marker bound from JsonAs, TextAs, Binary and QueryString ([#1085](https://github.com/getkono/kynos/pull/1085))
- *(media)* read a marker's charset only from a parameter outside quotes ([#1103](https://github.com/getkono/kynos/pull/1103))
- *(media)* [**breaking**] refuse a malformed media type marker at compile time ([#1101](https://github.com/getkono/kynos/pull/1101))
- *(response)* leave a $ref 200 in place under a status wrapper ([#1098](https://github.com/getkono/kynos/pull/1098))
- *(macros)* call prelude traits by fully qualified paths in derive expansions ([#1096](https://github.com/getkono/kynos/pull/1096))
- *(cookie)* match the __Host- and __Secure- prefixes in any case ([#1093](https://github.com/getkono/kynos/pull/1093))
- *(extract)* split body media-type parameters outside quoted values ([#1084](https://github.com/getkono/kynos/pull/1084))
- *(test)* match a response content key that is a media range ([#1070](https://github.com/getkono/kynos/pull/1070))
- *(macros)* refuse a repeated assets! dir, exclude or warn_over key ([#1074](https://github.com/getkono/kynos/pull/1074))
- *(rate-limit)* round the RateLimit-Policy window up to whole seconds ([#1061](https://github.com/getkono/kynos/pull/1061))
- *(macros)* name prelude items by their absolute paths in expansions ([#1072](https://github.com/getkono/kynos/pull/1072))
- *(openapi)* [**breaking**] type a content map entry as a media type or a reference ([#1068](https://github.com/getkono/kynos/pull/1068))
- *(schema)* set aside a self-unequal item before the uniqueness sort ([#1067](https://github.com/getkono/kynos/pull/1067))
- *(json)* read a nested struct and a struct record only from an object ([#1016](https://github.com/getkono/kynos/pull/1016))
- *(reply)* stop join repeating a problem description already unioned ([#1014](https://github.com/getkono/kynos/pull/1014))
- *(json)* read a top-level struct body only from an object ([#1008](https://github.com/getkono/kynos/pull/1008))
- *(macros)* [**breaking**] declare a problem's extension members in its response ([#1006](https://github.com/getkono/kynos/pull/1006))
- *(macros)* describe an Option<T> parameter by T's schema ([#996](https://github.com/getkono/kynos/pull/996))
- *(test)* match a path against a template segment mixing an expression and a literal
- *(limits)* hold a concurrency slot until the response body is done
- *(router)* recover a panic in an unchecked layer
- *(negotiate)* split Accept outside quoted parameter values
- *(range)* answer 412 to a false If-None-Match on an unsafe method
- *(cookie)* send Secure with a Partitioned cookie
- *(rate-limit)* key an IPv4-mapped peer by its IPv4 host
- *(security)* refuse an API key header sent in two fields
- *(security)* refuse a bearer token outside the b64token grammar
- *(cache)* build on the declared MSRV without let-chains ([#964](https://github.com/getkono/kynos/pull/964))
- *(containment)* close the gate's false negatives and correct the feature and profile prose ([#950](https://github.com/getkono/kynos/pull/950))
- *(security)* [**breaking**] refuse a derived http kind without a token scheme ([#947](https://github.com/getkono/kynos/pull/947))
- *(security)* [**breaking**] challenge a derived http-scheme 401 with its scheme token ([#942](https://github.com/getkono/kynos/pull/942))
- *(compression)* read the response length without locking the body ([#936](https://github.com/getkono/kynos/pull/936))
- build on the declared MSRV without let-chains ([#937](https://github.com/getkono/kynos/pull/937))
- *(extract)* [**breaking**] answer 415 for an optional body's content without a Content-Type ([#921](https://github.com/getkono/kynos/pull/921))
- *(security)* [**breaking**] challenge a bearer scope refusal with insufficient_scope ([#926](https://github.com/getkono/kynos/pull/926))
- *(range)* describe the Range parameter by the sender grammar ([#904](https://github.com/getkono/kynos/pull/904))
- *(language)* drop the offer marker bound from AcceptLanguage and Localized ([#899](https://github.com/getkono/kynos/pull/899))
- *(stream)* [**breaking**] ungate BinaryStream and describe it as a Binary body ([#898](https://github.com/getkono/kynos/pull/898))
- *(extract)* drop the unused Schema bound from Describe for Path<T> ([#900](https://github.com/getkono/kynos/pull/900))
- *(router)* [**breaking**] refuse two tag types describing one name differently ([#896](https://github.com/getkono/kynos/pull/896))
- *(router)* [**breaking**] refuse a policy set on a nested or merged router ([#895](https://github.com/getkono/kynos/pull/895))
- *(extract)* [**breaking**] refuse a non-JSON QueryString marker at compile time ([#890](https://github.com/getkono/kynos/pull/890))
- *(trace)* [**breaking**] carry the matched operation's method on the closing event ([#889](https://github.com/getkono/kynos/pull/889))
- *(security)* [**breaking**] refuse an Authorization header field in api_key in every build ([#888](https://github.com/getkono/kynos/pull/888))
- *(server)* [**breaking**] gate ServerError::MutualTlsConflict on the tls feature ([#882](https://github.com/getkono/kynos/pull/882))
- *(macros)* name an identifier enum's variants by the side serde reads ([#870](https://github.com/getkono/kynos/pull/870))
- *(assets)* [**breaking**] refuse a Cache-Control that is no field value rather than drop it ([#867](https://github.com/getkono/kynos/pull/867))
- *(macros)* [**breaking**] refuse unknown, qualified and repeated keys in security, problem and schema ([#866](https://github.com/getkono/kynos/pull/866))
- *(response)* [**breaking**] refuse an unencodable ETag or cookie rather than drop it ([#863](https://github.com/getkono/kynos/pull/863))
- *(macros)* describe an identifier enum's skipped variants, which serde still reads ([#862](https://github.com/getkono/kynos/pull/862))
- *(extract)* [**breaking**] refuse a response header group that cannot encode ([#857](https://github.com/getkono/kynos/pull/857))
- *(macros)* [**breaking**] refuse device_authorization_url outside the device flow ([#856](https://github.com/getkono/kynos/pull/856))
- *(macros)* [**breaking**] keep a cookie derive from moving the emitted version when openapi32 is enabled ([#854](https://github.com/getkono/kynos/pull/854))
- *(macros)* [**breaking**] refuse a field_identifier enum's catch-all in the Schema derive ([#848](https://github.com/getkono/kynos/pull/848))
- *(security)* [**breaking**] send a realm in the default Basic challenge ([#844](https://github.com/getkono/kynos/pull/844))
- *(macros)* [**breaking**] refuse unknown and repeated keys in every helper-attribute grammar ([#837](https://github.com/getkono/kynos/pull/837))
- *(extract)* parse a multipart part's Content-Disposition name quote-aware ([#838](https://github.com/getkono/kynos/pull/838))
- *(macros)* [**breaking**] refuse a bodied Reply variant at 204, 205 or 304 ([#835](https://github.com/getkono/kynos/pull/835))
- *(extract)* name an unreadable Content-Type in the 415 detail ([#833](https://github.com/getkono/kynos/pull/833))
- *(macros)* [**breaking**] make MultipartForm's decoder agree with the schema it publishes ([#826](https://github.com/getkono/kynos/pull/826))
- *(extract)* refuse a request carrying two Content-Type field lines ([#831](https://github.com/getkono/kynos/pull/831))
- *(rate-limit)* [**breaking**] remove Quota::unit, which advertised bytes Quotas never counted ([#830](https://github.com/getkono/kynos/pull/830))
- *(server)* bound an HTTP/2 connection that opens no stream ([#827](https://github.com/getkono/kynos/pull/827))
- *(compression)* keep every zstd level inside RFC 9659's 8 MB window ([#825](https://github.com/getkono/kynos/pull/825))
- *(cookie)* round a sub-second max_age up instead of sending Max-Age=0 ([#824](https://github.com/getkono/kynos/pull/824))
- *(extract)* [**breaking**] refuse a OneOf whose Binary marker shares its partner's type and subtype ([#816](https://github.com/getkono/kynos/pull/816))
- *(rate-limit)* report the refusing quota's ceiling in a Legacy 429 ([#814](https://github.com/getkono/kynos/pull/814))
- *(tls)* name the offending SNI name in TlsError::ServerName ([#813](https://github.com/getkono/kynos/pull/813))
- *(macros)* refuse a tagged enum with no described variant ([#810](https://github.com/getkono/kynos/pull/810))
- *(macros)* describe an undocumented Reply status by its class, not as a success ([#806](https://github.com/getkono/kynos/pull/806))
- *(server)* serve connections with upgrades so upgrade_unchecked can complete ([#823](https://github.com/getkono/kynos/pull/823))
- *(assets)* resolve a relative Directory root once, as documented ([#821](https://github.com/getkono/kynos/pull/821))
- *(compression)* encode a Required response when no coding is named ([#819](https://github.com/getkono/kynos/pull/819))
- *(examples)* correct the unsafe or non-conforming patterns four examples teach ([#818](https://github.com/getkono/kynos/pull/818))
- *(http)* never format an HTTP-date past year 9999 ([#815](https://github.com/getkono/kynos/pull/815))
- *(macros)* [**breaking**] refuse a #[kynos::operation] method the router cannot mount ([#809](https://github.com/getkono/kynos/pull/809))
- *(http)* read a Cookie pair with no '=' as a nameless cookie ([#808](https://github.com/getkono/kynos/pull/808))
- *(examples)* make the testing example's coverage assertion hold ([#805](https://github.com/getkono/kynos/pull/805))
- *(assets)* answer 404, not the root index, for a capture that is not UTF-8 ([#804](https://github.com/getkono/kynos/pull/804))
- *(limits)* report Timeout's sub-second limit in the 408 detail ([#803](https://github.com/getkono/kynos/pull/803))
- *(openapi)* report violation and blocker locations as escaped JSON Pointers ([#802](https://github.com/getkono/kynos/pull/802))
- *(router)* refuse a credentialed wildcard Cors mounted on one endpoint ([#801](https://github.com/getkono/kynos/pull/801))
- *(response)* [**breaking**] make a Location a field value by construction ([#799](https://github.com/getkono/kynos/pull/799))
- *(extract)* [**breaking**] hold a Protobuf body to the bounds its schema publishes ([#795](https://github.com/getkono/kynos/pull/795))
- *(cache)* honour Expires and read invalid freshness as stale ([#793](https://github.com/getkono/kynos/pull/793))
- *(compression)* read every Accept-Encoding field line ([#792](https://github.com/getkono/kynos/pull/792))
- *(tls)* [**breaking**] refuse an IP literal as an SNI certificate name ([#791](https://github.com/getkono/kynos/pull/791))
- *(rate-limit)* take the longest wait when a store failure and a refusal both deny ([#790](https://github.com/getkono/kynos/pull/790))
- *(response)* [**breaking**] refuse a Negotiated tuple that offers one media type twice ([#794](https://github.com/getkono/kynos/pull/794))
- *(range)* cut a ByteSource read past its span to the span ([#797](https://github.com/getkono/kynos/pull/797))
- *(range)* declare the Content-Disposition and Cache-Control a delivery sends ([#796](https://github.com/getkono/kynos/pull/796))
- *(limits)* round Concurrency's Retry-After up instead of truncating ([#788](https://github.com/getkono/kynos/pull/788))
- *(test)* attribute a response to the template the router actually matched ([#789](https://github.com/getkono/kynos/pull/789))
- *(extract)* [**breaking**] refuse a nested OneOf that repeats a media type at compile time ([#662](https://github.com/getkono/kynos/pull/662))
- *(decompression)* measure max_ratio against what arrived across stacked codings ([#660](https://github.com/getkono/kynos/pull/660))
- *(extract)* [**breaking**] refuse a Binary marker offered its OneOf partner's requests ([#656](https://github.com/getkono/kynos/pull/656))
- *(negotiate)* ignore empty elements in Accept ([#661](https://github.com/getkono/kynos/pull/661))
- *(assets)* read every If-None-Match field line in both asset modes ([#658](https://github.com/getkono/kynos/pull/658))
- *(multipart)* escape rejection keys as RFC 6901 pointers ([#657](https://github.com/getkono/kynos/pull/657))
- *(rate-limit)* spend no quota on a request any quota refuses ([#633](https://github.com/getkono/kynos/pull/633))
- *(schema)* refuse NaN under minimum, maximum and exclusive bounds ([#632](https://github.com/getkono/kynos/pull/632))
- *(negotiate)* match an offer whose media type carries parameters ([#630](https://github.com/getkono/kynos/pull/630))
- *(decompression)* decode every gzip member and zstd frame, refuse trailing input ([#628](https://github.com/getkono/kynos/pull/628))
- *(range)* read every If-None-Match field line, and ignore a repeated If-Modified-Since ([#653](https://github.com/getkono/kynos/pull/653))
- *(multipart)* accept a Content-Type with whitespace around its parameter delimiter ([#649](https://github.com/getkono/kynos/pull/649))
- *(router)* refuse an unchecked route that duplicates a declared operation ([#647](https://github.com/getkono/kynos/pull/647))
- *(params)* [**breaking**] refuse a path value that cannot address its route in relative_uri ([#646](https://github.com/getkono/kynos/pull/646))
- *(security)* accept a credential that is only an auth-scheme ([#645](https://github.com/getkono/kynos/pull/645))
- *(middleware)* describe the 400 an interceptor's Reads rejection answers with ([#642](https://github.com/getkono/kynos/pull/642))
- *(router)* hand endpoint-scoped interceptors the mounted paths key ([#641](https://github.com/getkono/kynos/pull/641))
- *(range)* answer If-None-Match: * with 304 when Served has no entity tag ([#640](https://github.com/getkono/kynos/pull/640))
- *(server)* let one server serve_io many connections from a shared service ([#639](https://github.com/getkono/kynos/pull/639))
- *(cache)* compare Vary-selected request fields across every field line ([#636](https://github.com/getkono/kynos/pull/636))
- *(extract)* refuse a body whose content coding nothing decoded ([#643](https://github.com/getkono/kynos/pull/643))
- *(sse)* end a field value at a bare CR ([#638](https://github.com/getkono/kynos/pull/638))
- *(response)* keep a failed body's status under Created and Accepted ([#634](https://github.com/getkono/kynos/pull/634))
- *(router)* run a nested router's interceptors and panic recovery over its unchecked routes ([#629](https://github.com/getkono/kynos/pull/629))
- *(forwarded)* split a forwarding line that is not text on its raw bytes ([#631](https://github.com/getkono/kynos/pull/631))

### Other

- *(openapi)* [**breaking**] gate YAML emission on one pub mod yaml ([#940](https://github.com/getkono/kynos/pull/940))
- *(router)* [**breaking**] move the router's own violations out of SpecError ([#928](https://github.com/getkono/kynos/pull/928))
- *(openapi)* [**breaking**] model xml nodeType as an enum excluding attribute and wrapped ([#927](https://github.com/getkono/kynos/pull/927))
- *(test)* [**breaking**] gate TestClient::query on openapi32 ([#915](https://github.com/getkono/kynos/pull/915))
- *(middleware)* [**breaking**] remove the ContributionConflict variants nothing constructs ([#901](https://github.com/getkono/kynos/pull/901))
- *(assets)* [**breaking**] make the asset endpoint module and type private ([#883](https://github.com/getkono/kynos/pull/883))
- *(middleware)* [**breaking**] drop the no-op EncodeHeaders impl on Preconditions ([#876](https://github.com/getkono/kynos/pull/876))

### Performance

- *(extract)* hand a single-frame request body to the codec without collecting it ([#961](https://github.com/getkono/kynos/pull/961))
- *(router)* wrap the response body for observers in one allocation ([#960](https://github.com/getkono/kynos/pull/960))
- *(unchecked)* stop allocating per unchecked layer per request ([#959](https://github.com/getkono/kynos/pull/959))
- *(cache)* [**breaking**] look a request up without cloning its headers or building an owned key ([#958](https://github.com/getkono/kynos/pull/958))
- *(request-id)* generate and echo an identifier with fewer allocations ([#957](https://github.com/getkono/kynos/pull/957))
- *(stream)* reuse each record's serialization buffer in JSON Lines, JSON-seq and SSE ([#955](https://github.com/getkono/kynos/pull/955))
- *(handler)* [**breaking**] keep the request out of a handler future beside what it extracts ([#954](https://github.com/getkono/kynos/pull/954))
- *(router)* shrink the per-request service and endpoint future boxes ([#952](https://github.com/getkono/kynos/pull/952))
- *(middleware)* [**breaking**] box only each interceptor's own future, without the request ([#943](https://github.com/getkono/kynos/pull/943))
- *(extract)* borrow path captures and decode only declared query values ([#910](https://github.com/getkono/kynos/pull/910))
- *(conditional)* [**breaking**] evaluate If-None-Match over the request's field lines ([#931](https://github.com/getkono/kynos/pull/931))
- *(response)* write derived replies and problems through the stack-buffered JSON writer ([#929](https://github.com/getkono/kynos/pull/929))
- *(multipart)* cut the per-part allocations of a multipart round trip ([#917](https://github.com/getkono/kynos/pull/917))
- *(form)* check a form body's escapes for UTF-8 in place ([#897](https://github.com/getkono/kynos/pull/897))
- *(assets)* stream a served directory's files instead of reading them whole ([#907](https://github.com/getkono/kynos/pull/907))
- *(middleware)* merge Vary in place with at most one allocation ([#902](https://github.com/getkono/kynos/pull/902))
- *(range)* write an entity tag and Served's headers without per-piece allocations ([#880](https://github.com/getkono/kynos/pull/880))
- *(response)* write a JSON body on the stack and allocate it once ([#878](https://github.com/getkono/kynos/pull/878))

## [0.4.0](https://github.com/getkono/kynos/compare/kynos-v0.3.0...kynos-v0.4.0) - 2026-10-13

### Added

- *(params)* derive ParamValue for a field-less enum from its serde names
- *(macros)* describe a string field's content with content_encoding and content_media_type
- *(schema)* carry contentEncoding and contentMediaType in field constraints
- *(macros)* let an ApiError failure send its own header group
- *(middleware)* [**breaking**] log only the identifier RequestId keeps on Trace's arrival event
- *(middleware)* ship a random request id source
- *(schema)* [**breaking**] enforce a map key's pattern ([#528](https://github.com/getkono/kynos/pull/528))
- *(schema)* [**breaking**] enforce the pattern field constraint ([#526](https://github.com/getkono/kynos/pull/526))
- *(server)* answer a request body that stalls between frames with 408 ([#488](https://github.com/getkono/kynos/pull/488))
- *(assets)* offer a fallible Asset::try_embedded constructor ([#398](https://github.com/getkono/kynos/pull/398))
- *(connection)* let an embedding accept loop supply a TLS identity ([#399](https://github.com/getkono/kynos/pull/399))
- *(body)* convert Bytes, Vec<u8>, String and &'static str into Body ([#396](https://github.com/getkono/kynos/pull/396))
- *(tls)* share session-ticket keys across replicas ([#315](https://github.com/getkono/kynos/pull/315))
- *(http)* borrow the resolved Forwarded from a request
- *(extract)* extract Forwarded from a request

### Documentation

- *(security)* say a scheme's default scopes are demanded by every guard naming it ([#578](https://github.com/getkono/kynos/pull/578))
- *(examples)* derive a query parameter's ParamValue in the parameters example
- *(examples)* describe the base64 thumbnail with the derive's content keys
- *(examples)* send Retry-After from one failure in the errors example
- *(examples)* mount Random in the tracing example and drop the stale source comments
- *(middleware)* state where Trace's two request_id fields can still differ
- correct claims about other frameworks and about enforcement ([#523](https://github.com/getkono/kynos/pull/523))
- badge every feature-gated item with its feature on docs.rs ([#518](https://github.com/getkono/kynos/pull/518))
- add contributing, code of conduct and getting-started guides ([#500](https://github.com/getkono/kynos/pull/500))
- *(querystring)* name the allOf condition admits_null over-requires
- *(querystring)* name the Option shape admits_null over-requires
- *(server)* name the HTTP/2 window upper bound prepare refuses
- *(server)* tighten Server::prepare's refusal list
- *(observer)* state that on_disconnect follows the body's release
- *(middleware)* name CompatibleWith where prose named its const helpers
- cut history narration to the intent it carries
- *(features)* drop the manifests' copies of the openapi32 rationale
- *(testing)* name the fixed-sentence rung of the Reply description fallback
- *(examples)* cap each payload body at its own operation ([#273](https://github.com/getkono/kynos/pull/273))
- correct documentation claims the code contradicts ([#290](https://github.com/getkono/kynos/pull/290))
- *(tls)* scope the shared session store to one server's listeners
- *(http)* drop the HSTS reader Forwarded never had
- *(router)* say an inserted MatchedPath or Forwarded is now ignored
- *(test)* quote the five allocations a static match now records
- *(test)* cite the test that drives a HEAD through assert_conformance

### Fixed

- *(router)* [**breaking**] keep the interceptor list in EndpointBuilder::catch_panics's result
- *(router)* keep the interceptor list across EndpointBuilder::catch_panics
- *(router)* union the headers of a status two contributors reach
- *(security-headers)* compare an IPv4-mapped TLS peer in its IPv4 form
- *(forwarded)* [**breaking**] read an IPv4-mapped address as its IPv4 host
- *(middleware)* drop the runs of spaces inside the CredentialedWildcardExposure message ([#541](https://github.com/getkono/kynos/pull/541))
- *(schema)* refuse a recursive generic type instead of overflowing the stack ([#516](https://github.com/getkono/kynos/pull/516))
- *(extract)* [**breaking**] cap buffered request bodies at 2 MiB by default ([#520](https://github.com/getkono/kynos/pull/520))
- *(extract)* [**breaking**] enforce declared bounds in JSON Lines, QueryString, map keys and multipart ([#517](https://github.com/getkono/kynos/pull/517))
- *(server)* close an HTTP/2 connection that holds no stream for its idle timeout ([#512](https://github.com/getkono/kynos/pull/512))
- *(http)* read the forwarded scheme from the trusted hop ([#507](https://github.com/getkono/kynos/pull/507))
- *(schema)* [**breaking**] enforce derived field constraints in Json and Form ([#456](https://github.com/getkono/kynos/pull/456))
- *(docs)* pin the reference bundles with SRI and serve them under a CSP ([#487](https://github.com/getkono/kynos/pull/487))
- *(cache)* keep a guarded operation's response to the caller it answered ([#514](https://github.com/getkono/kynos/pull/514))
- *(cache)* refuse a wildcard on any Vary line, not only the first ([#508](https://github.com/getkono/kynos/pull/508))
- *(server)* keep accepting through file-descriptor exhaustion ([#448](https://github.com/getkono/kynos/pull/448))
- *(server)* drain a failed listener's connections before reporting it ([#506](https://github.com/getkono/kynos/pull/506))
- *(docs)* serve the description a built service reports ([#505](https://github.com/getkono/kynos/pull/505))
- *(emit)* refuse a 3.1 emission carrying unrecognised fields ([#491](https://github.com/getkono/kynos/pull/491))
- *(assets)* honour If-Match before If-None-Match and Range ([#499](https://github.com/getkono/kynos/pull/499))
- *(macros)* name both sides of a PathParams template mismatch ([#498](https://github.com/getkono/kynos/pull/498))
- *(schema)* compare a rival component reached mid-descent ([#497](https://github.com/getkono/kynos/pull/497))
- *(security)* challenge for a Bearer token from derived OAuth2 and OIDC schemes ([#493](https://github.com/getkono/kynos/pull/493))
- *(cache)* [**breaking**] key a stored response on the request authority and honour request no-cache ([#492](https://github.com/getkono/kynos/pull/492))
- *(form)* read and write an array field as one pair per item ([#490](https://github.com/getkono/kynos/pull/490))
- *(cors)* refuse a preflight for a method no CORS configuration covers ([#484](https://github.com/getkono/kynos/pull/484))
- *(macros)* validate header and cookie wire names ([#483](https://github.com/getkono/kynos/pull/483))
- *(http)* [**breaking**] read only the forwarding field the trusted proxy writes ([#457](https://github.com/getkono/kynos/pull/457))
- *(security)* [**breaking**] one guard per operation, with schemes combined in its type ([#446](https://github.com/getkono/kynos/pull/446))
- *(middleware)* merge every Vary line in vary_on ([#482](https://github.com/getkono/kynos/pull/482))
- *(csrf)* try the trusted origin list before Sec-Fetch-Site ([#481](https://github.com/getkono/kynos/pull/481))
- *(assets)* refuse hidden segments and links below a served directory ([#471](https://github.com/getkono/kynos/pull/471))
- *(range)* honour If-Match and If-Unmodified-Since before a range ([#453](https://github.com/getkono/kynos/pull/453))
- *(extract)* select a multipart OneOf side by type and subtype ([#449](https://github.com/getkono/kynos/pull/449))
- *(cookie)* describe a derived cookie as style: cookie under openapi32 ([#451](https://github.com/getkono/kynos/pull/451))
- *(server)* close a connection that sends no request head within the header-read timeout ([#445](https://github.com/getkono/kynos/pull/445))
- *(form)* refuse a form pair that is not UTF-8 with a 400 ([#447](https://github.com/getkono/kynos/pull/447))
- *(assets)* derive a directory index only at a segment boundary ([#400](https://github.com/getkono/kynos/pull/400))
- *(body)* release a watched body before reporting its departure
- *(extract)* treat enum, allOf and not as unrecognised in admits_null
- *(extract)* read an absent query string as null in QueryString
- *(security)* [**breaking**] redact secrets from carried credentials' Debug output
- *(server)* [**breaking**] refuse HTTP/2 windows above 2^31-1
- *(sse)* [**breaking**] refuse a zero keep-alive interval
- *(server)* gate peer_address_service on http1 like its only caller
- *(cache)* hand on a body it declines to store instead of emptying it

### Other

- *(operation)* [**breaking**] take owned or borrowed strings in OperationCx setters
- *(rate-limit)* [**breaking**] make the decision structs non_exhaustive with constructors
- *(middleware)* [**breaking**] narrow stack's const helpers to the crate
- *(response)* [**breaking**] narrow short_circuit_mismatch to the crate
- *(limits)* [**breaking**] read BodyTimedOut's limit through an accessor
- *(sse)* [**breaking**] remove the KeepAlive::text alias
- *(server)* [**breaking**] gate HTTP/1 and HTTP/2 tuning on their module lines
- *(http)* [**breaking**] make the coding and date modules private
- *(http)* [**breaking**] narrow the entity-tag helpers to the crate
- *(http)* [**breaking**] remove the unused ResponseParts alias

### Performance

- *(router)* [**breaking**] carry the route in one request extension
- *(router)* await an uninterposed handler in place

## [0.3.0](https://github.com/getkono/kynos/compare/kynos-v0.2.0...kynos-v0.3.0) - 2026-09-27

### Documentation

- *(schema)* state jiff durations as ISO 8601 that RFC 3339 only partly admits
- *(schema)* name the date and duration edges as accepted ParamValue exceptions
- *(schema)* state the non-finite float exception to ParamValue
- *(schema)* name which type carries each excluded parameter format
- *(params)* state the ParamValue bound and drop the #216 notes
- *(macros)* link the QueryParams object-field limit to #216
- *(macros)* state the object-field limit of the QueryParams derive
- *(macros)* say the path parameter check is ordered
- *(macros)* state what the parameter derives actually check
- *(schema)* state what the default QueryParams::parameters leaves out
- *(schema)* state which flattened types a closed object admits

### Fixed

- *(params)* [**breaking**] bound each parameter field by ParamValue
- *(schema)* say a closed object refuses a document a flattened type writes into
- *(schema)* state ClosedFlatten's remedy without chaining two "or"s
- *(schema)* say only what holds of every type ClosedFlatten refuses
- *(macros)* withhold ClosedFlatten from a struct carrying a serde tag
- *(schema)* [**breaking**] bound a closed object's flattened field by ClosedFlatten

### Other

- *(schema)* [**breaking**] move the flatten markers to kynos::schema::flatten

## [0.2.0](https://github.com/getkono/kynos/compare/kynos-v0.1.0...kynos-v0.2.0) - 2026-09-24

### Added

- *(kynos)* [**breaking**] let each coding refusal name its problem type
- *(kynos)* [**breaking**] let a cross-site refusal name its problem type
- *(kynos)* [**breaking**] let each limit name the problem type its refusal carries
- *(kynos)* narrow a refusal's declared problem type rather than exemplify it
- *(kynos)* let an authorizer name the problem type of its own 403
- *(kynos)* fold a mounted scope's stack onto the one it was mounted beside
- *(kynos)* state the language a response is written in
- *(kynos)* choose a response language from the client's priority list
- *(kynos)* match a language range against an offer by truncation and by prefix
- *(kynos)* read a language tag as the grammar RFC 5646 closes

### Documentation

- *(cost)* say in the recorded baselines that the floor is compared
- *(schema)* say Unchecked is an OpenMap over any payload that is one
- *(schema)* say a flattened Problem constrains the five members it names
- *(macros)* list the deny_unknown_fields refusals beside their snapshots
- *(macros)* list a skipped adjacently tagged payload as refused, with its snapshot
- *(examples)* demonstrate a flattened open map
- *(kynos)* disclose the shift a delta cannot see
- *(kynos)* cite the sections that contain the sentences relied on
- *(kynos)* record where the calibration constant came from
- *(kynos)* say what the pre-region capture actually costs
- *(kynos)* say what naming the shapes centralises rather than removes
- *(kynos)* name all three items the shared counting module supplies
- *(kynos)* say what the shared counting driver asks of its caller
- *(testing)* name the harness the second counting target now shares
- *(kynos)* say what every refusal now names and where the rule lives
- *(kynos)* correct four claims the union's own change falsified
- *(kynos)* [**breaking**] say what the named 403 reaches, and declare the break
- *(kynos)* say both authenticator methods may name a 403
- *(kynos)* record the one thing a rejection carries that the request did not
- *(kynos)* show an authorizer naming its own 403
- *(router)* correct add_tag's claim about where a duplicated tag survives
- *(composition)* let the attribute tag document itself
- *(nfr)* keep the timed half of every performance row it refiled
- *(errors)* localize a problem detail from the application's own catalogue
- *(examples)* serve one resource in three languages
- *(nfr)* characterize the offer a catalogue on disk cannot state

### Fixed

- *(schema)* let a map of Unchecked values sit open beside a field serde never reads
- *(macros)* bound an open field beside a field serde never reads by AdmitsAny
- *(schema)* name the Unchecked map form in the Flatten and OpenMap refusals
- *(schema)* let an Unchecked map payload be flattened as an open map
- *(schema)* let a Problem be flattened beside its extension members
- *(macros)* describe a variant name two variants share under the first alone
- *(macros)* describe every name serde reads a field under
- *(macros)* close each externally tagged object branch to its variant key
- *(macros)* describe a PhantomData member as the null serde writes
- *(macros)* honour a container default on a tuple struct
- *(macros)* say the transparent refusal compares fields, not schemas
- *(macros)* refuse a transparent struct only where serde's two picks differ
- *(macros)* refuse a transparent struct by the field serde picks each way
- *(macros)* refuse a transparent struct serde may read and write through different fields
- *(macros)* refuse a container serde reads or writes through another type
- *(macros)* name the working remedy for a flattened map and struct alike
- *(schema)* name the route a refused tagged newtype payload has
- *(macros)* [**breaking**] hold a tagged newtype variant's payload to Flatten
- *(macros)* allow #[schema(open)] only over a map
- *(macros)* flatten only what names its own members
- *(kynos)* union the problem responses an extractor and a handler share
- *(kynos)* hold a 403's problem type to a static string
- *(router)* apply a route attribute's tag to its operation and register its metadata
- *(kynos)* count allocations on the measuring thread only
- *(kynos)* keep the runtime out of the region that counts allocations
- *(kynos)* keep the mounted scopes across catch_panics
- *(kynos)* [**breaking**] check a later interceptor against the scopes already mounted
- *(kynos)* [**breaking**] keep the interceptor stack across catch_panics
- *(kynos)* check an encoded field against the names its group declares
- *(kynos)* [**breaking**] attach a header group to a continued response exactly once
- *(openapi)* compare a header parameter's name the way HTTP compares one
- *(kynos)* make the constructor case fail for the reason it names
- *(kynos)* rank an offer by the priority the client wrote, not the one it holds
- *(kynos)* repair the four doc links the module split moved

### Other

- *(kynos)* [**breaking**] promote the refusal marker to error::problem
- *(kynos)* [**breaking**] split the language module along the two concerns it grew

## [0.1.0](https://github.com/getkono/kynos/releases/tag/kynos-v0.1.0) - 2026-08-29

### Added

- *(examples)* demonstrate the two interceptors nothing showed
- *(openapi)* convert between a described method and an HTTP one
- *(openapi)* check an emitted document against the OAI's own schema
- *(openapi)* [**breaking**] keep `openapi32` additive for a downstream `match`
- *(middleware)* decode a request body the client compressed
- *(middleware)* report the response the client never received
- *(compression)* [**breaking**] encode a streamed body, and let a handler overrule the negotiation
- *(compression)* choose each algorithm's level in its own units
- *(compression)* [**breaking**] honour a refusal with 406, and clamp what a qvalue may be
- *(middleware)* refuse a cross-site unsafe request without a token
- *(http)* resolve the client address through a stated trust policy
- *(kynos)* demonstrate a multi-part download in the streaming example
- *(kynos)* answer a multi-range request with every part it asked for
- *(kynos)* read only the part of a served file a range asked for
- *(kynos)* serve a byte range of an embedded file
- *(kynos)* demonstrate a resumable download in the responses example
- *(kynos)* answer a byte range request with the part it asked for
- *(kynos)* ingest streamed JSON in the streaming example
- *(kynos)* let a streamed JSON body be an alternative to every other codec
- *(kynos)* read a sequential JSON request body one record at a time
- *(kynos)* demonstrate a described download in the responses example
- *(kynos)* declare a content disposition as a response header group
- *(kynos)* encode a header parameter as RFC 8187 requires
- *(kynos)* cache a response the operation already declares
- *(kynos)* serve a directory, and record it where a generator cannot act
- *(kynos)* compile a directory into the binary as described operations
- *(middleware)* set cookies on a response, and drop the crate that did not
- *(rate-limit)* ship the algorithm, and leave the store to the application
- *(rate-limit)* [**breaking**] report every quota a policy enforced, in either spelling
- *(limits)* shed after a bounded wait, and let a deployment name the delay
- *(extract)* [**breaking**] write a header group through one function on both paths
- *(kynos)* demonstrate a JWT verifier the framework does not ship
- *(kynos)* [**breaking**] let a scheme say where its credential is carried
- *(kynos)* compare a shared secret without returning on the first difference
- *(kynos)* demonstrate a predicate allow-list in the CORS example
- *(kynos)* let CORS expose every response header
- *(kynos)* let a CORS allow-list be decided by a predicate
- *(kynos)* demonstrate the rate limit alongside the other interceptors
- *(kynos)* let a rate limit declare the headers it sets
- *(kynos)* demonstrate cross-origin sharing end to end
- *(kynos)* register a preflight on every path CORS covers
- *(kynos)* let a CORS configuration answer a preflight
- *(kynos)* let the router read an interceptor's value
- *(kynos)* let CORS vary on the origin it answered
- *(kynos)* let a header group declare what a response varies on
- *(kynos)* check a response against the description that declared it
- *(kynos)* [**breaking**] let a multipart form travel in both directions
- *(macros)* expand the Reply derive into a response
- *(macros)* expand the ApiError derive into its problem
- *(unchecked)* mark every operation the escape hatches reach
- *(router)* build one endpoint from its handler
- *(router)* route a request to the operation that answers it
- *(router)* describe one operation as it is mounted
- *(security)* describe each scheme and check its credential
- *(middleware)* refuse a request past the configured rate
- *(middleware)* record each request as it is served
- *(middleware)* give every request an id
- *(middleware)* compress a response the client accepts
- *(middleware)* mark a cross-origin response with what it permits
- *(middleware)* enforce the timeout and the concurrency cap
- *(middleware)* let an interceptor chain run and describe itself
- *(response)* stream a body as it is produced
- *(response)* offer every representation at one status
- *(response)* let a status and a header wrapper set what they name
- *(response)* write every codec's representation
- *(extract)* decode every body an operation can declare
- *(extract)* answer from what the connection already knows
- *(extract)* read a cookie parameter from the request
- *(extract)* read a header parameter and describe it
- *(extract)* read a query parameter from the request target
- *(extract)* read a path parameter from what the route captured
- *(kynos)* give every rejection its problem document
- *(kynos)* let a problem detail reach the wire
- *(schema)* let a constraint narrow the schema it applies to
- *(schema)* let a registry name each type once
- *(kynos)* give the Reply derive a reply to expand into
- *(kynos)* let a captured value be decoded where it is read
- *(kynos)* let a body be produced from a stream
- *(kynos)* [**breaking**] let conflicting interceptors fail to compile
- *(kynos)* let a short circuit name the statuses it answers with
- *(kynos)* let a limit name the response it answers with
- *(kynos)* give the protocol configs builders
- *(kynos)* let a tls failure convert in one hop
- *(kynos)* let a yaml emission failure become a build failure
- *(kynos)* [**breaking**] give a location its own type
- *(macros)* parse and validate the reply attribute
- *(macros)* [**breaking**] reject a format annotation on a field
- *(kynos)* describe an arbitrary-precision decimal
- *(kynos)* describe a fixed-precision decimal
- *(kynos)* describe jiff's dates, times and spans
- *(kynos)* describe chrono's dates and times
- *(kynos)* describe a uuid
- *(kynos)* [**breaking**] make an async fn mountable
- *(macros)* derive one Provides implementation per context field
- *(kynos)* tell users what to do when a bound is not satisfied
- *(macros)* expand every derive to a placeholder implementation
- *(kynos)* implement Schema for the standard library
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

### Documentation

- correct what a v0.1.0 reader would be misled by
- correct six claims the code does not support
- *(examples)* name the status the timeout now contributes
- *(examples)* wire OpenTelemetry over the seams rather than into the crate
- *(limits)* name the constraint that keeps BodySize buffering
- *(kynos)* stop claiming a cache refusal reaches tracing
- *(kynos)* say that tracing emits two events rather than a span
- *(kynos)* link the ordering rule from the interceptors it binds
- *(kynos)* say which intercept call is the outer one
- *(kynos)* import the streamed JSON body from the module that defines it
- *(kynos)* say where a multipart body does copy its octets
- *(kynos)* drop the doc comment the framing enum left behind
- *(kynos)* show a file as a resumable download in the assets example
- *(kynos)* say what the framing enum is before saying why it is a field
- *(nfr)* record what this work now proves, and admit the fifth allowance row
- *(nfr)* say which request limits actually apply without being mounted
- *(state)* settle how the context survives a per-core design
- *(kynos)* stop saying an extractor body is still todo in the test headers
- *(kynos)* stop saying a body is still todo
- *(kynos)* let the testing example drive the service it built
- *(kynos)* let the testing example prove its own assertions
- *(kynos)* give the structural examples real handler bodies
- *(kynos)* give the type-describing examples real handler bodies
- *(kynos)* give the response examples real handler bodies
- *(kynos)* give the request examples real handler bodies
- *(kynos)* let the document example state its metadata at once
- *(kynos)* say which version a description targets by default
- *(kynos)* stop two examples teaching the same interceptor
- *(kynos)* configure both protocols where alpn picks one
- *(kynos)* let the security example verify something
- *(kynos)* keep every security scheme in one file
- *(kynos)* let the middleware example show writing one
- *(kynos)* index the examples
- *(kynos)* let the error example name every failure path
- *(kynos)* show an interceptor reading both bodies
- *(kynos)* show a socket surviving a rebuild
- *(kynos)* show a request span reaching a subscriber
- *(kynos)* give server-sent events their own example
- *(kynos)* let graceful shutdown show every trigger
- *(kynos)* demonstrate serving over tls
- *(kynos)* demonstrate driving a service in-process
- *(kynos)* demonstrate the escape hatches
- *(kynos)* demonstrate the interceptors kynos ships
- *(kynos)* demonstrate every kind of security scheme
- *(kynos)* demonstrate the emitted document
- *(kynos)* demonstrate the streamed response types
- *(kynos)* demonstrate a protobuf body
- *(kynos)* demonstrate assembling a router from parts
- *(kynos)* demonstrate response content negotiation
- *(kynos)* demonstrate everything a handler can return
- *(kynos)* demonstrate every request body codec
- *(kynos)* demonstrate every request-head input
- *(kynos)* demonstrate how a type is described
- *(kynos)* demonstrate every scalar format at once
- *(schema)* rewrite every rejection remedy
- *(kynos)* demonstrate error handling end to end
- *(kynos)* demonstrate each group of the frozen surface
- clarify tokio-only runtime policy
- add architecture, middleware, and NFR design documents
- *(server)* add graceful shutdown example
- document HTTP/3 as demand-driven roadmap work
- add a minimal HTTP/2 server example
- document the anti-patterns and the feature flags

### Fixed

- *(kynos)* keep the tests the archive cannot run out of the archive
- *(router)* restore the feature gates the two splits displaced
- *(kynos)* keep the TLS fixtures out of the published archive
- *(openapi)* [**breaking**] let every extensible object carry its extensions
- *(kynos)* [**breaking**] stop renaming the Schema trait in every user's diagnostics
- *(middleware)* [**breaking**] make a concurrency limit of zero unrepresentable
- *(openapi)* [**breaking**] put a response's description where the requirement is true
- *(kynos)* build the tests a feature owns, at that feature
- *(compression)* state the length the encoded body actually has
- *(compression)* never encode beneath a strong validator
- *(csrf)* read the request's authority from the target as well as Host
- *(http)* believe a scheme only from a sender the policy names
- *(cache)* invalidate the target a non-error unsafe response changed
- *(conditional)* mint a 304 only from a 200
- *(cors)* name authorization beside the wildcard, and stop overstating a preflight
- *(cookie)* honour the name prefixes, the size limit and the domain grammar
- *(trace)* redact credential headers and read the correlation name from its group
- *(kynos)* gate the unchecked-route seam to the feature that calls it
- *(kynos)* mount the timeout outside the body limit it is meant to bound
- *(kynos)* mount a conditional outside the cache whose hit it answers
- *(kynos)* stop compression from encoding a resource that ranges
- *(kynos)* rewrite a percent sign a filename cannot say unambiguously
- *(kynos)* send multipart parts in the order the range-spec appeared
- *(kynos)* compare an asset's entity tag the way the grammar reads one
- *(kynos)* declare a 416 where a handler can actually produce one
- *(kynos)* leave a 206 or a 416 uncompressed
- *(router)* hand an unchecked route what the matcher captured, and a name
- *(server)* forward the configured HTTP/1 header cap unconditionally
- *(kynos)* send the charset RFC 7617 asks a basic challenge to name
- *(macros)* emit the oauth2 flows the attribute declared
- *(kynos)* give a request the connection it arrived on
- *(kynos)* [**breaking**] let a request-id header group say what carrying an id means
- *(kynos)* answer a preflight from the CORS covering the proposed method
- *(kynos)* write a multipart name its own reader reads back
- *(router)* declare an interceptor's header where a consumer resolves it
- *(kynos)* send the keep-alive an event stream was configured with
- *(kynos)* reject a wildcard origin alongside credentials where the docs say it is
- *(kynos)* union Vary rather than overwriting it
- *(kynos)* let an invalid router say what is invalid
- *(kynos)* let text and raw bytes be alternatives
- *(kynos)* let a multipart part be described
- *(kynos)* let an unchecked payload reach the wire
- *(macros)* stop a route's tag being parsed and discarded
- *(kynos)* route a contribution conflict into the build error
- run the compile-fail suite in CI, and stop three signatures freezing wrong
- *(macros)* let the generic operation attribute keep its own argument
- *(kynos)* correct the integer formats and the null union
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

- *(kynos)* [**breaking**] give every item exactly one canonical path
- *(kynos)* [**breaking**] split the parameter groups' two directions into traits
- *(kynos)* [**breaking**] settle four inconsistencies in the mounting surface
- *(kynos)* [**breaking**] seal three enums and mark thirteen constructors
- *(kynos)* [**breaking**] reserve room in the response types a specification leaves open
- *(kynos)* [**breaking**] let a rate limit hold only the count it reports
- *(kynos)* [**breaking**] let a rate-limit decision report when its window resets
- *(kynos)* [**breaking**] close the CORS documentation states
- *(kynos)* [**breaking**] let a 401 carry the challenge its scheme names
- *(kynos)* [**breaking**] let the body limit be the only source of a 413
- *(kynos)* [**breaking**] let a representation be produced only when chosen
- *(kynos)* [**breaking**] let an interceptor declare only what it does
- *(kynos)* [**breaking**] let cors decide in the type whether it documents
- *(kynos)* [**breaking**] let a request id name its header in the type
- *(openapi)* [**breaking**] let a license name one link
- *(kynos)* [**breaking**] keep a rustls failure in the cause chain
- *(kynos)* [**breaking**] make a serialization failure name its emitter
- *(kynos)* [**breaking**] stop a bare io failure becoming a build failure
- *(macros)* [**breaking**] name the typed uri for what it renders
- *(macros)* [**breaking**] name each parameter derive after its trait
- *(kynos)* [**breaking**] give the negotiation traits a canonical path
- *(kynos)* [**breaking**] let a media-typed body be unwrapped
- *(kynos)* [**breaking**] map each integer to its registered format
- *(kynos)* [**breaking**] stop a problem detail being a handler's return type
- *(kynos)* [**breaking**] give each extractor its own rejection
- *(kynos)* [**breaking**] make the escape hatches usable and the schemes describable
- *(kynos)* [**breaking**] give middleware the operation it is covering
- *(kynos)* [**breaking**] make the context a type rather than a container

### Performance

- *(kynos)* compare entity tags without allocating a list to hold them
