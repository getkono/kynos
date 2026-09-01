# Middleware

## The rule

A layer's type must declare everything it can do to a response that the
handler's type does not already say.

Everything below follows from that sentence. The mechanism is the
[`Interceptor`](../crates/kynos/src/middleware/mod.rs) trait's three associated
types, and the reason they are a closed set rather than an open one is that an
interceptor doing something the set cannot express is doing something OpenAPI
cannot describe.

| Associated type | What it declares | What it obliges |
| --- | --- | --- |
| `Short` | the responses it can answer with alone | `Err(Short)` is the only way to answer without the handler |
| `Adds` | the response headers it attaches | `Continued<Adds>` is the return type, so it must attach them — and `with_headers` is on `Continued<()>` alone, so it attaches nothing else |
| `Reads` | the request headers it consumes | it is handed that group, and nothing else |

**One group, once.** `with_headers` writes its group and then relabels the
`Continued`, so while it was callable on its own result a second call relabelled
back to the declared type with the first group's fields already on the response.
An interceptor declaring `Adds = ()` could therefore write anything at all, and
`Adds::NAMES` — which is what the conflict check compares — never saw it. It is
on `Continued<()>` alone now: `Next::run` yields one, `with_headers` consumes
it, and `Continued<G>` has no second call.

An interceptor wanting two groups declares one group naming both fields, the way
`ContentEncoding` names `content-encoding` beside `content-length`. That is not
a workaround — it is what makes the pair visible to the check.

**And a group writes only what it named.** `EncodeHeaders::encode` returns
whatever pairs it likes, so a hand-written group declaring `x-declared` and
writing `content-encoding` would escape the same way from inside. The single
writer both paths go through asserts the subset in debug builds. A subset, not
an equality: a group legitimately writes fewer fields than it declares —
`ContentEncoding` with no coding chosen, a `Cors` permitting no origin. `VARIES`
is outside it, since a `Vary` name is deliberately not in `NAMES`.

## Soundness, not exactness

The invariant is that the emitted document is a superset of observable
responses:

> emitted spec ⊇ observable responses

It is deliberately not equality. Exactness is unenforceable — a panic, an
unhandled 500, or an upstream proxy can all produce a response no type in the
program predicted — and soundness is the property consumers actually depend on.
A client that handles every documented response and encounters an undocumented
one has been lied to. A client that handles a documented response which never
occurs has merely written dead code.

Stating the weaker invariant is what makes it enforceable, and an enforceable
weak claim is worth more than an unenforceable strong one.

## Why the declaration is the signature

Three properties, each load-bearing:

**It is inert data.** What an interceptor declares is read from its types, so it
is inspectable without running the service — or, for the parts that are `const`,
without running anything at all. If learning what the stack emits required
executing the stack, the guarantee would be gone: a document you can only obtain
by running the server is a document you cannot check in CI.

**It cannot disagree with behaviour.** There is no `contribution` method,
because a method returning a description beside a method producing responses is
two statements of one fact. `Short` is both the declaration and the only way to
answer; `Adds` is both the declaration and the return type. An interceptor that
declares a 401 it never sends, or a header it never attaches, does not compile.

**Conflicts are a compile error.** Two interceptors covering one route and both
claiming 429, or both adding `x-request-id`, are rejected by
`Router::intercept`'s bound rather than by a check at build time. The bound
compares declarations — one `Short::STATUSES` against the other, one `Adds`
group's `NAMES` against the other — so a header written on a short-circuit
response is outside it, because it is in no `const`. That gap is named under
[what the framework computes](#what-the-framework-computes-and-what-it-does-not).
What survives in
[`ContributionConflict`](../crates/kynos/src/middleware/contribution.rs) is
the vocabulary for the subtrees where the types are erased and the check cannot
run — those taken under `layer_unchecked`.

*Covering one route* is the whole of it, and it was once read as *mounted on one
scope*, which is narrower. A router's interceptors cover the operations in every
group, nested router and endpoint beneath it, so a `Router::intercept` is
checked against the stacks those scopes brought with them as well as against the
router's own. That is what the fourth type parameter on `Router` and `Group`
carries; [Composition](#composition) has the shape.

What it is *not* checked against is a sibling. Two groups covering different
operations may hold the same interceptor, and refusing that pair would be a
false positive rather than a stricter check — no request reaches both.

**It applies per-operation, after routing.** An interceptor mounted on a subtree
covers the operations in that subtree and nothing else. Scope in the document
matches scope in the router.

## Declaring is not describing

Every header an interceptor sets is *declared*, so the conflict check sees it.
Whether it is *described* is a separate question, answered by
`HeaderParams::DESCRIBED`.

`Vary`, `Content-Encoding` and the CORS set are defined by HTTP itself and
handled by every client without being told, so their groups set `DESCRIBED` to
`false` and stay out of the emitted document. This does not weaken anything: a
second interceptor touching one of those names still fails to compile. The two
questions are "can this collide" and "does a consumer need to hear about it",
and only the first is about correctness.

**Where a described header lands.** An interceptor's `Adds` group is filed
against the *successful* responses the operation already declares, one entry
each — not under a `2XX` wildcard beside them. A consumer resolving a status
takes the exact key first, so a header under `2XX` next to a declared `200` is
one no reader of that operation's 200 will ever find, and the `2XX` entry is
then a response the service cannot produce. An operation declaring no success
at all — a redirect — gets no entry rather than an invented one: the header is
still sent, and understating a description by one header beats claiming a
response that does not exist.

A body is declared nowhere at all — `Continued::take_body` and
`Continued::set_body` need no declaration, because a body has no name to collide
on. Two interceptors rewriting one compose; two setting one header do not. An
encoding a consumer must know about is a header, which is why `Compression`
declares `Content-Encoding` rather than re-encoding silently.

**Negotiation can refuse.** `Compression::Short` is `NotAcceptable`, and 406 is
the answer to a request that refused *every* representation this build can
produce — every coding and identity too. RFC 9110 section 12.4.1 gives two
lawful answers there, honouring the field with a 406 or disregarding it, and
Kynos honours it: disregarding a `q=0` means sending octets the client said in
as many words it cannot decode.

It is answered before the chain runs, because the representation the handler
would produce is one no acceptable coding exists for, so producing it is work
whose result could not be sent.

Reaching it takes `*;q=0`, or naming every coding and identity with `q=0`. No
ordinary client does either, which is why 406 appearing on every covered
operation is a description of something real rather than noise.

Two details of the same section are easy to get wrong and are checked. Identity
is excluded by `identity;q=0` **or** by `*;q=0` with no more specific identity
entry — checking only the first is a live bug against the second. And an *empty*
field value excludes nothing: it "implies that the user agent does not want any
content coding in response", which is identity, not nothing.

**The length is restated with the body.** RFC 9110 section 8.6 counts the octets
actually transferred, and section 8.4 defines the representation "in terms of the
coded form" — so a `Content-Length` written before encoding names a body that no
longer exists. The same section is blunt about it: "a sender MUST NOT forward a
message with a Content-Length header field value that is known to be incorrect."

`content-length` is therefore in `ContentEncoding::NAMES`, and the group states
the encoded length whenever it states a coding. Restated rather than removed:
removing it would leave hyper to derive one from the body's size hint, which is
right only while the body is buffered, and stating it is right whatever the body
becomes.

Reaching the defect took a handler that set its own length, because hyper
derives one when the field is absent and honours it when it is present. That is
also why it went unnoticed.

**A strongly tagged response is left alone.** RFC 9110 section 8.8.1 says it in
as many words: "if the origin server sends the same validator for a
representation with a gzip content coding applied as it does for a
representation with no content coding, then that validator is weak". Encoding
beneath a strong tag therefore makes the tag name two representations, which is
the one thing section 8.8.1 forbids.

A **weak** validator is left alone in the other sense — it still compresses.
Weak is *defined* as shareable across representations, so a response that
already says `W/` is telling the truth after encoding. That is also the way out
for a service that wants both: send `W/"..."`, which is the right validator for
cache revalidation anyway, since `If-None-Match` takes the weak comparison.

Why the encoder does not simply re-tag per coding — `"rev-42-gzip"` beside
`"rev-42"` — which would be sound and would keep strong validators: the only
sanctioned way to write a response header is the `Adds` group, and declaring
`etag` there would make `Compression` and `Cache::deriving_etags` a compile
error on a stack that is otherwise correct. Re-tagging is what
[#30](https://github.com/getkono/kynos/issues/30) needs before a ranged
representation can be encoded at all, and it wants the validator minted where
the range and the coding are both known rather than bolted on at the encoder.

**Compression levels are per algorithm, and one of the defaults departs.**
gzip 6, brotli 4 and zstd 3. The three formats number their levels differently
and put the knee of the curve in a different place, so `GzipLevel`,
`BrotliLevel` and `ZstdLevel` are separate types that do not convert into one
another — a shared `Fastest`/`Best` scale would hide the fact being chosen.

Brotli's default is not its reference encoder's. That is 11, which is meant for
content compressed once and served a million times; applied to a response
generated per request it encodes at around a megabyte a second. Quality 4 is
what edge networks serve dynamic content at, and it still beats gzip 6 on size
while costing less CPU — which is the whole reason to offer brotli to a client
that has not cached anything.

Levels are set per mount, so scope is how they vary. There is no global setting
with a per-endpoint override: two `Compression`s covering one operation both add
`Content-Encoding`, and `header_names_disjoint` refuses that pair where it is
mounted.

**A body still being produced is encoded as it arrives.** A response whose
length is known is collected and encoded once. One whose length is not — an
event stream, a log tail, an export written as it is read — is encoded frame by
frame rather than skipped. `min_size` does not apply there: it is a statement
about a length nobody has. No `Content-Length` rides on the result, because the
encoded length is not known until after the head has gone and RFC 9110 §8.6
forbids forwarding one known to be incorrect.

`LatencyMode` is the trade that opens, and its default is `Interactive` rather
than the mode that compresses best. A body the server produces incrementally has
a reader consuming it incrementally, and withholding bytes to fill a compression
window does not slow such a response down so much as break it — an idle event
stream can go minutes without the client seeing an event it was sent
immediately. `Throughput` is for a body that is a stream only because it is
large, and it is the one you have to ask for.

**A handler may overrule negotiation for one response.** `Encoding::Disabled`
for a body that reflects a secret back beside attacker-chosen input — RFC 9110
§17.6 describes that attack and is deliberately not normative about it, so it is
a policy the application owns. `Encoding::Required` for one too large to be
worth sending as it is: identity stops being an acceptable answer, so a client
that will take only identity is answered 406 rather than handed the whole
representation. It travels in the response's extensions, so no interceptor has
to declare a header for it, and the one status it can produce is the 406
`Compression` already contributes.

**A response that ranges is left alone.** `Compression` refuses a 206, a 416,
anything carrying a `Content-Range`, and anything carrying `Accept-Ranges`,
whatever the client accepted. RFC 9110 §14.1.2 calculates a byte range *with
respect to the encoded sequence of bytes* when a coding is applied, and Kynos
calculates one over the identity octets — so a resource cannot be both encoded
here and ranged beneath.

The first three are the range already taken. Encoding a 206 after its
`Content-Range` has been written leaves a field describing octets the body no
longer carries, and §14.4 tells the recipient of an invalid `Content-Range` not
to recombine it with a stored representation, which is exactly the silent
corruption a client that does recombine gets. The status is checked and so is
the field, so a partial response arriving from a `layer_unchecked` beneath is
caught the same way.

`Accept-Ranges` is the range still to come, and it is the one that bites. An
asset set mints a strong `ETag` over the file's contents and slices every range
from those same octets. Encode the 200 and that one tag names two
representations — which §8.8.1 forbids in as many words, since a strong
validator must change *whenever a change occurs to the representation data that
would be observable in the content of a 200 response*, and a server whose
representations differ only in metadata "needs to incorporate additional
information in the validator to distinguish those representations". Without it,
§13.1.5's `If-Range` matches on a resume it exists to refuse, §15.3.7.3
licenses the client to combine, and identity octets land on the end of an
encoded prefix. Nothing errors; the file is just wrong.

**An asset set gets the compression back, by storing it.** A directory holding
`app.js.br` beside `app.js` serves whichever the client accepts — and mints a
strong validator per stored form, so section 8.8.1 is satisfied and section
14.1.2's range is calculated over the octets actually sent. `Compression` still
refuses the response, and correctly: it is handed one whose coding and tag were
already decided. That is the whole asymmetry. The encoder cannot mint a
validator, because the only sanctioned way for it to write a response header is
the `Adds` group, and declaring `etag` there makes `Compression` beside
`Cache::deriving_etags` a compile error on a stack that is otherwise correct.
The asset server is downstream of nothing: it decides both at once.

So the cost below is now the cost for a set whose build pipeline writes no
encoded form, and for `Compression` over a handler that mints its own strong
tag.

**The cost is real, and it lands on the content most worth compressing.** A
stylesheet or a JS bundle served by an `AssetSet` under `Compression` ships
uncompressed, because every file the asset server answers advertises ranges.
Kynos takes that over a corruption a client cannot detect, and the encoder is
not where the trade can be reopened: re-deriving the validator over the encoded
octets would be sound, and the range and the `ETag` are both settled by the
handler or the asset server before the interceptor is handed the response.

Two ways round it, both outside the encoder:

* mount `Compression` on a group that does not cover the asset set, so the API
  is encoded and the files stay resumable — router scope is the only reason the
  two meet at all;
* let a reverse proxy or CDN encode the files, which is sound there only
  because it owns the validator it sends as well as the coding.

## Why the rate-limit headers keep a prefix

`RateLimit` emits `X-RateLimit-Limit`, `X-RateLimit-Remaining` and
`X-RateLimit-Reset` by default, and RFC 6648 has deprecated `X-` prefixes for
new headers since 2012. The choice is deliberate.

The unprefixed names belong to `draft-ietf-httpapi-ratelimit-headers`, which has
already *replaced* the old triple with a single structured `RateLimit` field
plus `RateLimit-Policy`. These names are `DESCRIBED`, so they reach generated
clients — which makes a wrong name expensive rather than cosmetic. Emitting the
draft's spelling by default would claim settled ground that is not settled, and
that is the failure this project's architecture notes exist to catch.

### What the `X-` triple does not settle

There is no specification for the prefixed names — that is the whole reason the
draft exists — so nothing defines what `X-RateLimit-Reset` counts. The two
dominant implementations disagree: GitHub sends a Unix timestamp, and most
others send delta-seconds.

Kynos sends **delta-seconds**, matching the draft's `t` so that the two
spellings mean the same thing and a client migrating between them reads the same
number. It is stated here because a generated client cannot tell `30` from an
epoch second by looking, and the field is `DESCRIBED`, so the guess reaches
generated code.

That ambiguity is inherited along with the prefix rather than caused by it, and
it is a second reason the structured spelling is worth taking early.

### The migration, and how to take it early

`RateLimit::standard_fields` is the other spelling: `RateLimit` and
`RateLimit-Policy`, rendered as the RFC 8941 structured-field Lists the draft
defines. It is a type-state rather than a flag, shaped exactly like
`Cors::document_response_headers`, because it changes what every covered
operation declares and what every generated client reads.

**The two are never emitted together.** A response carrying both spellings is
two statements of one fact, which is the objection this document raises against
a `contribution` method. `Legacy` and `Structured` are sealed, and their
`Adds` groups name disjoint fields.

The draft's spelling is not merely newer, it is *more expressive*, and that is
the reason to offer it at all. The `X-` triple has room for one quota. A service
enforcing a per-second burst and a per-day allowance can report only half of
what it enforced, and a client cannot tell which half. `RateLimit-Policy` has a
member per quota:

```text
RateLimit-Policy: "burst";q=15;w=1, "daily";q=10000;w=86400
RateLimit:        "burst";r=13;t=1, "daily";r=9998;t=69158
```

A name that cannot be an `sf-string` drops its member rather than producing a
field a parser will reject: one unnameable policy must not cost the client the
others.

### What the framework computes and what it does not

`Quotas` is the algorithm Kynos ships — a sliding-window counter over named
quotas. What it does *not* ship is a store, for the reason it ships no JWT
verifier: a counter store is a dependency, and prescribing one would mean
prescribing `moka`.

Three properties of that algorithm are decisions rather than details:

- **The window slides.** A fixed window lets a client spend a full quota at the
  end of one and a full quota at the start of the next, which is twice the
  advertised rate. Weighting the previous window by how much of it is still in
  view removes that for one extra read and no request log. GCRA would be
  stricter and needs a portable compare-and-swap no generic cache offers.
- **A refusal spends nothing.** The counter is read before it is incremented, so
  a throttled client that keeps retrying does not push its own window along
  forever and can actually recover.
- **`Retry-After` is solved, not guessed.** It is when the estimate falls below
  the ceiling assuming the client sends nothing more — and rounded *up* to whole
  seconds, because truncating a sub-second wait to zero tells a client to retry
  straight into the refusal it just received. Reporting the window's *length*
  instead would be a delay the service does not require, which is the same
  objection `limits.rs` raises against inventing one for a concurrency cap.

A store that cannot answer **allows** by default. A limiter exists to shed load,
and one that sheds everything when its cache blinks has turned a degradation
into an incident. `StoreFailure::Deny` is the other choice, and it answers with
the 429 the limiter already declares rather than a 503 — a second status would
collide with `Concurrency` on any route carrying both, and `statuses_disjoint`
would refuse to compile it.

One thing worth knowing about the two halves. The 429's headers ride on
`Responses`; a success's ride on `Adds`. They never co-occur on one response,
because a short-circuit never calls `with_headers` — so the conflict check,
which compares `Adds` against `Adds`, is not weakened by the pair.

That comparison does leave one gap, named here rather than left to be found. A
`Short` response's headers are in no `const`, so an interceptor whose `Adds`
names `Retry-After` would overwrite the one `RateLimit`'s 429 wrote, and the two
compile. It is unreachable with anything Kynos ships — `Concurrency` and
`RateLimit` both write `Retry-After` from their own short circuits, and only one
short circuit answers a request — and it stays a gap on purpose. Closing it
needs a `HEADERS` const on `ShortCircuit`, which `#[derive(ApiError)]` could not
derive from an `IntoResponse` body: it would be a declaration written beside the
behaviour rather than being it, which is the `contribution` method this document
refuses.

### Replacing the algorithm, and why no bucket ships

`Quotas` is one implementation of `RateLimitPolicy`, not the trait's only
inhabitant. An application wanting a different algorithm — a token bucket, a
leaky bucket, an allowance bought by the month — implements that trait instead
and keeps everything else: the 429, `Retry-After`, both header spellings, the
description, and the `RateLimitKey` implementations that decide whose bucket a
request counts against.

**Kynos ships no token bucket.** Not because one is hard, but because there is
no single right one, and the differences are exactly the parts a service has to
choose: whether a bucket is per process or per fleet, what happens to a client
whose bucket was evicted, whether an idle client's tokens accrue for a minute or
a day. A shipped bucket answers all three by fiat, and a service wanting
different answers would carry the wrong one in the binary and write its own
anyway. [`examples/token_bucket.rs`](../crates/kynos/examples/token_bucket.rs)
is a production-shaped one — `std` only, injected clock, continuous refill,
bounded memory — written to be read and copied rather than depended on.

**There is no `rate-limit` feature flag either.** Gating this module would gate
`RateLimit`, `Decision` and the two header spellings, which are
description-shaping surface, behind a flag whose off-state buys nothing: the
counters are the only expensive part and they are the application's under every
configuration. A flag that removes no dependency and no cost is a build
configuration to get wrong.

The seam is asserted rather than assumed.
[`tests/rate_limit.rs`](../crates/kynos/tests/rate_limit.rs)'s
`a_policy_kynos_does_not_ship_reaches_the_wire` drives a policy Kynos does not
ship and reads what came back, with `Retry-After` and the `RateLimit` field's
`t=` given deliberately different values — they answer different questions, one
from the `Denial` and one from the `ServiceLimit`, and a wiring that read either
from the other would fail there.

## The order a chain runs in

**The first `intercept` call is the outermost interceptor.** A chain is a slice
run head-first, and each scope's own interceptors come before whatever a group
or a nested router contributed — so a router's wrap a group's, and an endpoint's
are innermost of all. Written top to bottom, a stack reads outside in:

```rust,ignore
Router::<()>::new()
    .mount(kynos::routes![report])
    .intercept(Conditional::new())  // outermost
    .intercept(Cache::new(store))   // inside it
    .intercept(Compression::new()); // closest to the handler
```

Order is not part of the type. `CompatibleWith` checks that two interceptors do
not add one header or answer with one status, and a set has no positions. So
every ordering rule this document states is one a reader has to follow, and the
two places it matters are the slow-body row below and
[where a cache sits](#where-a-cache-sits).

## What bounds a request before an interceptor runs

An interceptor covers the operations in its subtree, which means it runs after
routing — and a request that never reaches routing is bounded by something else
or by nothing. The table is here because "does Kynos have payload limits" has
four different answers depending on which layer is asked.

| Vector | HTTP/1 | HTTP/2 | Server | Interceptor | Bounded by default? |
| --- | --- | --- | --- | --- | --- |
| Request line and URI length | must fit `max_buffer_size` (408 KiB) | `max_header_list_size`, 16 KiB | — | — | yes, loosely |
| Header count | `max_headers`, 100 → 431 | by list size rather than count | — | — | yes |
| Header-list size | `max_buffer_size` | `max_header_list_size` | — | — | yes |
| Query-string length | subsumed by the URI | subsumed by the list size | — | — | yes, loosely |
| Body size | — | — | — | `BodySize`, when mounted | **no, deliberately** |
| Request-head read time | `header_read_timeout`, 30 s | n/a | — | — | yes |
| Slow body | — | — | — | `Timeout`, *outside* `BodySize` | **no** |
| Keep-alive idle | `header_read_timeout` covers the wait for the next head | `Http2KeepAlive`, unset | — | — | HTTP/1 only |
| Handler runtime | — | — | — | `Timeout`, when mounted | **no** |
| Response body stall | — | — | — | `BodyTimeout::idle`, when mounted | **no** |
| Response body total time | — | — | — | `BodyTimeout::deadline`, when mounted | **no** |
| Total connections | — | — | `max_connections`, 10 000 | — | yes |
| Per-IP connections | — | — | — | — | **no**, and see below |
| Per-IP request rate | — | — | — | `RateLimit` keyed `ByClientAddress`, with a trust policy set | no |
| Concurrent in flight | — | `max_concurrent_streams`, 200 per connection | — | `Concurrency`, when mounted | partial |
| Request rate | — | — | — | `RateLimit`, when mounted | no |
| Request smuggling | hyper and `httparse` | n/a | — | — | yes, and not Kynos's |
| Reset flood | — | `max_pending_accept_reset_streams`, `max_local_error_reset_streams` | — | — | yes |
| TLS handshake stall | — | — | `handshake_timeout`, 10 s | — | yes, with `tls` |
| Decompression bomb | — | — | — | `Decompression`, when mounted | **no** |

Five rows are worth reading twice.

**A body cap is not default, and that is a decision.**
[`nfr.md`](nfr.md#extraction) records the three reasons. The shortest is that a
default limit would add 413 to every operation of every application that never
asked for one — and this framework's whole position is that a declared response
is a promise.

**A decompression bomb is `BodySize`'s blind spot, not its job.** Two kilobytes
of zeroes are a gigabyte of gzip output, so a cap measured before decoding
measures the one number an attacker chooses freely. `Decompression` takes the
limit instead and applies it to what the handler will actually see. The two
cannot be mounted together — both answer 413, and `statuses_disjoint` refuses
the pair — which is right rather than awkward: on a route that accepts content
codings, `BodySize` alone is not a weaker guard but a misleading one.

**The slow-body row depends on mounting order.** `BodySize` reads a length-less
body frame by frame, so a client sending one frame slowly holds that loop open.
`Timeout` wraps whatever is beneath it, which means it bounds the read only when
it is mounted *outside* the limit doing the reading — the earlier `intercept`
call, per [the ordering rule](#the-order-a-chain-runs-in). The types do not
enforce it, and neither does a test:
`a_timeout_over_a_body_limit_declares_both_statuses` in
[`tests/limits.rs`](../crates/kynos/tests/limits.rs) mounts the arrangement but
asserts only on the emitted document, which is order-insensitive and passes
either way. Pinning the read needs a client that dribbles a chunked body over a
real socket, which the harness cannot express today. This paragraph is where a
reader learns the rule, and nothing below it is checked.

**A response body is bounded by neither of the rows above.** `Timeout` wraps the
chain's future, and that future completes when the *head* is ready. A handler
returning a stream — Server-Sent Events, JSON Lines, a large body read from
elsewhere — returns immediately and emits for as long as it likes, with its
timer already stopped. `BodyTimeout` is the row that covers it, in two shapes:
`idle` restarts on every frame and bounds the gap between them, `deadline` never
restarts and bounds the total. It declares no status, and cannot — the status
and the headers have already left, so a body that runs out of time is a
truncated response and an error on the stream rather than a status a client can
read.

`BodyTimeout` belongs *outside* anything that rewrites a body. An interceptor
that rewrites one has to read it, and one that buffers — `Compression` below its
size threshold, `Cache` storing a response — reads to the end before writing
anything; handed a body that fails part-way it has no partial response to emit
and falls back to an empty one, so a timeout mounted beneath it reaches the
client as a complete, zero-length success. Outside, the error is the body's last
frame and the driver resets the stream. Like the slow-body rule above, the types
do not enforce this.

An idle limit polls the inner body *before* its clock, which is not an
optimization. The gap the timer measures is between polls rather than between
frames, and a driver stops polling a body for reasons that are nothing to do
with the producer — an HTTP/1 write buffer that is full, an HTTP/2 window that
is closed, a busy executor. Consulting the clock first would end a body that had
a frame ready and report a slow *reader* as a stalled *writer*. A deadline does
consult its clock first, because a body still producing steadily is exactly what
it exists to end.

A body this timer ends is reported to an `Observer` as a disconnect rather than
a delivery, which is the only way it is visible: the status and the headers were
already a success. `a_body_the_timer_ended_is_reported_as_interrupted` in
[`tests/limits.rs`](../crates/kynos/tests/limits.rs) holds that, with a
completing body as its control.

A Server-Sent Events keep-alive is a real frame, so it restarts an `idle` clock
exactly as an event does. An event stream with keep-alive enabled and an
interval shorter than the limit therefore never trips one. That is the intended
reading — the connection is demonstrably alive — but it means `idle` bounds the
transport rather than the application there, and `deadline` is what bounds how
long such a stream may run at all. Both directions are asserted in
[`tests/limits.rs`](../crates/kynos/tests/limits.rs).

**A timeout answers 408, and neither status is exact.** RFC 9110 §15.6.5 scopes
504 to a server "acting as a gateway or proxy" awaiting an upstream — which an
origin wrapping its own chain is not, and which a load balancer in front of the
service genuinely *is*, so an origin's own 504 was indistinguishable from that
hop's. §15.5.9's 408 describes the slow-body row above exactly and the
handler-runtime row only by extension; it is the closest the specification
defines and the one `tower-http` sends. 503 would read better for handler
runtime and is unavailable: `Concurrency` declares it, and `statuses_disjoint`
would then refuse a router bounding handler time *and* capping concurrency,
which is an ordinary pairing.

**A per-IP cap is absent rather than pending.** Behind a load balancer every
connection arrives from one address, so a cap counted in-process is either
meaningless or a self-inflicted outage.

The security policy that half of it needed now exists.
`Router::trusted_proxies` names the hops whose forwarding fields may be
believed, and `ByClientAddress` keys a rate limit on what they resolve to — so a
per-IP *rate* limit is honest behind a proxy where `ByPeerAddress` was silently
a global one. A per-IP *connection* cap is still absent, because a connection is
counted before any header is read and the policy lives after routing.

Unset, nothing is believed and `ByClientAddress` behaves exactly like
`ByPeerAddress`. RFC 7239 section 8.1 is why: the field "cannot be relied upon
to be correct, as it may be modified, whether mistakenly or for malicious
reasons, by every node on the way to the server, including the client making the
request." A limiter reading it unasked would let a client choose the bucket it
counts against — a limit that looks like one and is not, which is worse than
none.

### A response no type predicts

The soundness invariant is *emitted ⊇ observable responses* for the responses
Kynos produces. A **431** from hyper's own header parsing is not one of them: it
is written by the protocol driver before any route matched, so it reaches no
operation and no `Responses` implementation ever saw it. It joins the panic, the
unhandled 500 and the upstream proxy on the list of responses the invariant does
not reach — named here rather than left to be discovered, because a consumer
meeting one is entitled to know Kynos never claimed otherwise.

## Preflight

A CORS preflight is answered by the router, not by a chain.

It is registered as an operation on the matched path while the service is built,
after the description has been assembled. That ordering is the whole design:

- **It contributes no `paths` key.** Not because a filter removes it, but
  because `describe` had already finished when it was created.
- **It appears in no `Allow` header.** The `Allow` loop runs before
  registration, so a 405 still names only the operations the description
  declares.
- **A path that declares its own `OPTIONS` gets no synthesized one.** The
  user's operation wins by construction rather than by a race.
- **It runs no interceptor.** A browser sends a preflight with no credentials
  and no `Authorization`; an auth interceptor short-circuiting it would break
  CORS for every operation on the path. `middleware.md` says an interceptor
  covers the *operations* in its subtree, and a preflight is not one. Observers
  still see it, which is right — a preflight is worth logging.

An `OPTIONS` that is *not* a preflight — no `Origin`, or no
`Access-Control-Request-Method` — is answered exactly as it was before CORS was
mounted: the same `method_not_allowed` policy, the same `Allow` value.

The methods a preflight advertises are the ones the covering scope declares, so
a `Cors` on a group owning `GET /x` advertises `GET` even where the router also
owns `POST /x`. `Cors::allow_methods` overrides that, for a deployment fronting
routes Kynos does not serve.

**A path can be covered by more than one `Cors`.** A group's interceptor stack
is checked against the router's and never against a sibling's, on the premise
that no request reaches two operations — and a preflight is the request that
does, since it is answered once per path. So the answer is assembled per scope:
`Access-Control-Request-Method` picks the configuration whose real response will
honour it, and a proposed method no scope covers falls back to the first, which
refuses it in the advertised list either way.

**Mount `Cors` outermost.** A short-circuiting interceptor mounted *outside* it
answers without the `Access-Control-*` fields, and the browser then reports an
opaque CORS failure in place of the status the service actually sent. A 429 from
`RateLimit`, a 413 from `BodySize`, a 503 from `Concurrency` and a 504 from
`Timeout` are all worth a client being able to read.

The converse composes already: `erased.rs` turns an inner `Err(Short)` into a
response that the outer `Next::run` hands back as a `Continued`, so a `Cors`
outside any of them decorates their refusals. Only the outside-in direction goes
wrong, and like [the cache ordering](#where-a-cache-sits) it is a rule a reader
has to follow rather than one the types keep — `CompatibleWith` compares a set,
and a set has no positions.

**One limit.** An endpoint-scoped `Cors` answers no preflight: an endpoint's own
interceptors stay inside the endpoint, which is what runs them, so preflight
registration cannot see them. Mount CORS at a router or group scope.
`a_cors_mounted_on_one_endpoint_answers_no_preflight` records the behaviour, so
closing the gap turns a test red rather than nothing.

## The one interceptor the router recognises by identity

Everything above is read from an interceptor's *types*. There is exactly one
exception, and it is bounded rather than general: while the router is built, it
downcasts each interceptor to `Cors` and asks the configuration two questions
the type system cannot answer.

Both are about a *value*. `allow_any_origin` and `allow_credentials` are
`mut self -> Self` builders on purpose — an allow-list read from the environment
at startup has to be applied conditionally — so whether they were both called is
not a fact a `const` can see. The two questions are whether the pair was
selected, which is refused (`Error::Middleware`), and what a preflight on the
covered paths should answer.

What stops this becoming a capability:

- The match is a closed list of two concrete types. `CorsDocumentation` is
  sealed, so there cannot be a third, and
  `every_cors_documentation_state_is_one_of_the_two_the_router_recognises`
  fails if one is added without teaching the probe about it.
- A third-party interceptor is never asked. `ErasedInterceptor::as_any` is
  `pub(crate)`, and there is no trait method an outside implementation could
  supply to be read this way.
- **Nothing read here reaches the description.** The refusal stops a document
  being produced at all; the preflight is registered after `describe` has
  finished. So the property this document opens with — that a declaration
  cannot disagree with behaviour, because it is the same text — is untouched.

## Repeatable response fields

`Set-Cookie` is the field HTTP forbids comma-joining, and it is the reason
`HeaderParams` has a `REPEATABLE` const at all.

A property of the *group* rather than a table of field names, because a
per-name allow-list is a table that goes wrong and the group already knows
whether its own fields comma-join. `false` — the default — inserts, which is
right for almost everything: a response carrying two `Content-Encoding` values
is one no client can decode.

Both ways a group reaches the wire go through one writer,
`extract::params::header::write`. They did not, and the comment on the second
claimed they could not disagree while they were two functions that did:
`Continued::with_headers` inserted and `WithHeaders::into_response` appended, so
a group naming `Set-Cookie` twice reached the wire whole from a handler and
truncated from an interceptor. No shipped interceptor named a repeatable field,
so nothing noticed until one did.

**OpenAPI cannot say a field repeats.** `Response.headers` is a map keyed by
field name, so `SetCookies` declares one `Set-Cookie` entry and says the rest in
its description. That is the honest half of what can be said, and understating a
description beats claiming a shape the format has no way to express.

## The attributes a cookie carries

Seven, and the set is closed: `Path`, `Domain`, `Max-Age`, `Secure`,
`HttpOnly`, `SameSite` and `Partitioned`. `Cookie::removal` is the eighth thing
a caller reaches for and is not an attribute — it is a cookie with an empty
value and `Max-Age=0`, carrying whatever scope the original was set with,
because a user agent matches a removal on `Path` and `Domain` and ignores one
that does not.

Three of them are supplied rather than merely accepted, because the alternative
is a cookie the service believes it set and the client silently discarded:
`SameSite=None` implies `Secure`, `__Secure-` implies `Secure`, and `__Host-`
implies both `Secure` and `Path=/`.

**`Expires` is deliberately absent.** RFC 6265bis §4.1.2.1 gives `Max-Age`
precedence wherever both appear, so a cookie carrying both is one attribute
stating the lifetime and one being ignored. `Max-Age` is also a duration, which
is what a server actually knows; `Expires` is an absolute HTTP-date, so
emitting one means trusting the client's clock against the server's and
serializing a date format that `architecture.md`'s dependency table has no row
for — an HTTP-date crate is one of the three dependencies it names as refused.
`Max-Age=0` is the removal, so nothing needs a date in the past either.

That is the whole of what an acceptance contract asking for "Path, HttpOnly,
Secure, SameSite, Max-Age, and expiry attributes" needs: the expiry is
`Max-Age`, stated as a duration.

The set being closed is asserted rather than intended.
[`response/cookie/tests.rs`](../crates/kynos/src/response/cookie/tests.rs)
reads the builders off the source and counts them against the sweep that
renders them, so an eighth attribute that no test exercises fails the build
instead of shipping unasserted.

## What the cookie interceptor is not

`SetCookies` writes cookies. It does not sign them, encrypt them, or keep a
session, and none of the three is a gap to close later.

A cookie carrying a credential is a
[`SecurityScheme`](../crates/kynos/src/security/mod.rs) rather than a parameter
— `extract::params::cookie` has said so since it landed — and signing or
encrypting one is how that credential is protected. That puts it on the
authentication side of the line [`security.md`](security.md) draws, where Kynos
ships the carrier and not the verifier. It would also arrive with a crypto stack
(`hmac`, `sha2`, `aes-gcm`, a source of randomness) that the dependency table
has no row for and that no feature gate could contain, since the jar would be in
the default build.

Sessions are named in [`architecture.md`](architecture.md#invariants)'s third
invariant as the example of what a layer above Kynos owns.

CSRF *was* listed here as the exclusion the type system refuses rather than the
policy, on the grounds that `statuses_disjoint` compares `Short::STATUSES`
across the interceptors covering a route and `Auth<S>` contributes 403 to every
authenticated operation. **That was wrong, and the error is worth naming rather
than quietly deleting.**

`Auth<S>` is not an interceptor. It is an extractor — `FromRequestParts` in
[`security/auth.rs`](../crates/kynos/src/security/auth.rs) — and its 403 reaches
the document through `OperationCx::add_responses`, never through a `const`.
`CompatibleWith` is instantiated only over pairs of `Interceptor::Short`, and no
shipped interceptor declares 403 at all. A CSRF interceptor declaring one
compiles beside a credential guard, and always would have.

The residual is real but much smaller, and it is about *description* rather than
compilation: `Responses::merge_from` keeps the first entry on a key collision,
so a CSRF 403 and an `Auth` 403 on one operation produce one entry with
whichever description landed first. Understating a description by one sentence
is the failure mode this project accepts elsewhere for the same reason.

What made the exclusion look structural was that the crypto objection above is
real for *token-based* CSRF: a synchroniser token needs randomness, an HMAC and
somewhere to keep the token, which is a session. [`Csrf`](../crates/kynos/src/middleware/csrf.rs)
avoids all three by not having a token — `Sec-Fetch-Site` is set by the browser
and script cannot forge it, so an unsafe request that says it came from another
site can be refused on that alone. Four header comparisons, no dependency.

The fallback for a browser too old to send it compares `Origin` against the
request's own authority, and that authority is read from `Host` *or* from the
request target. RFC 9113 §8.3.1 replaces `Host` with the `:authority`
pseudo-header, which `http` puts on the URI rather than in the map, so reading
`Host` alone found no authority on any HTTP/2 request — and refused every
same-origin unsafe request from exactly the browsers the fallback exists for.
`Host` wins where both are present: §8.3.1 requires them to agree, so the
choice is a tie-break rather than a policy.

## Vary is declared apart from the names

`Vary` is the one response header two interceptors may both contribute to, so it
has a channel of its own: `HeaderParams::VARIES` rather than `NAMES`.

RFC 9110 §12.5.5 defines it as an unordered set of field names. `Compression`
varies on `Accept-Encoding` and `Cors` varies on `Origin`, and both belong on the
same response — a browser-facing service wants that pairing. Naming `vary` in
`NAMES` would make it a compile error, and the conflict check is right about
every *other* header, so the fix is to stop calling this one a conflict rather
than to weaken the check.

Kynos merges what is declared into whatever `Vary` the response already carries:
case-insensitively, because a field name is; and never past `*`, which already
says the response depends on more than field names can express. The merge runs
in both places a group reaches the wire — `Continued::with_headers` and
`WithHeaders::into_response` — so the two cannot disagree.

`VARIES` is never described. A shared cache reads `Vary`; a client generator has
no use for it. Getting this wrong is not a missing nicety: a CORS response that
varies on `Origin` without saying so lets a cache hand one origin's
`Access-Control-Allow-Origin` to another, which defeats the check entirely.

## Where a cache sits

Outermost but one. `Conditional` outside `Cache`, and `Cache` outside `Cors`
and `Compression` — outside being the *earlier* `intercept` call, per
[the ordering rule](#the-order-a-chain-runs-in).

The first half is what makes the pair worth having: a hit turned into a 304 has
produced only the *cached* body. The other way round, the handler runs and its
work is thrown away — which `Conditional` costs anyway, and which a cache is
there to avoid. The second half is so that what gets stored is a response whose
negotiated headers have already landed.

**The order is documented, not enforced.** Enforcing it needs a marker threaded
through `CompatibleWith` for one interceptor, which generalizes the `Cors`
downcast into exactly the capability this document bounds elsewhere.

Getting the first half backwards is not cosmetic. `Cache::Short` is `Infallible`
and a hit is served by constructing a `Continued` rather than by calling
`next.run`, so a hit never reaches an interceptor mounted *inside* the cache: a
`Conditional` there answers a miss and nothing else, and the 304 the pairing
exists for is unreachable on exactly the request that should get it.
`a_conditional_over_a_cache_answers_a_hit_with_no_body` in
[`tests/cache.rs`](../crates/kynos/tests/cache.rs) is what holds it, and it
holds only because the route it mounts states a lifetime — over one the store
never keeps, `Conditional` alone answers identically and the test asserts
nothing about the pair.

What *is* enforced is the case where getting it wrong is catastrophic. A
response carrying `access-control-*` headers whose `Vary` does not name `origin`
is refused outright, because storing one hands one origin's
`Access-Control-Allow-Origin` to another and defeats the check entirely.

The other half — a `Cache` *inside* `Compression` — is not refused either, and
this document used to call it merely suboptimal. It is not. The body is stored
and tagged over identity octets and then encoded on the way out, so one strong
validator names two representations, against RFC 9110 §8.8.1. A client can
validate the encoded body with `If-None-Match`, be answered 304 — which replays
`ETag` and `Vary` and not `Content-Encoding` — and reuse those octets as the
identity representation.

The mis-ordering is not what causes it. `Compression` encoding *any* strongly
tagged response had the same defect, with no cache in the stack at all, because
the encoder did not look at `ETag`. That is [#29](https://github.com/getkono/kynos/issues/29),
and it is closed: the encoder now leaves a strongly tagged response alone, so
the stored-and-then-encoded case has nothing left to go wrong in. A weak
validator still compresses, and is the right validator for revalidation anyway.

### Why a hit is not a new response

`Cache::Short` is `Infallible`. A hit replays a status the operation already
declares, so a cache invents nothing and contributes no response of its own.
What it adds is `Age`, and under `deriving_etags` an `ETag`.

That does mean the cache is the one place outside `Next::run` that constructs a
`Continued`. The constructor is `pub(crate)`, so a third-party interceptor still
cannot mint one; and the invariant it protects is intact, because what is
replayed is a response *the operation itself produced* and that passed the
storability rules before it was stored.

`Conditional::Short` is `NotModified`, and 304 *is* a new status, so it is
declared. It is answered **after** the chain runs, because RFC 9110 section 13
evaluates a precondition against the current representation and only the handler
knows what that is — a `Continued` deliberately cannot change a status.

### And only from a 200

RFC 9110 section 15.4.5 defines 304 as the answer to a request that "would have
resulted in a 200 (OK) response if it were not for the fact that the condition
evaluated to false". The status is therefore part of the precondition rather
than a filter for failures, and the guard is an equality against 200 rather
than `is_success()`.

The difference is five statuses — 201, 202, 203, 204 and 206 — and 206 is the
one that bites. A 304 replays `ETag` and `Vary` and never `Content-Range`, so a
client resuming a download would be told its copy is current with no way to
tell whether that meant its *range* or the whole representation.

### What an unsafe method's precondition does not get

`If-None-Match` on a `POST` or `PUT` is read and then dropped: the request
proceeds as though it carried no precondition.

Section 13.2.2 step 3 says an `If-None-Match` that evaluates false answers 304
for `GET` and `HEAD` and **412** for anything else, and 412 is the create-only
idiom — `If-None-Match: *` on a `PUT`, meaning *only if it does not exist yet*.
Under Kynos that precondition is ignored and the write lands, which is the lost
update the mechanism exists to prevent.

It is a gap rather than a decision, and it is a gap for a structural reason:
412 is a status `NotModified` does not declare, and widening `Short` to carry it
would add 412 to every operation the interceptor covers — including every safe
one, which can never produce it. Closing it properly needs a precondition guard
whose declaration varies with the method, which the contribution model states
once per interceptor rather than once per operation. Until then, a service
relying on create-only semantics must enforce them in the handler.

### What is never stored

`no-store` from either side, `no-cache`, `private`, `Vary: *`, any response
setting a cookie, and a credentialed request whose response did not say it was
shareable.

The cookie rule has no opt-out. Replaying a response that mints a session to a
second client is the worst bug a cache has, and `Vary` cannot protect against
it: the cookie is in the *response*, and nothing in the request selects it.

**There is no heuristic freshness.** RFC 9111 section 4.2.2 permits one, and
every heuristic is a guess that turns a correct origin into an incorrect cache.
A response that did not say how long it may be reused is not reused;
`default_freshness` is opt-in and documented as the guess it is.

`stale-while-revalidate` and `stale-if-error` are absent, and not as an
oversight: the revalidation is a background task — a `tokio::spawn` outside
`server/` — which would be a seventh row in the runtime allowance table for a
nicety, and a stale response is one the operation's description has no way to
mark.

### What a write drops

RFC 9111 section 4.4 requires a cache to invalidate the target URI on a
non-error status to an unsafe method, and a non-error status is a 2xx or a 3xx.
So a `POST`, `PUT`, `PATCH`, `DELETE` or an extension method the `http` crate
does not recognise drops what was stored for that target, and a 4xx or 5xx
answer to the same request drops nothing — a refused write changed nothing worth
paying a handler call to rediscover.

Two details are decisions rather than mechanics.

**Both stored methods are dropped, not the one that arrived.** The requirement
invalidates a *URI*; `PrimaryKey` carries the method; and only `GET` and `HEAD`
are ever stored. So the key a `POST` would build names nothing at all, and
dropping that key would satisfy a reading of the rule while leaving in place the
exact copy the client is about to read back.

**An unknown method counts as unsafe.** Section 4.4 says "including methods
whose safety is unknown", and `Method::is_safe` is false for an extension method
the crate does not know — so the two agree without a table of method names to
keep in step.

The `Location` and `Content-Location` invalidations in the same section are a
MAY and are absent. Honouring them means comparing origins, since the section
forbids invalidating across one, and a URI parsed out of a response header is
not something the router can map back to a `paths` key — so the entry to drop
could not be named even after the check passed.

## Opaque

The vocabulary is
[built](../crates/kynos-openapi/src/annotation/mod.rs) and
[checked](../crates/kynos-openapi/src/validate/rules/opaque.rs), and the router
side that produces it is
[in `router/service.rs`](../crates/kynos/src/router/service.rs)'s `mark_opaque`.

`unchecked` and `Opaque` are cause and effect, not alternatives, and the
distinction is worth being precise about:

- **`unchecked` is the waiver.** Author-side, opt-in, feature-gated. It is the
  application owner asserting they know what they are doing, and it stays
  exactly as it is.
- **`Opaque` is the record that waiver leaves on the document.** Framework-side,
  derived, per-operation. Nobody writes `Opaque` by hand.

The invariant: **`Opaque` marks affected operations unverified, and never omits
them.** A document that silently drops an operation is worse than one that flags
it, because the omission is invisible to the consumer that trusts it.

Today the escape hatches in `crates/kynos/src/unchecked.rs` have three different
blast radii for the same underlying situation. The waiver must mark exactly what
it reaches:

| Escape hatch | Record | Where |
| --- | --- | --- |
| [`Unchecked<T>`](../crates/kynos/src/schema/unchecked.rs) | `x-kynos-unchecked` on the schema | The schema only; the operation is not marked, because a hand-written schema is not an undeclared effect |
| `layer_unchecked`, `into_tower_unchecked` | `x-kynos-opaque` on each covered operation | The covered subtree only, never the whole document |
| `route_unchecked` | An entry in `x-kynos-opaque-routes` | The document root; no `paths` key |
| `upgrade_unchecked` | An entry in `x-kynos-opaque-routes` | The document root; no `paths` key |

**A route with no expressible template gets no `paths` key.** This was once
specified as "operation emitted, flagged `Opaque`", which cannot be honoured:
a catch-all matches a set of paths that no single template describes, so every
key that could be minted is a claim about either the path or a parameter that
the service does not honour. The literal wildcard mints a parameter named
`*path`; a synthesized `{path}` promises a value that never contains an
unescaped `/` and always does; the bare prefix claims an operation that 404s.

Soundness does not require a `paths` entry — it requires that a consumer is
never *unaware* of what the service serves. A root-level array is visible,
greppable, diffable and reported by `validate`, and it leaves the path rules
in [`validate/rules/paths.rs`](../crates/kynos-openapi/src/validate/rules/paths.rs)
meaning what they say.

That also dissolves the `upgrade_unchecked` exception rather than adding one.
A connection that has left HTTP has no vocabulary in any version of the
specification, so it too is a route with no expressible template: same record,
different reason, no special case.

Two records rather than one stretched over both, because there are two
situations. `layer_unchecked` covers *real operations on real paths* whose
behaviour is unverified — the path is true, so the operation stays in `paths`
and carries a marker. `route_unchecked` has no operation to hang a marker on.

`x-kynos-document-not-authoritative` survives as a *derived* summary — true when
any operation is opaque or any route is recorded — rather than as the
mechanism. It is recomputed rather than set, in both directions, so a document
edited after the fact cannot keep a stamp it no longer earns or lose one it
does. That preserves the
one-glance signal for a consumer while confining the damage to the operations
actually affected: one unchecked layer on one subtree must not taint three
hundred operations it never touches.

## Tower interop

Interoperability is asymmetric, and deliberately so:

- **Outward is free.** A Kynos application mounts into an outer `tower` stack
  without declaring anything. Whatever wraps the service is outside the document
  and always was — `Service::into_tower_unchecked` already exists for this.
- **Inward requires a declaration.** A `tower` layer placed *inside* the stack
  must either be expressed as an `Interceptor` with a contribution, or be taken
  under a waiver that marks its subtree `Opaque`.

This keeps the invariant intact while leaving the ecosystem's migration path
open, which matters more than purity: an application that cannot adopt Kynos
incrementally will not adopt it.

## Composition

Interceptors are erased *for execution*, at every level. A router, a group and
an endpoint hold the same list and run it the same way.

That is not a performance trade against a statically composed chain — it is the
only shape available.
[`Next<'a, C>`](../crates/kynos/src/middleware/mod.rs) has two parameters and
appears in `Interceptor::intercept`'s signature, so a chain composed in the
type system would need a tail parameter that infects every interceptor anyone
writes. The terminal is boxed regardless, since `Endpoint` returns an opaque
future and a router holds endpoints it cannot name. A route with no
interceptors calls the terminal directly and pays nothing.

A phantom list of interceptor *types* rides alongside for checking, which the
objection above does not reach: it is not a composed chain, nothing is called
through it, and `Next` keeps its two parameters. `Router`, `Group` and
`EndpointBuilder` each carry one, and
[`CompatibleWith`](../crates/kynos/src/middleware/stack.rs) is what
`intercept` bounds on — so two interceptors that would collide are rejected
where the second is mounted.

Whole stacks meet at `group`, `nest`, `merge` and `mount`, and `CompatibleStack`
checks that cross-product. `mount` is why `routes!` expands to a tuple rather
than to an `Endpoints`: a collection cannot say what its members carry, so
building one would erase the endpoint-scoped interceptors the check needs. Two
*operations* are never checked against each other, since no request reaches
both.

### Two lists, not one

`Router` and `Group` carry **two** phantom lists, and the reason is that they
are checked in opposite directions.

| Parameter | Holds | Obliges |
| --- | --- | --- |
| `I` | the interceptors mounted on this scope | covers every operation, so an incoming sub-stack must clear it |
| `S` | what the scopes mounted here brought | covers subtrees, so an incoming sub-stack must **not** be compared against it |

Only `intercept` reads `S`, because only `intercept` covers everything already
mounted. `group`, `nest`, `merge` and `mount` check the incoming stack against
`I` alone and then fold it onto `S`.

Folding the two together would be the obvious simplification and is wrong: a
group's `Cors` would then be compared against a sibling group's, and a router
serving two resources that each permit their own origins would stop compiling.
`tests/cors.rs` mounts that program twice, deliberately.

The fold goes through
[`Flatten`](../crates/kynos/src/middleware/stack.rs), which erases an empty
stack — so `routes![a]` carrying `()` and `routes![b, c]` carrying `Both<(),()>`
both leave a router's type exactly as it was. That is not a nicety: without it,
`router = router.mount(..)` and a conditional mount would stop compiling, which
are the two idioms `Router::docs` returns `Self` to preserve.

**This was a hole, and it was the shape of the check rather than a slip.**
`group`, `nest`, `merge` and `mount` used to check the incoming stack and then
return `Self`, dropping it. At run time nothing was dropped —
`describe` concatenates the router's chain with each mounted operation's — so
the check was an ordering accident: `intercept` before `group` was refused, and
`group` before `intercept` was not, though the chain that runs is the same
either way. Across `nest` and `merge` it was worse, since a nested router's
group-scoped interceptors were in no type the outer router could see and
neither order refused anything. `BodySize` and `Decompression` both answer 413,
and [the decompression note](#what-bounds-a-request-before-an-interceptor-runs)
says outright that `statuses_disjoint` refuses the pair; mounted at different
scopes, it did not. `catch_panics` had the same defect on its own, returning
`Router<C, Catch>` — which is `Router<C, Catch, ()>` — so the policy parameter
was quietly doing the interceptor list's job too.

**And it had it twice.** Closing the first half left `Router<C, Catch, I>`,
which is `Router<C, Catch, I, ()>`: correct for `I` and dropping `S`, so a
`group` before a `catch_panics` before an `intercept` still compiled. The commit
that introduced `S` widened `Group::catch_panics` to four parameters and left
the router's at three, and nothing failed, because a return type naming fewer
parameters than its type has is well-formed — the rest take their defaults, and
a defaulted phantom list is an empty one. Both halves are now pinned, and
`every_builder_preserves_the_type_parameters` in
[`tests/ui.rs`](../crates/kynos/tests/ui.rs) counts the arguments of every
builder's return type so a third instance is a test failure rather than a
silent one.

`tests/ui/antipattern/` carries one case per scope that forgot, each with the
control that fails if the fix over-rejects instead.

### What the compile-time check still cannot see

One, and it is the erasure the paragraph above already names.
[`Endpoints`](../crates/kynos/src/router/endpoint/set.rs) declares
`Stacks = ()` by construction, so an endpoint-scoped interceptor that reaches a
router through a collection arrives with nothing to compare, and a router-level
interceptor adding the same header compiles.

There is no build-time backstop for it, and that is a decision rather than an
omission. A router's own chain and its groups' are visible where the document is
assembled, but an endpoint's interceptors live *inside* the endpoint and run
there — `DynEndpoint` has no way to read them, and giving it one means adding a
declaration method to the public `Endpoint` trait. That is a method returning a
description beside a method producing responses, which is the `contribution`
method [this document opens by refusing](#why-the-declaration-is-the-signature),
and it would be a declaration a third-party `Endpoint` could get wrong.

So the rule stands as `routes!` already states it: build a collection at run
time and the endpoint-scoped interceptors inside it stop being checked. Reach
for `routes!` wherever the set of routes is known when the program is compiled.

What the check compares is `const` data — `HeaderParams::NAMES` and
`ShortCircuit::STATUSES` — so it costs nothing at run time and nothing in the
emitted document. Header names compare case-insensitively, per RFC 9110.

An interceptor is handed the [`Route`](../crates/kynos/src/router/operation.rs)
it covers while it runs, through `Next::route`. That is what keeps a metric
label keyed by the `paths` key rather than by the request path, so label
cardinality is bounded and the label cannot disagree with the description.

It is *not* handed one while declaring, because there is no declaring step left
to hand it to. Declaring differently per operation is expressed by mounting
different instances at different scopes — which is the same principle as above,
and which every interceptor Kynos ships was already doing: not one of them
consulted the `Route` argument the old `contribution` method gave it.

## Conformance

The invariant is a claim about a running service, so it has to be tested like
one: a harness checking live responses against the emitted document across the
matrix of owned layers, in CI.

That harness is [`tests/matrix.rs`](../crates/kynos/tests/matrix.rs), and it
runs. It asserts in both directions, which is what makes it a conformance check
rather than a smoke test: `assert_conformance` says nothing happened the
document did not predict, and `assert_declared_responses_covered` says nothing
the document predicts went unexercised. The second is coverage over the
*contract*, and it is why every interceptor there sits on a group holding
exactly one operation — a limit mounted at the router would declare its status
on all of them, and every one would then have to be made to produce it.
[`nfr.md`](nfr.md#middleware) carries it as `enforced`, and what it caught on
its first run is in
[`testing.md`](testing.md#what-the-harness-found-on-its-first-run).

A response declaring *no* representation is checked too, which it was not at
first. Declaring nothing is a claim about the exchange rather than the absence
of one, so a body or a `Content-Type` arriving under it is reported. Until that
held, `assert_conformance` read "declares nothing" as "nothing to check", and
eight short circuits sent a problem document under a description of no content
without the matrix noticing. That defect was in what an interceptor *declares*
rather than in what it does, which is a class of error no type check reaches.

This harness is not the only instrument for that class, and is not the cheapest.
`every_short_circuit_declares_the_content_it_sends` in
[`tests/interceptors.rs`](../crates/kynos/tests/interceptors.rs) asserts the
same agreement directly, with no document, no client and no route: it drives
each short circuit's value and compares what reaches the wire against what
`Responses` declared. It is also the more exhaustive of the two here — the
matrix reached five of the eight defective implementations, the sweep covers all
ten. What the matrix buys instead is reach: it holds anything on a
live exchange, an application's own short circuit and a handler included, where
the sweep is total only over the set Kynos ships. Prefer the direct assertion
for a claim about a type, and reach for the matrix when the claim is about an
exchange.

What it is not is a property test. The matrix is enumerated, so it covers the
layers Kynos owns in the arrangements that file names — not every stack a
reader can assemble. Adding an owned layer means adding it there.

## Rationale

*Non-normative. This section explains the reasoning behind the rules above so
that revisiting them is possible on the merits.*

### What a tower layer actually does to completeness

Four behaviours account for essentially all of it:

1. **Short-circuiting** — returning 401 or 429 without reaching the handler, so
   the document omits a status the service demonstrably returns.
2. **Body and header rewriting** — changing the shape of a response the handler's
   type already described.
3. **Route injection** — serving a path that appears nowhere in `paths`.
4. **Retry** — silently altering the idempotency semantics a consumer reasons
   about, without changing any individual response.

The first three are visible in a response. The fourth is not, which is why
declaration rather than observation is the only mechanism that catches it.

### Why CORS is written here rather than borrowed

`tower-http::cors` is the obvious thing to depend on, and it does not fit. Its
configuration types — `AllowOrigin`, `AllowMethods`, `AllowHeaders`,
`ExposeHeaders`, `MaxAge` — are opaque: the inner enums are private and the
readers (`to_header`, `is_wildcard`) are `pub(crate)`. Kynos has to *read a
configuration back*, twice: to refuse a combination the protocol cannot honour
while the router is built, and to synthesize a preflight from the operations a
path declares. Neither is possible through those types, so depending on them
would buy the constructors and none of the behaviour. No other crate in the
hyper or tower stack carries CORS types on their own.

What is here therefore matches `tower-http`'s surface where the difference
would be a missing capability, and departs from it where the difference is the
point:

| `tower-http` | Kynos |
| --- | --- |
| `allow_origin(Any / exact / list / predicate)` | `allow_any_origin`, `allow_origins`, `allow_origins_matching` |
| `allow_origin(mirror_request)` | any permitted origin is echoed already, except under `allow_any_origin` alone |
| `allow_headers(Any / list / mirror_request)` | `allow_any_header`, `allow_headers`; the wildcard echoes what was asked under credentials, and names `authorization` beside `*` without them |
| `expose_headers(Any / list)` | `expose_any_header`, `expose_headers` |
| `max_age(Duration)` | `max_age` |
| `allow_credentials(bool)` | `allow_credentials` |
| `allow_methods(Any / list / mirror_request)` | derived from the operations the covering scope declares; `allow_methods` overrides |
| `vary(list)` | derived; a declared header name is a `const`, so it is not a builder's to set |
| `allow_credentials(predicate)`, `max_age(dynamic)` | absent |
| `allow_private_network` | absent |
| `CorsLayer::permissive()`, `very_permissive()` | absent |

The last four rows are decisions rather than gaps.

`allow_methods` is derived because the alternative is a second place to state
what the path already declares, and two statements of one fact drift. `vary`
is derived because `HeaderParams::VARIES` is a `const` the collision check
reads while the program is compiled; a value a builder set at run time is not
one the compiler can check two interceptors against.

A credentials predicate and a dynamic `max-age` are per-request decisions that
change what a *shared cache* may reuse, and neither varies on a field a cache
keys on. `allow_private_network` sends
`Access-Control-Allow-Private-Network`, a header from a draft that has since
been renamed; adding it is the same squatting the rate-limit headers above
refuse, and it stays out for the same reason.

`permissive()` is a one-line constructor for the configuration a service should
arrive at deliberately. `Cors::new()` permits nothing and every widening is a
call a reviewer can see.

### The cost of owning the common layers

Owning the common layers has an ergonomic dividend beyond correctness: it
removes `tower-http` version-skew pain, which is a real and recurring tax on
applications in this ecosystem.

The price is bus factor — on the order of fifteen crates of ongoing maintenance.
The mitigation is to keep each one small enough that a single contributor can
hold it in their head and own it.
