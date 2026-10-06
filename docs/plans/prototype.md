# Gearbox — Prototype

## Context

`docs/vision.md` (127 sections) proposes a product-composition layer for the
Gears platform: `gear.gdl` + `product.gdl` → Starlark evaluation → typed Rust IR → deterministic
resolver → `product.lock` → generators (Cargo crates, Docker, Helm), with CLI/GUI/MCP as clients
of one engine. It is a vision doc: nothing is built, and its examples reference runtime features
that do not exist.

This plan builds a **working vertical slice** of that architecture, with one hard rule: **GDL may
only express what `gears-rust` actually implements today.** Everything the vision assumes but the
runtime lacks (roles, shards, per-instance addressability, a K8s-Lease provider, a K8s
EndpointResolver, deployment profiles as a runtime type) is either rejected or downgraded to an
explicit diagnostic carrying a `file:line` citation — never silently faked.

A redis provider has since landed in `gears-rust`, and it turned that rule over in an instructive
way: the provider is real and configurable, but its capabilities are **not** things the runtime
implements *today* in any readable sense — it decides consistency and prefix-watch from the server it
connects to at startup. So GDL expresses what can be expressed (the provider, its primitives, that
it needs credentials) and GBX0520 states the rest, which is the same rule applied to a case it did
not anticipate: a fact that exists but not yet.

The prototype proves the vision's central promise end to end: **one gear source, three deployment
topologies, zero changes to business code** — and it produces the artefacts that make "gear = pod"
possible at all (generated application crates), which is the real blocker today, not runtime support.

Repo state: `gearbox` is `cargo new` plus the vision doc. `gears-rust` and `cargo-gears`
exist locally and were surveyed; `cargo-gears/design/ideas/gears-product-configurator-OLD.md` is
the repo-grounded predecessor and its conclusions are folded in below.

### Decisions taken with the user
- **RPC:** JSON-RPC 2.0 over stdio with LSP `Content-Length` framing → `vscode-jsonrpc` works with
  zero adapter code in Theia, no ports/CORS/auth, and the same server backs the `.gdl` language client.
- **Scope:** full pipeline — resolve → lock → application crates that `cargo build` → Dockerfile → Helm
  umbrella + `values.schema.json`.
- **Slice:** real gears from `gears-rust` + one new custom gear (vision §81's canonical acceptance test).
- **Theia:** a runnable Gearbox Studio in this repo — extension + `browser-app` + `electron-app`.
- **Layout:** engine lives in `gearbox`; `gear.gdl` files are added to `gears-rust` next to
  the real gears (additive only).
- **Build clean:** no dependency on `cargo-gears` crates; port templates by hand.
- **The catalogue is a merge, not a mirror** (ADR 0002): every fact a Rust attribute already carries
  is *projected* out of it via `syn`; `gear.gdl` *declares* only the facts that have no Rust home,
  and a description restating a projected fact is rejected.
- **CLI is a standalone `gearbox` binary**, not `cargo gears` (departs from vision §4/§97–99;
  matches the decision already recorded for the ConstructorFabric deck).
- **Spec artefacts:** a short **PRD now** (before code), the prototype as the spike, then a
  **DESIGN + ADRs grounded in what actually worked** — see §14. All in `gearbox/docs/`,
  following the `gears-rust/docs/spec-templates/gears-sdlc/` templates.
- No CI.

This plan is also copied to `docs/plans/prototype.md` as the first commit.

---

## 1. Grounded reality — what constrains the GDL

Verified against `gears-rust` (all claims spot-checked in source):

| Fact | Evidence | Consequence for GDL |
|---|---|---|
| No gear manifest exists. Gear metadata lives **only** in `#[toolkit::gear(name, deps, capabilities, ctor, client, lifecycle)]`, and no `gear.toml` exists anywhere (`find -name gear.toml` = 0; the platform added some later, folded into `gear.gdl` on 2026-10-02) | `libs/toolkit-macros/src/lib.rs` | `gear.gdl` carries **only** genuinely new information — enforced, not merely intended, by `cpt-gearbox-fr-gdl-no-restatement` |
| The gear attribute's location is not uniform: 34 of 44 at `src/gear.rs`, 8 at `src/module.rs`, 2 nested; and `gears/mini-chat/mini-chat` declares **three** gears in one crate | `grep -rln '#\[toolkit::gear('` over `gears/` + `examples/` | projection needs a locator: scan `src/` by default, optional `cargo(attr = …)` to narrow, exactly-one-match required (`cpt-gearbox-fr-attribute-location`) |
| `capabilities` is a **closed set of 7**: `db, rest, rest_host, stateful, system, grpc_hub, grpc` | same, `Capability` enum | GDL exposes exactly these, nothing more |
| **`deps` means link-time co-location** — the macro emits `pub use ::crate as _gear_dep_x` to keep `inventory::submit!` alive | same | must be named so it can't be confused with contract consumption |
| **Missing deps are a hard error**, so co-location is a *downward closure, not a partition* — `types-registry` is linked into every application whose closure names it, and two anchors sharing a dep do **not** merge | `RegistryError::MissingDeps`, `libs/toolkit/src/registry.rs:589` | applications **overlap**; the resolver must model that, and `deps` edges are **never cuttable** |
| Contract kind is the **trait-name suffix**: `…Api` / `…Embedded` / `…Backend` / `…Extension`; only `Api`\|`Backend` are remote-capable | `libs/toolkit-contract/src/descriptor.rs` | placement constraint is derivable statically |
| Contract version is real: `#[toolkit::contract(gear, version)]`, trailing major on the name must agree, parallel majors coexist | `toolkit-contract-macros/src/parse.rs:80-97` | version-mismatch check works without cargo metadata |
| **`#[toolkit::consumes]` emits a REST resolving client only** — `{Contract}RestResolvingClient` is hardcoded, no gRPC branch | `consumes.rs:166` | `transport = grpc` on a cut edge is **unsupported** |
| `consumes` derives `owner_gear` from **kebab of the struct ident**, not from `gear(name=)`; mismatch only `warn!`s | `consumes.rs` module docs | first-class `validate` check — a mismatch silently breaks `consumer_wiring` |
| `consumes` injects **no** topo dep | same | remote providers need not be in the local registry |
| `WireOutcome::{Local,Remote}`; `try_get_local` short-circuits (no readiness gate), else `register_remote_proxy` + `EndpointResolver` gates `/readyz` | `libs/toolkit/src/discovery.rs`, `host_runtime.rs:546-554` | local-vs-remote is genuinely *derived* from placement |
| `EndpointResolver` impls: `Directory` (gRPC), `Static` (config), `Null`. **No K8s-DNS resolver** | `discovery.rs` | k8s profile must use `Static` and say so |
| `ClientWiring::{Local, Rest{endpoint,tuning}, Grpc{endpoint,tuning}}`; key `gears.<gear>.config.client_wiring.<contract_snake>`; absent ⇒ Local | `toolkit-contract/src/wiring.rs` | transports are exactly `{local, rest, grpc}` |
| Deployment profiles are **prose only** — `grep HostWorkers --include=*.rs` = 0 hits; `DESIGN.md:230` has an unchecked box. What exists is per-gear `RuntimeKind::{Local,Oop}` + `ExecutionConfig` | `bootstrap/config/mod.rs:86-93` | profile is a *Gearbox* concept projected onto per-gear runtime + topology |
| Only `LocalProcessBackend` is implemented; `BackendKind::{K8s,Static,Mock}` are bare variants | `libs/toolkit/src/backends/mod.rs:12-19` | self-hosted = one machine, full stop |
| Cluster registry is **hardcoded Rust**: cache `{standalone, postgres, redis}`, lock `{postgres, redis}`, leader-election `{}` (always SDK CAS default) | `gears/system/cluster/cluster/src/gear.rs:120-127` | redis landed after this row was first written; still no etcd/NATS/K8s-Lease. Codegen must emit Rust, not just YAML |
| Cache capability matrix is **asymmetric**: `standalone` → linearizable ✔ prefix_watch ✔ but process-local; `postgres` → linearizable ✔ prefix_watch ✘; `redis` → declares neither | `CacheFeatures::new(true)` in standalone `cache.rs:281`; `new(false)` in postgres `cache/mod.rs:174`; computed from the connected server in redis `cache/mod.rs:347-358` | `cache(linearizable + prefix_watch)` is **unsatisfiable multi-process** — the best real capability demo in the repo. redis is a third shape: its capabilities are not composition-time facts at all, which is GBX0520 |
| **No `role` concept.** OoP directory identity is `OopServeOptions.gear_name`, hardcoded per binary, no config override. Labels exist for OoP only, equality-AND only | `bootstrap/oop.rs`, `system-sdks/.../labels.rs` | roles/shards ⇒ diagnostic, excluded from resolution |
| `gears.<c>.config.consumer_wiring.<dep>` **cannot** be set via `APP__` env — `remap_gear_env_key` remaps `_`→`-` only in the segment right after `gears` | `config/mod.rs:429` | Helm must put consumer wiring in the **ConfigMap**, never env |
| `registered_gears.rs` is real and load-bearing; its own header asks for a generator | `apps/cf-gears-example-server/src/registered_gears.rs` | exactly what we generate |
| `--list-gears`, `--dump-gears-config-yaml/json` exist on the example server | `apps/cf-gears-example-server/src/main.rs` | use as lock-file **verification oracles** |
| `cf-api-contracts` has **no `[lib]` section** ⇒ lib ident is `cf_api_contracts` | its `Cargo.toml` | lib ident must be **declared** in `gear.gdl`, never derived from the crate name |
| Only one Helm chart in the repo (mini-chat), hand-written | `gears/mini-chat/deploy/helm/mini-chat/` | golden reference for the Helm generator |

Also: `make oop-example` is stale (`--features oop_gear` doesn't exist), so the Profile-2 baseline
must be re-established by hand before any generated output is trusted.

---

## 2. Architecture

### 2.1 Rust workspace

Replace the single-package `Cargo.toml` with a virtual workspace. Add `rust-toolchain.toml`
pinning `1.97.0` to match `gears-rust` (local is 1.96.0 — `rustup toolchain install 1.97.0` first).

```
gearbox/
  Cargo.toml              # [workspace] members = ["crates/*"]
  rust-toolchain.toml     # 1.97.0
  crates/
    gearbox-ir/           # canonical typed model + IDs + diagnostics. Pure data.
    gearbox-gdl/          # Starlark host → IR. The only crate naming `starlark::`.
    gearbox-lock/         # canonical product.lock read/write + lock_hash + diff
    gearbox-verify/       # syn scan of gear crates → the projected half of the
                          # catalogue (ADR 0002; rename candidate: gearbox-scan)
    gearbox-resolve/      # deterministic resolver + explanation graph. Pure fn.
    gearbox-gen/          # artefact emission → FileSet. Never touches the FS.
    gearbox-engine/       # facade; the ONLY crate doing filesystem/process I/O
    gearbox-rpc/          # JSON-RPC over stdio, LSP framing
    gearbox-cli/          # [[bin]] name = "gearbox"
  templates/              # minijinja sources, include_str!
  products/payments-demo/product.gdl
  fixtures/               # golden locks, effective-config dumps, rpc-schema.json
  ide/                    # npm workspaces (§6)
```

Dependency DAG is strictly layered: `ir` ← {`gdl`, `lock`, `verify`, `resolve`, `gen`} ←
`engine` ← {`rpc`, `cli`}. **Boundary guarantee (vision §7):** a test in
`gearbox-engine/tests/no_frontend_deps.rs` runs `cargo metadata --no-deps` and asserts the
engine's dependency set names no CLI/RPC/UI crate.

**ADR 0002 changes `gearbox-verify`'s position.** Under projection the `syn` scan is not a
validate-time check but a *catalogue-load input*, so descriptor assembly consumes both it and
`gearbox-gdl`; nothing can build a `GearDescriptor` from GDL alone. The name no longer fits either —
it verifies nothing, it reads — so `gearbox-scan` is the rename candidate. Decide when the crate is
actually written (M2), not now.

Pinned deps: `starlark` + `starlark_syntax` (pin the exact minor — pre-1.0, the `Dialect` and
`#[starlark_module]` surface moves), `minijinja` 2 (`custom_syntax`), `syn` 2 (`full`,
`extra-traits`, `visit`), `toml` 0.9, `serde-saphyr` (same YAML crate the runtime deserializes
with — round-trip fidelity for free), `schemars` 1.2, `ts-rs` 11, `blake3`, `petgraph`, `heck`.

### 2.2 Key design rules carried from the vision
- **Template text. Serialize data.** minijinja for Rust/Dockerfile/NOTES.txt; **serde only** for
  `product.lock`, `values*.yaml`, `values.schema.json`, `Chart.yaml`. A test enforces that no
  generator ever templates a YAML *structure*.
- **Helm delimiter collision:** minijinja for Helm template bodies uses `<< >>` / `<% %>` / `<# #>`
  so Helm's `{{ }}` passes through untouched.
- **`selected` vs `resolved`** is a generic `Selected<T> { selected: Choice<T>, downgraded_by:
  Option<DiagnosticCode> }` next to every derived value.
- **Provenance is built during resolution**, never reconstructed. Node ids are content-derived
  (`decision:cut:{consumer}->{provider}`), never counters, so the graph is byte-stable.

### 2.3 Catalogue loading is staged, because the data costs differ by orders of magnitude

An engine property, not an RPC detail: the same stages back the single-shot CLI path and the
incremental one the Studio needs. ADR `cpt-gearbox-adr-staged-catalogue-loading` is the authority.

| stage | work | what it yields | cost on the slice |
|---|---|---|---|
| **S0** discover | one walk per source root for `gear.gdl` | paths only | one walk |
| **S1** declare | evaluate each description (Starlark, tiny); locate documents (`stat` only) | `display_name`, `description`, `category`, `visibility`, `package`/`sdk` locators, `requires`, `serves`, `cluster_plugins`, `docs` | ms per file |
| **S2** project gear | `syn` over the gear crate's `src/` | `id`, `runtime_caps`, `colocated_deps`, `lifecycle`, `client_trait`, `fills`, provider transports | **165 files** |
| **S3** project sdk | `syn` over the SDK crates | contracts, extension points, GTS types, vendor defaults | 90 files, **crates shared** |
| **S4** join | contract merge, plugin resolution | GBX0206, GBX0511–GBX0517 | cheap, needs S2+S3 of the participants |

Measured: 255 `.rs` files for the 14-gear slice at ~0.7 s; **2658** `.rs` files under `gears/`, so a
full registry is roughly ten times that.

**The consequence that shapes the UI: `GearId` is projected, so it does not exist until S2.** At S1
there is a name to show and no identifier to key by, so a tree must key rows by `gdl_path` and join
the `id` later. An implementation that keys by `id` has no choice but to block on every crate — which
is why this is a requirement and not an optimisation.

**"Not yet computed" is a separate list, never a third state on a field.** `CatalogueScan` carries
`pending: Vec<PendingGear>` beside `catalogue.gears`; a gear in `gears` is complete and readable
without qualification. `Option::None` and an empty `Vec` keep exactly one meaning — *absent* — which
is what stops a consumer rendering "no GTS types" for a gear nobody has looked at yet.

**Parsing each crate once per load is a precondition of all this**, and is done: a staged loader that
re-parses shared crates cannot be cheap however it is scheduled. Before the cache,
`tenant-resolver-sdk` was read four times on the slice (host plus three plugins) and
`authn-resolver-sdk` three; now the same load parses **19 crates for 25 requests**, which
`gearbox catalogue --format text` reports.

---

## 3. GDL surface

> This section is the surface as planned. The surface as built is `docs/gdl.md`,
> which is normative where the two differ.

### 3.1 Host API (globals; no `load()` needed for the vocabulary)

Functions: `gear`, `product`, `cargo`, `provide`, `consume`, `rest`, `grpc`, `lifecycle`,
`endpoint`, `provider`, `cluster_profile`, `use_gear`, `source`/`path`/`git`/`registry`,
`embedded`/`self_hosted`/`kubernetes`, `bind`, `application`, `fail`.

Frozen namespaces (a typo is an `AttributeError` at eval time, not a silently-null string):
- `cap.{db,rest,rest_host,stateful,system,grpc_hub,grpc}` — the closed 7
- `transport.{local,rest,grpc}`
- `contract_kind.{api,embedded,backend,extension}`
- `cluster.{cache,leader_election,lock}` — the only three primitives that exist
- `cluster_cap.{linearizable,prefix_watch}`
- `binding_mode.{auto,local,remote}`
- `prefer.{existing_infrastructure,fewer_applications,isolate}`

`print` is overridden to emit a `Hint` diagnostic — **stdout is the RPC channel and nothing else
may write to it.**

### 3.2 Keeping GDL declarative (vision §12) — three layers

1. **Dialect lockdown:** `enable_def: false`, `enable_lambda: false`, `enable_top_level_stmt: false`,
   `enable_f_strings: false`, `enable_types: Disable`, `enable_load: true`, `enable_load_reexport: false`.
2. **Token blacklist** (this is the enforcement that actually matters — comprehensions and ternaries
   are legal in the dialect): lex with `starlark_syntax::lexer::Lexer` before parsing and reject
   `if elif else for while def lambda not and or in` → **GBX0103** with the exact span.
3. **No mutable host state and no readable inputs:** globals are frozen; `gear()`/`product()` write
   once into `Evaluator::extra` (second call = GBX0105). No host function exposes the deployment
   profile, environment, clock, or filesystem — so a script *cannot* branch on resolution inputs
   even if it could branch.

`load()`: a `FileLoader` restricted to relative paths inside the declaring file's source root,
rejecting `..` escapes (GBX0104), memoizing frozen modules by canonical path. Loaded fragments may
contain assignments only.

### 3.3 Rejected / downgraded vision constructs

| Construct | Verdict | Behaviour |
|---|---|---|
| `role(...)`, `sharded`, `instance_addressable` | **Downgraded** | parsed into `declared_roles`, **excluded from resolution**; GBX0318 `Warning` when a selected gear declares several, GBX0602 `Warning` when one asks for labels |
| `provider("redis"\|"k8s-lease"\|"etcd"\|"nats")` | **Rejected** | GBX0505 `Error` listing the actual registry contents |
| `registry(package, version)` source | **Rejected, later shipped** | was GBX0605; cargo now fetches the package and its Cargo closure, and the code is retired |
| `transport.grpc` on a cut edge | **Downgraded** | GBX0402 `Warning`, forced to `rest` |
| `kind = service()` / plugin model | **Deferred** | unknown kwarg (GBX0106) — no runtime concept behind it |
| Deployment profile as a runtime type | **Reframed** | GBX0606 `Hint` once per resolve |

### 3.4 Representative `gear.gdl` — the new custom gear

`gears-rust/gears/payments-audit/payments-audit/gear.gdl`:

```python
AUDIT_SDK   = cargo(crate = "cf-gears-payments-audit-sdk", lib = "payments_audit_sdk",
                    path = "../payments-audit-sdk", features = ["rest-client"])
PAYMENT_SDK = cargo(crate = "cf-api-contracts-sdk", lib = "api_contracts_sdk",
                    path = "../../../examples/toolkit/api-contracts/api-contracts-sdk",
                    features = ["rest-client"])

gear(
    # NOT DECLARED HERE, projected from Rust (ADR 0002). Writing any of these is
    # GBX0210 `cpt-gearbox-fr-gdl-no-restatement`, which names the owning attribute:
    #   id            <- #[toolkit::gear(name = "payments-audit")]
    #   runtime_caps  <- capabilities = [rest, stateful]
    #   colocated_deps<- deps = [cluster]   (link-time; see the note below)
    #   lifecycle     <- lifecycle(entry = "serve", stop_timeout = "15s")
    #   client_trait  <- client = ...
    #   contract id / version / kind on every provide+consume below

    name = "Payments Audit",
    description = "Cluster-cached audit trail; the elected leader reconciles it periodically.",
    category = "example",
    visibility = "public",

    # lib is MANDATORY, never derived — cf-api-contracts has no [lib] section.
    # attr is OPTIONAL: omitted means scan this crate's src/ tree and require
    # exactly one #[toolkit::gear]. payments-audit puts it at the modal location.
    package = cargo(crate = "cf-gears-payments-audit", lib = "payments_audit",
                    path = ".", link = ["payments_audit"],
                    attr = "src/gear.rs"),

    provides = [
        provide(contract = "PaymentsAuditApi",
                rust = "payments_audit_sdk::PaymentsAuditApi", sdk = AUDIT_SDK,
                local = "Self::build_local",
                # No `transports`: the set comes from this gear's own
                # #[toolkit::provides(transports = [...])], checked against
                # whether `PaymentsAuditApiRest` / `...Grpc` exist beside the
                # base trait in the sdk. Declaring it here is GBX0210.
                rest = rest(base_path = "/api/v1/payments-audit")),
    ],

    # `from_` and `critical` are product-level; the contract identity and the fact
    # that the edge exists at all come from #[toolkit::consumes]. Remote-capable +
    # annotated in Rust => the resolver MAY cut this edge.
    consumes = [
        consume(contract = "PaymentApi",
                rust = "api_contracts_sdk::PaymentApi", sdk = PAYMENT_SDK,
                from_ = "api-contracts", critical = False),
    ],

    requires = [
        # `profile` is mandatory and is a join key: it must name a profile this
        # crate implements as `impl ClusterProfile { const NAME }`. No default --
        # a defaulted `"default"` would resolve to a scope nothing registered.
        cluster.cache(profile = "event-broker", capabilities = [cluster_cap.linearizable]),
        cluster.leader_election(profile = "event-broker"),
    ],

    serves = [endpoint(name = "rest", via = "rest_host")],
)
```

Note what is *gone* versus the pre-ADR-0002 draft: `id`, `runtime_caps`, `colocated_deps`,
`lifecycle`, and the `version`/`kind` arguments on `provide`/`consume`. `colocated_deps` in
particular was never GDL's to own — the macro emits `pub use ::cluster as _gear_dep_cluster`, so the
authority for that edge is the linker, and a missing dep is `RegistryError::MissingDeps`. It stays
uncuttable for exactly that reason.

The `cluster` gear's `cluster_providers` are likewise projected — so a new provider registered in
Rust appears in the catalogue with no edit anywhere, and **GBX0204 disappears entirely**: there is no
second list to rot. It takes three hops, because no single place has the whole answer:

| hop | source | yields |
|---|---|---|
| 1 | `ClusterGear::provider_registry()` — the `with_*_provider` chain | which provider types, for which primitive |
| 2 | the plugin crate's `impl Cluster*Provider::provider()` → its `PROVIDER_NAME` const | the operator-facing name |
| 3 | the plugin crate's unique `impl ClusterCacheBackend`/`DistributedLockBackend`/`LeaderElectionBackend` | `consistency()` and `features()` |

Hop 3 is the one that surprises: **the provider traits carry no capability at all**, only a name and
a `build_*` factory. Capabilities live on the backend the factory returns behind an `Arc<dyn _>`, and
no source-level parse can follow that value flow. It does not have to — within one plugin crate there
is exactly one impl of each backend trait, so the impl is locatable by trait, and a crate that grows
a second one is reported (GBX0510) and narrowed rather than guessed.

Hop 2 needs one fact Rust cannot supply: the registry writes
`standalone_cluster_plugin::StandaloneCacheProvider`, and nothing in that expression says which
directory the crate is in. So the cluster description declares a locator — the same role `sdk` plays
for a contract:

```python
cluster_plugins = [
    cluster_plugin(
        package = cargo(crate_name = "cf-gears-standalone-cluster-plugin",
                        lib = "standalone_cluster_plugin",
                        path = "../plugins/standalone-cluster-plugin"),
        process_local = True,        # declared: no Rust construct states it
        needs_credentials = False,
    ),
]
```

`process_local` and `needs_credentials` stay declared deliberately. The nearest signal in Rust is
that one plugin's options carry a `connection_string` and the other's do not, and reading deployment
semantics out of that would be an inference rather than a fact. `process_local` is also what GBX0503
rests on, which is a reason to write it down rather than derive it almost-correctly.

The SDK's fall-back backends are projected as a **rule**, not a value: `defaults/{lock,leader}.rs`
compute `Features::new(self.cache.consistency() == Linearizable)`, so leader election and lock inherit
whatever the profile's cache declares. Both real caches are linearizable today, so the rule always
yields `true` — recording the rule rather than that answer is what keeps it correct when a third
cache lands.

### 3.4.1 Why `attr` exists — `mini-chat`

One crate can declare several gears. `gears/mini-chat/mini-chat` declares three, so it needs three
descriptions, each pinning its own attribute:

```python
# gears/mini-chat/mini-chat/gear.gdl                     -> "mini-chat"
package = cargo(crate = "cf-gears-mini-chat", lib = "mini_chat", path = ".",
                attr = "src/gear.rs",
                link = ["mini_chat",
                        "mini_chat::infra::plugins::static_audit",
                        "mini_chat::infra::plugins::static_model_policy"])

# .../src/infra/plugins/static_audit/gear.gdl  -> "static-mini-chat-audit-plugin"
package = cargo(crate = "cf-gears-mini-chat", lib = "mini_chat", path = "../../../..",
                attr = "src/infra/plugins/static_audit/gear.rs")
```

Omitting `attr` here is `GBX0211`: three candidates, and the diagnostic lists all three paths plus
the literal `attr = "…"` line to add. This is also why a description cannot be required to sit at
the crate root — `gdl_path` is already a `RelPath`, so the IR needs nothing new.

### 3.5 `product.gdl`

`gearbox/products/payments-demo/product.gdl` — all three profiles declared as **data**,
selected by `gearbox resolve --profile <id>`. No `if` anywhere; profile-scoping is a
`profiles = [...]` list field on `bind`/`cluster_profile`/`application`.

```python
product(
    id = "payments-demo", name = "Payments Demo", version = "0.1.0",
    sources = [source(id = "gears-rust", at = path("../../../gears-rust"))],
    profiles = [
        embedded(id = "dev"),
        self_hosted(id = "local", host = "gateway", worker_discovery = "directory",
                     target_dir = "../../../gears-rust/target"),
        kubernetes(id = "prod", discovery = "static", namespace = "payments",
                   image_registry = "registry.example.com/payments"),
    ],
    default_profile = "dev",
    gears = [ use_gear("api-gateway", source = "gears-rust"),
              use_gear("service-discovery", source = "gears-rust"),
              use_gear("api-contracts", source = "gears-rust"),
              use_gear("api-contracts-consumer", source = "gears-rust"),
              use_gear("payments-audit", source = "gears-rust") ],
    bindings = [
        bind(consumer = "payments-audit", contract = "api-contracts/PaymentApi@v1",
             mode = binding_mode.remote, transport = transport.rest,
             profiles = ["local", "prod"]),
    ],
    # `name` must match a profile some gear implements as `impl ClusterProfile`;
    # this is the operator side of the same join key the gear declares.
    # `provider("...")` here *references* a catalogue provider by name — it is
    # not the retired gear-side `provider(...)` record, which described one.
    cluster_profiles = [
        cluster_profile(name = "payments-audit", cache = provider("standalone"), profiles = ["dev"]),
        cluster_profile(name = "payments-audit", profiles = ["local", "prod"],
            cache = provider("postgres",
                connection_string = "postgres://payments@${PG_HOST}:5432/payments?password=${PG_PASSWORD}",
                schema = "cluster", pool_max_size = 10)),
    ],
    applications = [application("audit", anchor = "payments-audit", replicas = 2, profiles = ["prod"])],
    preferences = [prefer.existing_infrastructure(), prefer.fewer_applications()],
)
```

Note what it does **not** list: `grpc-hub`, `authn-resolver`, `types-registry`, `cluster` — they
arrive via `colocated_deps` closure, which is exactly the fact the Graph widget makes visible.

**What shipped in M2, and where it differs from the example above.**
`products/payments-demo/product.gdl` exists and evaluates
(`gearbox product --file products/payments-demo/product.gdl`). It lists the four slice gears plus `cluster`, rather
than five: `payments-audit` was to be the new custom gear, and it never arrived — M6 turned out not
to need it, because the description's own severable edge already splits `api-contracts` into a second
application. A `use_gear` naming it would still reference a gear no source provides. Its cluster scope,
`event-broker`, went to `api-contracts-consumer` instead: that crate now carries the
`impl ClusterProfile` marker, so the scope has a requester that exists. `cluster` itself has to be
selected explicitly — nothing pulls it, because the only gear declaring `deps = [cluster]` has no
description, and without it in the closure the provider registry is empty.

Three surface decisions settled by implementing it:

- **Single-argument constructors are positional** — `path("…")`, `provider("…")`, `use_gear("…")`,
  `application("…")`. Everything else stays keyword-only, so `path(at = "…")` would only name the obvious
  while `bind(consumer = …, contract = …)` genuinely needs the labels.
- **`provider(...)` is the only function taking `**kwargs`.** A cluster plugin's option schema is
  genuinely open — the SDK hands a plugin a raw JSON map and keeps the schema out of the framework —
  so there is no arity to check. Everywhere else the parameters are spelled out, which is what makes
  an unknown argument GBX0106 instead of a silently ignored key. Options are sorted by key on the way
  into the IR, because `SmallMap` preserves the order the author happened to type and the lock must
  not depend on it.
- **`gear()` and `product()` live in different global sets**, so a file that calls the wrong one fails
  at the call rather than producing half of each.

An option that is not a string, integer, bool, list or map is refused rather than encoded as JSON
`null` — the lock is TOML, and TOML has no null.

**GBX0110 is now reachable, and GBX0107 appears not to be.** Duplicate profile-scoped declarations
are real and tested: two `bind` entries for one edge in one profile, an unscoped entry colliding with
a scoped one, one cluster scope bound twice, and a duplicate profile id. Disjoint scopes correctly do
*not* collide, which is the whole point of declaring profiles as data.

`GBX0107` (`GdlDowngraded`, "accepted for forward compatibility but excluded from resolution") has no
honest firing site at evaluation time: every case it was meant to cover has a more specific code —
`GBX0318`/`GBX0602` for roles and shards. It also carries
`requires_evidence = true`, so firing it would mean citing a runtime limitation it does not name.
Either it belongs to the resolver (M4) or it should be retired the way `GBX0201`-`GBX0205` were; not
invented a use for in the meantime.

---

## 4. Typed IR (`crates/gearbox-ir/`)

Every public type derives `Serialize, Deserialize, TS, JsonSchema`. Stable-ID newtypes with
validating constructors as the single validation point:

| Newtype | Format |
|---|---|
| `GearId` | `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` — exactly `toolkit-macros::validate_kebab_case` |
| `ContractId` | `{gear}/{BaseTraitName}@v{major}` — e.g. `api-contracts/PaymentApi@v1` (Rust's trailing major is stripped, so v1/v2 are one family) |
| `ApplicationId` | kebab; derived from the anchor `GearId`, `-2`/`-3` on collision |
| `ProviderId` | `{primitive}:{provider}` |
| `RequirementId` | `{gear}#{namespace}[{ordinal}]` |
| `CapabilityId` | `{namespace}.{name}` — e.g. `cluster.cache.linearizable` |
| `NodeId` | `{kind}:{payload}` — content-derived, never a counter |

Core shapes (full definitions in implementation):

- `Catalogue { gears: BTreeMap<GearId, GearDescriptor>, contracts, sources, diagnostics }`
- `GearDescriptor { id, display_name, visibility, source, gdl_path, package: CargoRef,
  runtime_caps: BTreeSet<RuntimeCap>, colocated_deps: BTreeSet<GearId>, lifecycle, provides,
  consumes, requires, serves, client_trait, cluster_providers, declared_roles, config_schema }` —
  the **merge** of two disjoint origins (ADR 0002), which the IR does not distinguish because by the
  time a descriptor exists the distinction is spent:

  | Origin | Fields |
  |---|---|
  | **projected** from Rust | `id`, `runtime_caps`, `colocated_deps`, `lifecycle`, `client_trait`, `cluster_providers` (primitives, names, capabilities), cluster profile names, the contract identity/version/kind inside `provides`/`consumes`, the **transports** each provider wires up (from its own `#[toolkit::provides]`, checked against the projection traits the sdk declares), the **GTS types** its sdk declares, and `extension_points` / `fills` / `vendor_selector`. From *Rust*, not only from an *attribute*: provider names come from a `PROVIDER_NAME` const, capabilities from a backend trait impl, transports from which projection traits exist, and vendor defaults from either `impl Default` or `#[serde(default = "…")]` |
  | **declared** in `gear.gdl` | `display_name`, `visibility`, `package`, `sdk` (crate locator for the plugin-API traits), `requires`, `serves`, `cluster_plugins` (crate locator plus `process_local`/`needs_credentials`), `declared_roles`, `config_schema`, `category`, and the product-level parts of `provides`/`consumes` (rest base path, sdk, local ctor, `from_`, `critical`) |
| **discovered** on the filesystem | `docs` — PRD, DESIGN, ADRs and a checked-in `OpenAPI` document, found under `docs/` beside the gear and in its parent. Neither projected nor declared; `docs(...)` overrides only when a gear is laid out differently |
  | assigned by the loader | `source`, `gdl_path` |

- `CargoRef { crate_name, lib_ident /* MANDATORY */, path, features, default_features, link, attr }`
  — `link` is the `use X as _;` idents, allowing nested plugin module paths; `attr:
  Option<RelPath>` narrows the attribute scan and is required only when a crate declares more than
  one gear (`mini-chat` does — see §3.4.1). `RelPath` already rejects absolute paths, `..` and
  backslashes, so it needs no extra confinement check.
- `RuntimeCap { Db, Rest, RestHost, Stateful, System, GrpcHub, Grpc }` — closed.
- `ContractKind { Api, Embedded, Backend, Extension }` with `provides()`, `requires()`,
  `remote_capable()` (= `Api | Backend`); `remote_capable == false` is the placement constraint.
- `RequirementKind::{ Contract { contract, from, resolving_client }, Cluster { primitive, profile } }`
- `ResolvedProduct { schema_version, product, sources, gears, applications, bindings, cluster,
  cuttable_if_declared: Vec<CutCandidate>, provenance }`
- `ResolvedApplication { name, kind: Host|Worker, anchor /* == OopRunOptions.gear_name for workers */,
  gears /* topo-sorted, MAY OVERLAP other applications */, replicas, entrypoint, bin_name,
  crate_name, listens, rest_host, grpc_hub, needs_db, spawns }`
- `ResolvedBinding { consumer, consumer_application, contract, provider, provider_application, mode,
  transport, mechanism, endpoint_source, endpoint, critical, selected: Selected<_> }`
- `BindingMechanism { ColocatedLocal, ConsumesStatic, ConsumesDirectory, ProvidesClientWiring }`
  — names the *actual code path*, not an abstraction.
- `ClusterResolution::{ Provider { name }, SdkCasDefault { over_cache } }`
- `CutCandidate { consumer, provider, contract, blocked_by: CutBlocker, suggested_edit, file,
  estimated_savings }` with `CutBlocker { ColocationClosure, UndeclaredHubEdge,
  InProcessOnlyContract, NoRemoteTransport, ProfileForbidsSplit }`
- `Diagnostic { code, severity, message, location /* LSP-shaped */, related, subject: NodeId,
  help, evidence /* real file:line in gears-rust */ }`
- `ExplanationGraph { nodes, edges: Vec<ProvenanceEdge> }` with `ProvenanceKind { Declared,
  ColocatedBy, SelectedBy, DerivedFrom, ConstrainedBy, PreferredOver, DowngradedBy, Diagnosed }`

---

## 5. Resolver

`Resolver::resolve(&Catalogue, &ProductIntent, ProfileId) -> Resolution`. Pure — no I/O, clock, or
env. Deterministic by construction: iterate `BTreeMap`/`BTreeSet` only; sort every output `Vec` by
its ID tuple; ties break lexicographically.

1. **Profile projection.** Filter `bindings`/`cluster_profiles`/`application_pins` by
   `profiles.is_empty() || contains(profile)`. Duplicate matching keys → GBX0110.
2. **Closure.** BFS from `selected_gears` over `colocated_deps` (deterministic pop order).
   Unknown gear → GBX0301; cycle → GBX0302.
3. **Cut-candidate classification.** Per `consumes` requirement: no provider → GBX0404; major
   mismatch → GBX0405 (**exact major equality**, no widening — parallel majors are by design and
   there is no adapter); `!remote_capable` → not cuttable + GBX0403 error; provider ∈ closure →
   not cuttable + GBX0407 (`try_get_local` wins regardless of config); no `transport.rest` →
   GBX0406. Otherwise cuttable.
   Separately, the **"edges I would cut if declared"** report: for every `(c, p)` where `p ∉
   closure(c)`, there is no `consumes` edge, but `p` provides a remote-capable contract — record
   `UndeclaredHubEdge` with the literal `#[toolkit::consumes(...)]` line to add and its file.
   **Never cut** (direct `hub.get::<dyn X>()` is common and splitting breaks at runtime). GBX0401.
   *This report is a deliverable, not a limitation.*
4. **Application partition.** `Embedded` → exactly one application (anchor = the unique `rest_host` gear);
   any split request or `replicas > 1` → GBX0307 + downgrade. `SelfHosted`/`Kubernetes` → anchors
   = host ∪ chosen-cut providers ∪ pins; **one deterministic pass** over cut candidates (no scoring,
   no search — vision §41); `P(a).gears = topo_sort(closure(a))`, so applications **overlap**.
5. **Structural checks.** >1 `rest_host`/`grpc_hub` per application → GBX0303/0304 (mirrors
   `registry.rs`); `rest_host` inside a Worker → GBX0312 (workers serve via `oop_serve`'s own
   router); Directory discovery without `service-discovery` → GBX0308 and without `grpc-hub` →
   GBX0309 (`run_oop_spawn_phase` blocks on `wait_for_grpc_hub_endpoint()`); missing `target_dir` →
   GBX0310; orphan gear → GBX0311. `SelfHosted` always emits GBX0604 (local OS processes only).
6. **Binding derivation.** Same application ⇒ `Local` / `ColocatedLocal` / no endpoint. Different ⇒
   `Remote`; transport priority: explicit-and-supported → gRPC requested (GBX0402, downgrade to
   REST) → provider ∩ `{Rest}` → else GBX0406 and revert to Local by merging the applications.
   Mechanism `ConsumesStatic` or `ConsumesDirectory` per profile discovery. Env-only wiring request
   → GBX0409 (`remap_gear_env_key` cannot express it).
7. **Cluster matching** over `{standalone, postgres, redis}` + SDK CAS defaults. Table-driven from
   `ClusterProviderDecl.capabilities`, which is itself projected from the `with_*_provider` calls by
   `gearbox-verify` rather than declared and diffed (ADR 0002), so a provider added in Rust cannot
   go missing from the table. Auto ranking:
   (a) `prefer.existing_infrastructure` favours a provider already bound for another primitive,
   (b) multi-process-capable first when >1 application or any replicas>1, (c) lexicographic. No
   candidate + cache bound ⇒ `SdkCasDefault` (GBX0504) — which is *always* the leader-election
   answer, since zero LE providers are registered. Then the hard guard: a **process-local** provider
   with >1 application or replicas>1 → **GBX0503 `Error`** ("`standalone` cache is in-memory and
   per-process; leader election over it elects a leader per replica"). This catches a silent
   correctness bug the runtime would happily start with — the single most valuable diagnostic here.
8. **`stateful` + replicas without leader election** → GBX0507 `Warning`.
9. **Explanation graph** — every decision pushes its node + provenance edges at the moment it is made.
10. **Canonical ordering + `lock_hash`** = `blake3(canonical TOML with lock_hash elided)`.

`Error` diagnostics do **not** abort resolution — a best-effort product plus errors lets the UI
render a partial graph. `gearbox-engine` refuses to *write* the lock or generate when any `Error`
is present, unless `--allow-errors`.

Diagnostic ranges: `GBX01xx` GDL, `GBX02xx` catalogue assembly, `GBX03xx` topology,
`GBX04xx` binding, `GBX05xx` cluster, `GBX06xx` runtime gaps, `GBX07xx` generators.

**ADR 0002 retires five of the nine `GBX02xx` codes** already declared in
`crates/gearbox-ir/src/diagnostics.rs`, because the divergence they detect becomes unrepresentable:

| Code | Fate |
|---|---|
| `GBX0201` name, `GBX0202` deps, `GBX0203` caps, `GBX0204` provides/cluster-providers, `GBX0205` consumes | **retired** — one authority each, so there is no second value to differ from |
| `GBX0508` profile not implemented, `GBX0509` provider unprojectable, `GBX0510` backend ambiguous, `GBX0607` cluster not deployable | **added** — the cluster projection's own failure modes; see §3.4 |
| `GBX0206` gear name is not kebab of its struct ident | **survives, and matters more** — a Rust-internal inconsistency the runtime only `warn!`s about, which silently breaks the `consumer_wiring` override key |
| `GBX0207` contract suffix or trailing major disagrees | **retired** — both halves are compile errors in `toolkit-contract-macros` (`parse.rs:70` unrecognised suffix, `parse.rs:83` marker vs `version`), so a crate exhibiting either does not build and never reaches a catalogue. Replaced by a differential test pinning our suffix and marker rules to the macro's own vectors, because Gearbox now *relies* on those rules and a drift would silently skip a contract |
| `GBX0208` gear crate has no `gear.gdl` | **survives, and does real work** — with `--product`, a selected gear missing from the catalogue triggers a search for a crate declaring it; found means "here is the `gear.gdl` to write, and the `cargo(...)` line read from its manifest", not found means `GBX0301`. The tree has 44 gear attributes and 14 descriptions, so the found case is the common one |
| `GBX0209` declared lib ident does not match the crate | **survives** — `crate_name` and `lib` are the only declared facts with an external authority (`Cargo.toml`), checked at catalogue load for **every** `cargo(...)` in a description, not just `package` |

Two are added: `GBX0210` a description restates a projected fact, `GBX0211` the attribute scan found
zero or several candidates.

**One case is deliberately left unreported.** An *unmarked* trait name with `version = "v2"` or later
does compile: ADR-0007 makes an unmarked name unconstrained, because a v1 contract keeps its unmarked
name when v2 is added beside it. Reporting it would contradict the platform's own decision, and the
tree contains no instance — so it is recorded as a known gap and as an assertion in
`crates/gearbox-ir/tests/contract_shape.rs`, which is what will notice if the macro ever tightens it.

---

## 6. `product.lock`

`toml` 0.9 serialization of `ResolvedProduct`, arrays-of-tables in sorted order, two-line generated
header, no other comments. Written only by `gearbox-lock::write_canonical`.

```toml
# GENERATED by gearbox 0.1.0 — do not edit. Run `gearbox resolve` to regenerate.
schema_version = 1

[product]
id = "payments-demo"; version = "0.1.0"; profile = "local"
profile_kind = "self-hosted"; gearbox_version = "0.1.0"; lock_hash = "blake3:…"

[sources.gears-rust]
kind = "path"; location = "../../../gears-rust"; digest = "git:8f3c1a9b…"

[gears.payments-audit]
source = "gears-rust"
gdl_path = "gears/payments-audit/payments-audit/gear.gdl"
crate_name = "cf-gears-payments-audit"; lib_ident = "payments_audit"
crate_path = "gears/payments-audit/payments-audit"; link = ["payments_audit"]
runtime_caps = ["rest", "stateful"]; colocated_deps = ["cluster"]
selected_by = ["product.gdl:use_gear"]
# … one table per gear in the closure; `selected_by` records WHY it is here,
#   e.g. selected_by = ["colocated_deps:api-gateway"] for grpc-hub

[[applications]]
name = "gateway"; kind = "host"; anchor = "api-gateway"
gears = ["types-registry","authn-resolver","grpc-hub","api-gateway",
         "api-contracts","api-contracts-consumer","service-discovery"]
replicas = 1; entrypoint = "run_server"
bin_name = "gbx-gateway"; crate_name = "gbx-payments-demo-gateway"
rest_host = "api-gateway"; grpc_hub = "grpc-hub"; needs_db = false

[[applications.spawns]]                     # mirrors config/oop-example-master+follower.yaml
gear = "payments-audit"
executable_path = "../../../gears-rust/target/debug/gbx-payments-audit"
args = ["--config", "config/payments-audit.yaml"]

[[applications]]
name = "payments-audit"; kind = "worker"
anchor = "payments-audit"              # == OopRunOptions.gear_name, the directory identity
gears = ["cluster", "payments-audit"]  # `cluster` linked here, NOT in gateway
replicas = 1; entrypoint = "run_oop_with_options"; bin_name = "gbx-payments-audit"

[[binding]]
consumer = "payments-audit"; consumer_application = "payments-audit"
contract = "api-contracts/PaymentApi@v1"
provider = "api-contracts"; provider_application = "gateway"
mode = "remote"                        # DERIVED from placement, never configured
transport = "rest"; mechanism = "consumes-directory"
endpoint_source = "directory:service-discovery/api-contracts"
critical = false; selected = "explicit:remote/rest"

[[cluster]]
profile = "payments-audit"; primitive = "leader_election"
requesters = ["payments-audit"]; selected = "auto"
resolved = { via = "sdk-cas-default", over_cache = "postgres" }
diagnostics = ["GBX0504"]

[[cuttable_if_declared]]
consumer = "api-contracts-consumer"; provider = "payments-audit"
blocked_by = "undeclared-hub-edge"
suggested_edit = "#[toolkit::consumes(contract = payments_audit_sdk::PaymentsAuditApi, from = \"payments-audit\")]"
file = "examples/toolkit/api-contracts/api-contracts-consumer/src/gear.rs"
```

`dev` differs by having one application with all nine gears and all bindings `mode = "local"`.
`prod` adds `[product.kubernetes]`, per-application `image`/`subchart`/`service_port`,
`mechanism = "consumes-static"` with a `{{ .Release.Name }}`-templated endpoint, and GBX0603.

---

## 7. Generators

Every generator returns a `FileSet` of `FileEntry { path, bytes, kind, ownership }`.
`gearbox-engine::apply_generate` is the only writer: `Generated` → overwrite; `GeneratedOnce` →
write if absent; `OperatorOwned` → 3-way merge against a base cached in `.gearbox/<product>/.base/`,
conflict → GBX0701 and leave the file untouched. **`similar` has no three-way merge** -- it is a
diffing library, and the reconciliation is `gearbox-engine/src/generate/merge3.rs`, which uses
`similar` for the two line diffs and decides the policy itself. That policy never emits conflict
markers: a `values.yaml` containing `<<<<<<<` is a file `helm` can no longer parse, so a merge that
"succeeded" by writing markers would have destroyed the artefact it was protecting. Output root
`.gearbox/<product>/<profile>/`. **Composition output is never written into `gears-rust`.**

**Scaffolding a new gear or plugin reuses this writer and adds no vocabulary.** A new crate consists
entirely of absent files, so every file it writes is `GeneratedOnce` — which is why it can write into
a gear source root without violating ADR `cpt-gearbox-adr-macro-projected-catalogue`: the attribute
is written once, at creation, and the tool never returns to it. The single exception is one appended
`[workspace] members` entry, idempotent by content, which is the only case in the whole surface where
Gearbox modifies a file it did not create. ADR `cpt-gearbox-adr-authoring-ownership-tiers` records
the boundary and the survey behind it; PRD `cpt-gearbox-fr-scaffold-ownership` states it as a
requirement.

| Output | Mechanism | Golden reference |
|---|---|---|
| Generated workspace + `rust-toolchain.toml` | serde | `gears-rust/rust-toolchain.toml` |
| `apps/<p>/Cargo.toml` | serde | `examples/oop-gears/calculator/calculator/Cargo.toml` |
| Host `src/main.rs` | minijinja `{{ }}` | `examples/toolkit/users-info/users-info-server/src/main.rs` |
| Worker `src/main.rs` | minijinja | `examples/oop-gears/calculator/calculator/src/main.rs` |
| `src/registered_gears.rs` | minijinja | `apps/cf-gears-example-server/src/registered_gears.rs` |
| `config/<p>.yaml` (AppConfig) | **serde → `serde_saphyr`** | `config/oop-example-master+follower.yaml` |
| `docker/<p>.Dockerfile` + `build.sh` + `.dockerignore` | minijinja `{{ }}` | `gears/mini-chat/deploy/docker/mini-chat.Dockerfile` |
| Helm umbrella + subcharts | Chart/values via **serde**; template bodies via minijinja `<< >>` | `gears/mini-chat/deploy/helm/mini-chat/` |
| `product.lock`, `explain.json` | serde | — |

**Generated host `main.rs`** mirrors the users-info shape and adds the oracle flags
(`--list-gears`, `--dump-gears-config-yaml/json`) plus one new one:
`--list-registered-gears`, which prints `GearRegistry::discover_and_build()`'s real
inventory-discovered topo order and real `deps` as `<name>\t<dep>,<dep>` lines. This is the
strongest available verification oracle — diff it against the lock's `applications.gears`.

**Generated `registered_gears.rs`** emits one `use <ident> as _;` per `CargoRef.link` entry (so
nested plugin modules work) and **no `#[cfg(feature)]` gates** — the generated `Cargo.toml`'s
dependency set *is* the feature switch. Validation is therefore semantic (the oracle), not textual.

**Generated `Cargo.toml`** path-deps into `gears-rust` with relative paths. Those crates use
`edition.workspace = true`, resolved from their own workspace root because they physically live
there, so external path deps work unchanged. Set `CARGO_TARGET_DIR` to `gears-rust/target` — that
is what `self_hosted(target_dir = …)` is for.

**Generated AppConfig** writes `runtime: { type: oop, execution: {...} }` for worker anchors (the
key must exist — `build_oop_spawn_options` iterates `config.gears.keys()`), and deliberately writes
**no** `client_wiring` when a provider is co-located (absent ⇒ `ClientWiring::Local` is already
correct). The `leader_election` key is deliberately **omitted** from `cluster.profiles.default` —
that omission is what engages the SDK CAS default, and the lock records it as
`resolved = { via = "sdk-cas-default" }` so it reads as intentional.

**Helm.** Umbrella + one subchart per application with `enabled` flags. No `lookup`, no
`randAlphaNum`, no `genCA` — renders with zero cluster access. `values.generated.yaml` (Generated)
carries images/ports/replicas/wiring; `values.yaml` (OperatorOwned) is 3-way merged.
`values.schema.json` is composed programmatically in `SchemaGen` (top-level keys are per-application
and therefore dynamic): `schemars::schema_for!(EscapeHatches)` + `schema_for!(ExternalDatabase)`
merged under each application key, `additionalProperties: false`, `enum` for `pullPolicy`/`service.type`.
**Secrets: none in values** — Bitnami `existingSecret` + `existingSecretPasswordKey` only,
projected as `secretKeyRef` env consumed by the postgres plugin's `${PG_PASSWORD}` expansion.
Full escape-hatch set per subchart (`nameOverride`, `global.imageRegistry`, `podAnnotations`,
`nodeSelector`, `tolerations`, `affinity`, `resources`, `extraEnv`, `extraVolumes`,
`serviceAccount.*`, `podSecurityContext`, …). Probes from `oop_serve`: `/healthz`, `/readyz`.
Not generated: mini-chat's `lease.yaml`/`rbac.yaml` — those serve its private `k8s_lease` elector,
which is not a cluster provider.

**How the k8s profile works without a K8s-DNS resolver — the honest answer:**
- `discovery = "static"` (the default the prototype ships): write
  `gears.<consumer>.config.consumer_wiring.<provider> = "http://{{ .Release.Name }}-<subchart>:<port>"`
  into the consumer's **ConfigMap** (never env — `remap_gear_env_key` can't express it). The
  proxy-wiring phase installs `StaticEndpointResolver` and the REST resolving client hits the
  Service DNS name. **GBX0603** is emitted so nobody believes a DNS resolver exists.
- `discovery = "directory"` (opt-in): the gateway subchart runs `service-discovery` + `grpc-hub` on
  a fixed port; workers get `TOOLKIT_DIRECTORY_ENDPOINT` and `APP__OOP_HTTP__ADVERTISE_URI` from
  the downward API with `allow_loopback_advertise: false`. This works today; it is just more moving
  parts, so it isn't the default.

---

## 8. JSON-RPC surface

**Transport.** stdio, JSON-RPC 2.0, `Content-Length: <n>\r\n\r\n<utf8 json>`. Byte-compatible with
`vscode-jsonrpc`'s `StreamMessageReader`/`Writer`. All logging goes to **stderr** plus
`gearbox/log` notifications. Started as `gearbox rpc --stdio [--allow-writes] [--root <dir>]`.

**Lifecycle:** LSP-shaped `initialize` / `initialized` / `shutdown` / `exit`, so one connection
serves both the Studio and the `.gdl` language client. `initialize` result advertises
`{ textDocumentSync, diagnosticProvider, completionProvider, hoverProvider, documentSymbolProvider,
definitionProvider, gearbox: { catalogue, resolve, generate, explain } }`.

**Catalogue loading is staged over the wire, per §2.3.** `gearbox/catalogue/load` returns as soon as
S0/S1 are done — paths plus declared facts, with everything still unprojected listed under `pending`
— and the engine then sends `gearbox/catalogueChanged` as gears complete, carrying the gears that
moved from `pending` into the catalogue rather than the whole thing again. `$/progress` reports
completed against discovered. `$/cancelRequest` already applies to the scan; **cancelling leaves what
is already projected valid** rather than discarding it, so a cancelled load degrades to a smaller
catalogue and not to none. A response whose `pending` is non-empty is therefore normal, and a client
must not read absence of a field on a pending entry as absence of the fact.

**Methods:** `gearbox/catalogue/{load,get}`, `gearbox/product/{load,resolve,explain}`,
`gearbox/lock/{read,write,diff}`, `gearbox/generate/{plan,preview,apply}`, `gearbox/validate`,
`gearbox/graph`, `gearbox/watch/{start,stop}`.
`gearbox/generate/plan` returns `FilePlan[] = { path, action: create|update|unchanged|conflict,
ownership, kind, blake3, previewAvailable }`.
`gearbox/graph` returns a `GraphDto { nodes: {id, kind, label, group?, badges[]}[],
edges: {from, to, kind, label?, style: solid|dashed}[] }` for views `deps|contracts|applications|cluster`.

**LSP subset:** `textDocument/didOpen|didChange|didSave|didClose|completion|hover|documentSymbol|definition`
(jump from `use_gear("x")` / `from_ = "x"` to the declaring `gear.gdl`).

**Notifications:** `textDocument/publishDiagnostics`, `$/progress`, `gearbox/log`,
`gearbox/catalogueChanged`, `gearbox/lockChanged`.

**Application errors** in the `-32000` block, `data.diagnostics` always populated:
`NotInitialized`, `WorkspaceNotOpen`, `GdlEvalFailed`, `ResolveFailed`, `GenerateFailed`,
`WritesDisabled`, `PathOutsideWorkspace`, `Cancelled` (`$/cancelRequest` honoured — catalogue scan
and resolve are cancellable), `SourceUnavailable`.

**TS types.** `ts-rs` 11 with `#[derive(TS)]` on every public `gearbox-ir` type plus the RPC
envelopes, `#[ts(export, export_to = "../../ide/gearbox-studio/src/common/generated/")]`, driven by
`cargo test export_bindings`. Chosen over `specta` for having no runtime component and matching
serde's `BTreeMap`/`Option`/tagged-enum encoding. **Two anti-drift guards:**
`make ts && git diff --exit-code ide/.../generated` must be a no-op, and
`gearbox rpc-schema` (schemars) is diffed against `fixtures/rpc-schema.json` so a rename that
ts-rs tolerates still shows as a fixture diff.

---

## 9. Theia Studio

```
ide/
  package.json            # private, npm workspaces
  theia-version.txt       # ONE pinned exact @theia/* version
  gearbox-studio/         # native Theia extension: views + engine supervision
    src/common/{protocol.ts, generated/}      # ts-rs output, checked in
    src/browser/{gearbox-studio-frontend-module.ts, catalogue-store.ts, reveal-service.ts,
                 catalogue/, detail/, product/, graph/, explain/, lock/, generate/,
                 gdl/                         # .gdl language, contributed natively
                 theia/                       # one file per overridden Theia class
                 diagnostics/gearbox-marker-contribution.ts,
                 contribution.ts, commands.ts, menus.ts, keybindings.ts, preferences.ts}
    src/node/{gearbox-studio-backend-module.ts, gearbox-engine-process.ts,
              gearbox-service-impl.ts}
  browser-app/            # @theia/cli: theia build / theia start
  electron-app/           # + @theia/electron, electron-builder
  scripts/{rpc-smoke.mjs, ...}, tests/{conformance/, fixtures/, report/}
```

Two departures from the original sketch, both recorded in ADR
`cpt-gearbox-adr-domain-specific-ide-shell`. There is **no `gdl-language/` VS Code extension**: the
grammar is contributed natively from `src/browser/gdl/`, because the VSIX route needs
`@theia/plugin-ext` and the whole plugin host to colour one file type. And there is a
**`src/browser/theia/`** directory that mirrors Theia's own package layout, one file per overridden
class — the arrangement Arduino IDE uses, which puts an override at the path of the thing it
overrides.

**Package manager: npm workspaces.** No yarn is installed, and pnpm's non-hoisted `node_modules`
breaks Theia's plugin host and `@theia/cli` asset copying. `save-exact=true`.

**Theia version:** resolve `npm view @theia/core version` once, write it to `theia-version.txt`, and
use it verbatim for every `@theia/*` dep **plus root `overrides`** — a transitive `^` pulling a
second `@theia/core` copy breaks inversify identity and is the most common Theia build failure.
`engines: { node: ">=20 <21" }`.

**Extension points.** `theiaExtensions: [{ frontend: "lib/browser/gearbox-studio-frontend-module",
backend: "lib/node/gearbox-studio-backend-module" }]`.
Frontend: `AbstractViewContribution` + `WidgetFactory` per widget, `CommandContribution` /
`MenuContribution` / `KeybindingContribution` (`gearbox.resolve` = Ctrl+Alt+R, `generate.plan`,
`generate.apply`, `validate`, `explain.selection`, `engine.restart`),
`TabBarToolbarContribution`, `FrontendApplicationContribution`, `PreferenceContribution`
(`gearbox.enginePath`, `catalogueRoots`, `allowWrites` default false, `defaultProfile`),
`WebSocketConnectionProvider.createProxy<GearboxService>` + a `GearboxClient` callback for
notifications, and `ProblemManager` from `@theia/markers`.
Backend: `ConnectionContainerModule.create` + `RpcConnectionHandler<GearboxClient>` (one service
per frontend connection, one engine per workspace root) and `BackendApplicationContribution.onStop`.

**Widgets.**

| Widget | Shows |
|---|---|
| Catalogue | tree by `category` → gear, **with foldable categories and a filter over name, id, category and path**. `gears-rust` carries 62 crates with `#[toolkit::gear]` against the 14 described today, so the list quadruples as descriptions land; a flat list stops being readable well before that. A filter overrides a fold — a match hidden inside a collapsed category is the one thing a filter must never do, because the reader concludes the gear is absent. Badges for `runtime_caps`, chips for `colocated_deps`, provides/consumes counts. Click reveals the `gear.gdl` at its declaring range. A toggle per projected row adds the gear to the open product or takes it out, writing `products/…/product.gdl` after a preview and a confirmation. **This replaces «produces a *proposed* `use_gear(...)` diff, never an auto-edit», which this plan required until M5.** What changed is the reading of ADR-0010, not the appetite for writing: its tier 3 is «structured manifests | tool edits surgically | **Permitted**», and its survey calls manifest editing «the single most universal behaviour in the set». The tier-5 prohibition covers *human logic*, and a GDL description cannot be logic — `cpt-gearbox-fr-gdl-declarative` refuses every branching construct — so a `use_gear(...)` entry is a data entry in a list, exactly like the line `cargo add` writes. What survives from the old wording is the part that mattered: the diff is still shown first and nothing is written until it is accepted. The four refusals in front of the write are in §9.2. **Renders incrementally** (§2.3): the grouping is available at S1 because `category` is declared, while the badges arrive at S2 because `runtime_caps` and `colocated_deps` are projected — so the tree's shape settles first and fills in. Rows are keyed by `gdl_path`, not `id`, because the id does not exist until S2. A `pending` row renders dimmed, and **clicking it still reveals its `gear.gdl`** — that path is known from S0, so a pending row is never inert. |
| Inspector | **one panel, two sections, one selection.** *What it is*: everything projected for the selected gear -- capabilities, co-location, extension points with the vendor the host selects on, what the gear fills and under which vendor, contracts with the transports **this provider wires up**, GTS types, and clickable PRD/DESIGN/ADR links. *Why it is here*: the resolver's `because` sentence per edge, nearest reason first, each linking to its `origin`, with `DowngradedBy` steps marked. This was two panels -- `Gear detail` keyed off a catalogue row and `Explain` off a product focus -- which meant the ordinary act of clicking a gear in the product tree filled one and left the other asking to be given a catalogue selection. In the bottom area, not the side panel, because the side panel clipped exactly the facts it exists to show. |
| Product | **a tree, as vision §60 sketches it**: foldable branches for Gears (with `asked for` / `pulled in by the closure` beneath), Applications, Contracts and Cluster, each with an icon and a count. The profile switch, the resolved profile and the description-file link stay in the header rather than becoming a Deployment branch: the switch has to be reachable *while* a resolution is in flight, which a branch of the resolved product cannot be. §60's Security is absent — it is not modelled in the IR. Artifacts live in the Generate view, which exists now that `capabilities.generate` is `true`. Bindings carry mode/transport/mechanism chips; cluster shows `selected` vs `resolved`; diagnostics summarise at the bottom. |
| Graph | four views. **deps** (solid = co-location), **contracts** (dashed = cuttable, solid = forced local, red = undeclared-hub-edge), **applications** (boxes with gear chips, overlapping gears drawn in *every* box — this is what makes closure-not-partition visible), **cluster** (requirement → capability → provider, unsatisfied in red). Layout: `elkjs` `layered` with a fixed seed → deterministic, so screenshots and "why did this move" are stable. Rendered as hand-written React SVG. |
| Conflicts | the resolution's diagnostics as a domain screen, not a list under a tree: code, message, the `help` sentence that says what to do, the location and every `related` location as links, the `evidence` `file:line` in `gears-rust` where a claim asserts a runtime limitation, and -- where the engine names a `subject` -- a link that points the Inspector at the thing being complained about. `Resolve again` means re-resolve after an edit; there is no automatic resolver and none is promised. A second consumer of `ProductStore.diagnostics`, beside the Problems markers, so the two cannot disagree. The Product view keeps a one-line summary that opens this. |
| Start | what there is to do with no product open: **New Product…**, **Clone**, `Open Product…`, the products found in the workspace, and the ones opened before. New Gear / Open Gear appear only when the tier-0 scaffold exists; until then Home stays two honest product actions rather than a fake fourth button. Replaces an empty main area, which read as an application that had failed to load rather than as a tool waiting to be told what to work on. |
| Lock | read-only Monaco view of canonical `product.lock`, diff toggle vs disk, `lock_hash` badge that goes stale-yellow when resolve ≠ disk. |
| Generate | `FilePlan[]` as a directory tree with create/update/unchanged/conflict icons, per-file Monaco diff preview, ownership badge, Apply disabled unless `allowWrites` and no Errors and no conflicts. |

**Which widget belongs where is decided by the working context, not by a perspective the reader
switches.** Home places Start in main and the catalogue left; Product places the Product view in main
and collapses the left panel, because the catalogue is a source of components there rather than the
subject. The Inspector, Conflicts, Lock, Generate and the Graph are one command away and are remembered
per context once opened -- belonging is the saved layout, not a forced open on every switch. The
reasoning, and why the two switchable perspectives were a mistake, is in §9.1 and in ADR-0011's
amendment of 2026-08-31.

**Engine supervision** (`src/node/gearbox-engine-process.ts`): one engine per workspace root;
`child_process.spawn(enginePath, ['rpc','--stdio','--root',root,…])`;
`createMessageConnection(new StreamMessageReader(child.stdout), new StreamMessageWriter(child.stdin))`
from `vscode-jsonrpc/node`; `child.stderr` piped line-by-line into a "Gearbox Engine"
`OutputChannel`. Exponential-backoff restart (250 ms → 4 s, 5 attempts, reset after 60 s healthy),
then a `MessageService.error` with a Restart action. `onStop` sends `shutdown` + `exit`, waits 2 s,
then `SIGKILL`. Version guard comparing `serverInfo.version` to the extension's, since the ts-rs
types are compiled against a specific engine.

**Diagnostics → markers, two deliberate paths.** File-anchored GDL/validate diagnostics
(GBX01xx/02xx) arrive over `textDocument/publishDiagnostics` from the `gdl-language` LSP client and
Monaco renders them natively — zero code in `gearbox-studio`. Resolve/generate diagnostics
(GBX03xx–07xx) come back in the resolve result and are mapped by `GearboxMarkerContribution` to
`ProblemManager.setMarkers(uri, 'gearbox', markers)` — a single owner string, so a re-resolve
replaces the whole set atomically and stale markers can't accumulate. Diagnostics with no location
anchor to `product.gdl` at 0:0 with the evidence in `relatedInformation`. `data.help` becomes a
quick-fix-shaped code action where the remedy is mechanical (add a `#[toolkit::consumes]` line, add
a gear to `colocated_deps`, switch a cluster provider).

**Narrowing the shell** (ADR `cpt-gearbox-adr-domain-specific-ide-shell`). Two levers, in order.
First the dependency set: a menu that no package contributes cannot appear, which is why `Selection`
and `Go` are removable by dropping nothing more than the packages that own them — except that both
are owned by packages the editor needs, which is exactly why the second lever exists. Second,
override what remains: `super.registerMenus()` then `unregisterMenuAction(id, path)` for an entry,
removal by node id for a whole top-level menu, `initializeLayout(): NOOP` to hide a view without
losing the command that opens it, and `rebind(TheiaX).to(MyX)` — **every one carrying a comment
saying why**, because a suppression without a reason is indistinguishable from an accident later.
`FilterContribution` exists in 1.75 and is held in reserve: it removes a contribution class
wholesale and cannot strip a single menu item.

Two things come first, before any individual view. A `Contribution` base class implementing the five
contribution interfaces with the common services pre-injected, plus a one-line `configure` binder, so
a feature costs a file rather than six lines of shared boilerplate. And a toolbar: a plain widget in
the shell's `top` area, added the same way the menu is (`shell.addWidget(..., { area: "top" })`).
The browser target does not hide that panel — `setTopPanelVisibility` only collapses it when
`window.menuBarVisibility` is `compact` or `hidden`, and we have a menu — so there is nothing to
override. Actions are scoped to the active perspective, not to a widget: the toolbar is a shell
panel and has no "own widget" for `TabBarToolbarRegistry.isVisible` to key on.

**Kept visible on purpose**, diverging from Arduino IDE, which hides more: the editor, Explorer,
Terminal and Git. The workflow ends in generated crates a person will want to build, inspect and
diff, and `.gearbox/<product>/` is a directory like any other.

**Two working contexts**, Home and Product, derived from what is open rather than switched by hand,
and laid out by Theia 1.75's `PerspectiveService` (`PerspectiveContribution`, not a homemade
`createLayout`). `StudioContextService` owns the context and drives the layout, the context keys the
menus read, and the header -- one direction only, because a perspective can be restored from a saved
layout with no domain object behind it, and a `Product` menu keyed off that would offer verbs for a
subject that is not there. A third context, `gear`, is in the type and deliberately unreachable until
gear authoring exists. This replaced two *switchable* perspectives with toolbar buttons whose words
duplicated two menu entries while meaning something else; §9.1 records why that was wrong.

**`.gdl` language** is contributed **natively** — `LanguageGrammarDefinitionContribution` plus
`TextmateRegistry` from `@theia/monaco`, in `src/browser/gdl/` — not as a bundled VS Code extension.
The VSIX route would require `@theia/plugin-ext` and the whole plugin host to colour one file type in
an application whose users do not install extensions. Note that `--plugins=local-dir:../plugins`,
which `browser-app` still passes, currently does nothing: the package is absent and the backend's
argument parser is not strict, so the flag is accepted and ignored.

The grammar's regex craft is hand-written; its vocabulary is **generated from the same globals the
interpreter evaluates against** (`crates/gearbox-gdl/tests/export_grammar.rs` →
`src/browser/gdl/generated/vocabulary.ts`), with `make grammar-check` failing the build on drift
exactly as `make ts-check` does for the wire types. A grammar is a second place where the language is
written down, and this is what stops the two disagreeing. Two traps worth stating: a grammar needs
both `monaco.languages.register({ id })` and `mapLanguageIdToTextmateGrammar`, and with only the
second there is no highlighting and no error; and the forbidden-keyword set is **14 tokens**
(`crates/gearbox-gdl/src/declarative.rs`), not the five this plan once named — `while` is not among
them, because it is refused at parse as `GdlParse` rather than as GBX0103.

Completion is deferred with the rest of the LSP surface, and its rule is already fixed: only for
fields GDL still owns. `runtime_caps`, `colocated_deps` and `cluster_providers` are projected, so
offering completions for them would invite the restatement the surface rejects. Inside `from_ = "`
catalogue gear ids; inside `profile = "` the profiles the gear's crate actually implements (which is
the join key, so completing it prevents a GBX0508 rather than reporting one); inside
`capabilities = [` the capabilities the requirement's primitive admits; inside
`cluster_plugin(backend = "` the backend impls found in that plugin crate.

---

### 9.1 What is built, and where the implementation diverged from this plan

Eight widgets exist against the real engine: **Catalogue** (tree by category, staged), **Inspector**
(what a thing is, beside why it is here), **Graph** (all four views), **Product**, **Conflicts**,
**Lock**, **Generate** and **Start**.
`capabilities.generate` is `true` now that M5 is on the wire; the view appeared the same way the
resolver notice disappeared -- driven from the engine's own capability, not from a hard-coded
string. Docker and Helm land for a Kubernetes profile (M7): a Dockerfile per application, an umbrella
chart with one subchart per application, `values.yaml` as `OperatorOwned`, and `values.schema.json`
with `additionalProperties: false`. Their absence on `dev`/`local` is still a smaller output set
rather than a flag — `generate: false` would hide a panel that answers correctly for everything it
does cover.
There used to be a `skipped` list on a generate plan for the worker entry points; M6 removed it,
because nothing is skipped any more.

Also built since this section was written, and not planned here: the `Contribution` base class ADR
0011 asks for; the menu narrowing that removes Selection and Go and fills a Gearbox submenu;
resolution diagnostics as Problems markers, which is what finally uses the long-declared
`@theia/markers`; nine RPC methods -- `product/load`, `resolve`, `validate`, `product/lock`,
`product/addGear`, `product/removeGear`, `generate/plan`, `generate/apply` and `generate/file`; and
the description edit the add/remove pair carry, which reverses a prohibition this plan used to
state (§9.2); and the two perspectives plus the toolbar, carried by Theia 1.75's
`PerspectiveService` rather than by `createLayout` or a homemade switch. Catalogue places the
catalogue and gear-detail; Product places the product and Explain. Lock, Generate and Graph stay
off the maps so a switch does not open panels nobody asked for. `primaryViews` is first-activation
only; each descriptor's `onActivate` raises the primary widget after a snapshot restore so an
editor left in main does not hide Product.

**Two write gates, and they are not the same.** `writable_path` is for description edits: the path
must exist, must be `.gdl`, and must sit inside the declared workspace or a source root -- "this
method edits descriptions only". `writable_out_root` is for generation: the path must sit inside
the workspace, must **not** sit inside any source root (writing generated Rust next to human Rust
is ADR-0010 tier 5), and may not exist yet -- the nearest existing ancestor is what gets checked,
or the first generate would always refuse itself. Apply without `allow_writes` is a third refusal
(`WRITES_NOT_ALLOWED`), and a resolution that reported errors is a fourth (`GENERATE_REFUSED`);
the client is not the security boundary (`cpt-gearbox-fr-rpc-writes-opt-in`).

**Contents arrive on request, not with the plan.** `FilePlan` is one preview line. The real set
includes `Cargo.lock` at around a hundred kilobytes, and a diff needs both sides of one file, so
`gearbox/generate/file` re-runs generation and picks one entry. Stateless, like everything else
on this server. A cache of the last plan would need a staleness check before apply; that is why
there is none yet.

**The check is `npm run conformance`, and it is organised by document rather than by feature.**
`ide/tests/conformance/` holds one test per claim in the PRD, the ADRs and §9 of this plan, each
named for the claim and carrying its source; `test.fixme` marks a documented claim that is not
implemented. The run writes `docs/conformance.md`, which is the authoritative version of this
section. A hand-maintained count in prose is what drifted here in five places, so it is not
maintained by hand any more. What the table cannot say: there is no Electron shell; M6 generates its
worker processes but has not been run live; M9 is not started. The toolbar and the two perspectives ADR-0011 asked for are built: Theia 1.75's
`PerspectiveService` carries them, the switch sits in `.gbx-toolbar` beside the menu, and the
browser target never needed a `hideTopPanel` override.

The staged-loading tests are deliberately a *timeline* rather than a final state: a snapshot taken
after loading would pass even if the tree had appeared all at once, which is exactly the claim
ADR-0009 makes and could not previously check. On the real tree the sampler catches a window in
which all 14 rows are on screen, named and grouped, and still `pending` -- so the staged design is
not merely implemented, it is visible.

**ADR-0009 survived contact with a consumer**, with one thing learned: `gdl_path` turned out to be
load-bearing beyond keying rows. The *selection* is keyed by it too, so choosing a gear before it
is parsed does not lose the choice when it finishes. An id-keyed selection could not do that.

Divergences from what §9 planned, each for a reason found while building:

| Planned | Built | Why |
|---|---|---|
| `elkjs` `layered` with a fixed seed | hand-rolled layered assignment + two barycentre sweeps | Deterministic by construction rather than by seed, and no async layout pass. The graph is a shallow DAG of 14 nodes. If it grows a cycle or a hundred nodes, `elkjs` is the answer. |
| detail as part of the Catalogue widget | its own widget in the **bottom** area | In a 300px side panel the projected facts -- provider transports, which point a plugin fills and under which vendor, GTS types -- were clipped. The tree answers "what is there"; the detail answers "what is it", and they need different amounts of room. |
| `@theia/{core,editor,filesystem,markers,monaco,navigator,process,workspace}` | plus `@theia/{preferences,userstorage,variable-resolver,messages}` | Without `@theia/preferences` the frontend dies on `No matching bindings found for serviceIdentifier: Symbol(PreferenceProvider) - named "1"` -- the user-scope provider. Without `@theia/messages`, `MessageService` still resolves and every message goes nowhere, which is worse than an error: it makes reporting a failure look like handling it. |
| Git from `@theia/git` | the VS Code `vscode.git` extension in the plugin host, plus a multi-root workspace Studio opens for itself | `@theia/git` has not been released since `1.61.0-next.8`, so there is nothing to depend on. The extension needs a workspace to find repositories, and the two that matter are siblings -- so the workspace is multi-root and derived from the roots the engine already reports. Writing an `ScmProvider` over `simple-git` was costed first: roughly a thousand lines for a subset with no history, blame, conflict resolution or gutter diffs. |
| Explain calls `gearbox/product/explain` | the explanation arrives with the resolution | `ResolveResult` already carries the whole `ExplanationGraph`, so "why" costs no second round trip -- and, more usefully, cannot answer about a different resolution than the one on screen. A separate call would have to be told which resolution it was about, or guess. |
| Lock as a read-only Monaco view | a `<pre>`, plus a rebind that makes any `product.lock` read-only *as a file* | No TOML grammar is installed -- the application has no plugin host, which ADR 0011 decided deliberately -- so Monaco would render the same uncoloured text behind a much larger component. What it would add here is a scrollbar. The editor half of `cpt-gearbox-fr-lock-read-only` is met where it matters, by rebinding `MonacoEditorProvider`; `FileService.getReadOnlyMessage` could not do it, because it resolves per URI *scheme* rather than per file. |
| Lock diffs against the lock on disk | not built, and the blocker is structural | Nothing on the wire says where the lock was written: `.gearbox/<product>/<profile>/` is `OUTPUT_DIR` in `crates/gearbox-cli/src/generate.rs`, a CLI default rather than an engine fact. Reporting it from the RPC would put the layout in a third place. The stale badge §9 pairs with this *is* built, for the comparison that is available -- the lock text's own hash against the resolution's. |
| `.gdl` as a bundled VS Code extension in `ide/gdl-language/`, declared via `theiaPlugins`, with an `extension.ts` starting a `LanguageClient` | a native `LanguageGrammarDefinitionContribution` in `gearbox-studio`, whose vocabulary is **generated** from the engine's own globals | The plugin path is not available: `@theia/plugin-ext` is not installed and `ide/plugins/` does not exist, so the `--plugins=local-dir:../plugins` flag in `browser-app/package.json` is inert. Adding a plugin host to ship one grammar is a large dependency for a small feature, and there is no `LanguageClient` to start -- the engine's JSON-RPC surface is LSP-*shaped* but has no `textDocument/*`. The native path also buys something the plugin could not: `cargo test -p gearbox-gdl --test export_grammar` derives the word lists from `gear_vocabulary()` / `product_vocabulary()` and `keyword_verdicts()`, so the editor cannot colour a function the engine does not have. §9 also listed the forbidden keywords as `if/for/def/lambda/while`; the real set is wider (`and`, `or`, `not`, `in`, `elif`, `else`, `break`, `continue`, `return`, `pass`), and `while` is not in it at all -- the lexer folds it into a single `Token::Reserved` variant, so it is refused as a parse error rather than as GBX0103. It still renders red, from the second of the two generated lists. |

Three failure modes worth writing down, because all three *looked* fine:

- **`FrontendApplicationContribution.onStart` opens a view too early.** It runs before the shell is
  attached, so layout setup left the panel collapsed -- and a collapsed Theia side panel still keeps
  its widget in the DOM. The tree was queryable and invisible, and the first version of the UI check
  passed against a blank screen. `initializeLayout` is the correct hook, and it also only runs when
  there is no saved layout, so a person who closes the panel does not get it forced back open.
  The check now asserts `getClientRects().length > 0`, not node count.
- **A two-way RPC proxy plus a store that injects the service is a DI cycle.** inversify reports it
  as "circular dependency in one of the `toDynamicValue` bindings". One of the two edges has to be
  deferred; the client edge is the safe one, reached through a forwarder, because no notification
  can arrive before the store has asked for the service and started a load.
- **Theia's command palette is `.quick-input-widget`, without the `monaco-` prefix.** A selector
  that never matches is worse than no wait at all: with the timeout swallowed, the step passed or
  failed on timing. Keypresses sent while Theia is still installing its keybindings are simply lost,
  so the check presses F1 until the palette answers.

Two more failure modes surfaced once the app was actually used, both the same
shape as the three above -- something that looked fine and failed in silence:

- **The `gear.gdl` and docs links were dead.** Every path the catalogue carries
  is relative to its source root, and the root is the one thing only the engine
  knows -- `SourceDecl::location` holds the location *as the operator wrote it*,
  because it goes into `product.lock` and a lock carrying `/Users/someone/...`
  would not survive being committed. So the relative path went into
  `new URI(...)`, produced a URI with no scheme, and no opener claimed it.
  `initialize` now reports `roots: [{ id, path }]` -- an RPC fact, deliberately
  not an IR one, since the server and its client are on the same machine by
  construction while the lock has to stay portable.
- **The failure was swallowed twice.** First by an empty `catch` whose comment
  assumed the file was outside the workspace -- an assumption never checked.
  Then, after the first fix reported it through `MessageService`, by the absence
  of `@theia/messages`: the service resolves without the package, and every
  message goes nowhere. Reporting a failure looked like handling it.

The lesson for the UI check is the general one: asserting that a link *renders*
proves nothing, because the broken link rendered perfectly. It now asserts that
**a tab opens**, for `gear.gdl` and for a docs link.

A third trap, not in the UI at all: `npm run build` does not rebuild the engine.
The backend spawns `../target/debug/gearbox`, so the Rust half stays stale and
the symptom is a client reporting something the engine was already taught to
send. `npm run verify` builds it first.

**P0 stray-write hunt (2026-09-01).** A trap logged every `Gearbox: writing`
`console.info` with a stack (`ide/tests/fixtures/studio.ts`,
`ide/tests/global-teardown.ts`). Five full Playwright passes after closing the
suspected mechanism left `git status --porcelain products/` clean each time.
**After closing that mechanism, the stray write was not reproduced in five full
runs** — weak negative evidence, not a proof that no path exists.

**Product authoring (2026-09-01).** The GDL editor generalised from `gears` list
membership to any named argument inside a literal list on `product(...)` — see
ADR `cpt-gearbox-adr-create-product` and the ADR-0010 amendment of the same
date. Studio now offers **New Product…** and **Clone** on the Start screen,
config/features on the Inspector for gears the product asks for, and profile
add/remove/scalar edit on the Product view. RPC: `setConfig`, `setFeatures`,
`addProfile`, `removeProfile`, `setProfileField`, `create`.

**Explain origins (2026-09-02).** `ExplanationNode.origin` and `ExplanationNode::at`
had been on the wire with **no callers**, the third case of an IR field that
nothing filled — after `Diagnostic::subject` / `Diagnostic::about` (§9.1 above)
and the same shape of gap. GDL now records `declared_at` from
`Evaluator::call_stack_top_location` on `use_gear` / `bind` / profile
constructors / `gear(...)`; the graph builder threads those into origins.
Selected gears link into `product.gdl`; colocated gears link into their
`gear.gdl`. The lock is unchanged: it stores provenance *edges* only.

**Studio sessions and session-trust (2026-09-02).** The shell model is three working
contexts — **Home**, **Product**, and **Standalone Gear** — derived from what is
actually open (`StudioContextService`), not from a toolbar that rearranges layouts.
Gear stays reserved until New Gear (ADR-0010 tier 0) exists; Home must not show
New/Open Gear before that. Urgent work is Product UX trust: Product `onActivate`
opens the Product view (not `activateWidget` alone), the Inspector opens from
selection, disconnected/engine-down disables New Product / Resolve / Generate and
offers Retry, toolbar Generate is labelled `Generate`, and conformance prefers
visible Start/header buttons over palette-only paths. Catalogue on Home is Browse,
secondary to Start. See ADR-0011 / ADR-0013 amendments of the same date.

The favicon gap this section used to record is closed. `@theia/cli` 1.75 still offers no hook and
its generated `index.html` still has no `<link rel="icon">`, so `FabricThemeContribution` injects
one from `onStart` -- the same contribution that registers the Constructor Fabric colour themes and
the Geist fonts. The UI check still tolerates a favicon 404 **by name**, which now only matters if
that injection regresses.

Two themes since 2026-09-16, not one: `Gearbox (Fabric Light)` is the default and
`Gearbox (Fabric)` stays in the Color Theme picker. The same brand tokens read the other way round
-- navy carries the text rather than the background -- which is why no stylesheet changed: the
sheet is entirely `var(--theia-*)`, so a rule that needs a different value under a different
background gets it from the palette. One favicon serves both; it is a mark, not a palette. There is
no ADR on the theme, and the light default is recorded here and in the registration's own
comments.


#### The graph's four views, and what the corpus can and cannot show

The Graph panel hosts the four views §9 asks for behind a tab strip, rather than four widgets:
they share a coordinate system, a set of arrowhead markers and the layered layout, and four
registrations would have added four more entries to a Gearbox menu that had just been cleared of
duplicates. The widget id changed from `gearbox.graph.deps` to `gearbox.graph` with the rename.

The views split by where their data comes from, and that split is visible in the interface.
**Co-location** reads the catalogue and needs no product, because a `deps` edge is a declared fact
that no resolution changes. **Contracts, applications and cluster** read a resolution: they are answers
about one profile, so with no product open each says so and says how to get one, rather than
rendering an empty frame that is indistinguishable from a broken view.

Three things the implementation learned from the data, none of them in §9:

* **The interesting profile is `prod`, not the default `dev`.** On `dev` the demo product resolves
  to two local bindings and one application of nine gears -- every view would render
  truthfully and show nothing that could have made it wrong. So the conformance tests for these
  views resolve `prod`, where the same description severs a contract edge and splits into three
  applications.
* **A contract edge merges per gear pair, and that loses nothing.** `ResolvedBinding.mode` is
  "derived from placement, never configured", so two gears are either in one process or in two and
  every binding between them agrees about `mode`. `PaymentApi@v1` and `@v2` therefore travel one
  arrow, labelled with both. The demonstration §9 was reaching for -- one pair carrying a solid and
  a dashed edge at once -- **cannot exist**, and the honest substitute is stronger: the *same* edge
  is solid in `dev` and dashed in `prod`, which is what makes a contract edge a resolver decision
  rather than a declared fact.
* **Severability is looked up, never inferred.** The three edge states come from `mode` and from the
  engine's own `cuttable_if_declared`; note that `CutCandidate` is "an edge the resolver would sever
  but cannot", so its entries are forced *local* and red marks the one blocker source can remove.
  No `BindingMechanism` literal appears in the frontend, which
  `prd-studio.spec.ts` enforces -- rendering a mechanism is consuming a projected fact, while
  branching on one would be taking over a resolver decision.

One claim stays honest rather than green, and it is a corpus limit. The other was the cluster
view, and it is now observed: `api-contracts-consumer` requires the `event-broker` scope, so a
`ResolvedClusterBinding` reaches the view in every profile. The empty state it used to show is
still reachable and still asserted -- by a product that requires nothing.

* **Applications do not overlap on this corpus, in any profile.** `prod`'s extra anchors declare no
  `deps`, so their closures are singletons and its three boxes hold 6 + 1 + 1 of the same eight
  gears `dev` puts in one. The view states this in words instead of letting an absent repeated chip
  imply that a partition is what the model produces.

#### The Lock view against the lock on disk

Built, and **structured rather than textual** -- §9 asked for a "diff toggle vs disk", which sounds
like two columns of text. `gearbox_lock::diff` already compared two *parsed* locks and its
`summary()` doc comment names this widget as its consumer, so the view renders the engine's own
`+`/`-`/`~` lines: "`~ profile: prod -> dev`" rather than "line 214 differs". Computing that in the
client would be a second answer to the one question a lock settles, so nothing is computed there --
`LockDiff` is not even on the wire, only its sentences. The bytes on disk come too, behind a toggle,
for a reader who wants to see them.

`LockResult` carries the comparison rather than a second method answering it, for the same reason
`ResolveResult` carries its explanation: the two have to be about one resolution, and a separate call
cannot promise that. `LockParams` gained an `out` override so a test can point at a lock of its own;
the Studio sends none.

**Four states, because each is a different thing to do about it.** No lock yet (generate one); one
that matches (nothing to do); one that differs (regenerate, or find out why); and a file that
**does not verify** -- `gearbox_lock::read` recomputes the hash and refuses on a mismatch, so a lock
is self-verifying, and reporting an empty diff for a tampered file is what comparing text would do
and would say the opposite of the truth. The path is shown in every state, because "nothing on disk"
and "I looked somewhere else" are indistinguishable without it.

**One limitation, stated because it is a design decision and not an oversight.** The comparison is
taken when the lock is fetched, which is once per resolution. `.gearbox/**` is excluded from Theia's
file watcher on purpose -- generated output is rewritten wholesale and watching it reports churn
nobody acts on -- so a `gearbox generate` run in a terminal is not noticed. Hence a `re-read`
control in every state, and an automatic refresh after the Generate view applies. The comment in
`ProductStore.ensureLock` that said a resolve was "the only thing that could change the answer" was
true until this landed and is now corrected in place.


#### A product session decides where the engine looks and where it may write

A product is a directory and the unit of work, so opening one is not "point a panel at a file". Until
this landed the backend fixed both halves of that -- `../gears-rust` as the only source root, the
repository as the write boundary -- so a product anywhere else could be *read* and then refused on
every edit and every generate, because `writable_path` and `writable_out_root` both measure from the
declared workspace. `StudioSession { roots, workspace }` is what a client declares, and
`GearboxService.initialize()` takes it; the wire already carried all three fields.

**The order is forced, and one step of it is not obvious.** Source roots come from the product's own
`sources`, and reading those means evaluating the description, which needs a running engine. So the
engine starts twice per open: once with the product's directory as the workspace and no roots -- which
is enough, because `load_product` evaluates without joining a catalogue -- and again with the derived
roots. Two spawns is the price of deriving roots from the thing being opened rather than from a
constant, and it is the cost `Reload Catalogue` already pays.

`git(...)` sources are refused by name rather than skipped: fetching a repository is not built, and a
catalogue quietly missing a source is indistinguishable from a product whose gears do not exist.

Auto-open moved out of `ProductStore.discover()`, which had been opening a product *without*
re-initializing the engine -- so the catalogue kept whatever roots the previous session left. Listing
and opening are now separate, and "open it if it is the only one" lives with opening.

#### Open, Close, Recent -- and why Close stays closed

`File` carries the verbs that decide *what* is being worked on: `Open Product…`, `Close Product`, and
the Recent entries the picker offers. Under `File` rather than `Gearbox`, which holds the verbs that
act on what is already open. **New Product…** and **Clone** live on the Start screen (and the create
wizard); Home does not grow New Gear / Open Gear until the tier-0 scaffold exists.

**Close refuses rather than asking.** If the description has unsaved changes it says so and does not
close, which is the position `ProductEditService` already takes for a write: it will not touch a dirty
buffer, and it will not save on the author's behalf either, because that commits an edit they had not
finished. The full Save / Close without saving / Cancel set is a later choice, not a missing one.

Closing clears everything derived from the product, not just the reference -- resolution, lock,
diagnostics, selection, profile -- and bumps the store's epoch first, because a resolve begun before
the close would otherwise call `update` afterwards and put the product back.

**A close stays closed.** `ensureOpen` runs when the Product widget is constructed, not every time it
is shown, so revealing the panel again offers the picker instead of reopening what was just closed. A
panel that reopened it would make Close look broken. That is also why the Recent claim drives the
picker rather than the reveal helper.

Recent is written **only after a successful open** -- the difference between a Recent list and a list
of things once attempted -- keyed on the path so one product reached two ways is one entry, and an
entry that fails to open is dropped as it fails, with a message, rather than opening an empty panel.

**Two name collisions this stage, both mine, and the second was a fragility all along.** Adding a
helper to the test fixture without checking whether one existed produced a duplicate declaration that
broke the whole file. And adding `Gearbox: Open Product…` made it rank *first* in the command palette
for the text `Gearbox Product`, above `View: Toggle Gearbox Product` -- so `runCommand` pressed Enter
on the wrong command, opened a second quick-pick, and the caller waited sixty seconds for a panel
nobody had asked for. First-match-wins was never safe: every command added to this application could
shift the ranking under every test that reveals a view. `runCommand` now clicks an entry that
*contains* the label, and the measurement is in the comment, because equality would not do -- the
palette renders `Gearbox Product` inside `View: Toggle …`.

Not claimed in §9 yet: closing, Recent and the picker are asserted in `regression.spec.ts` because §9
still describes the shell it had before this rework. Rewriting that table is the remaining documentation
work of this stage, along with the multiple-source-roots decision that deriving roots from a product
has made live.


#### Three guards on the descriptions, because two were not enough

A stray `use_gear(...)` has reached `products/payments-demo/product.gdl` three times now. The gear is
not random: twice `types-registry`, once `grpc-hub` -- the second and third rows of the catalogue as
rendered. Something reaches a *particular row*.

The third occurrence exposed a hole in the guard built for the second. A per-test hook checks after a
test **ends**, so a write that lands during teardown, or after the final hook, is invisible to it: the
run reported no failures and a clean tree, and the write surfaced as a refusal to start the *next*
run -- a different day, a different file, no connection to the cause. So there are three checks now,
and between them no path stays quiet:

* `global-setup` refuses to start against a modified description;
* the fixture's hook **fails the test** that dirtied one, with the diff naming the gear;
* `global-teardown` fails the **run** for a write that lands after the last test, saying explicitly
  that no test is blamed because none was running.

The cause is still not found, and the guards are not a substitute for finding it. What the third
occurrence added is a direction: the next attempt should watch the catalogue's focus and its queued
actions as the page tears down, rather than arm a stack trace on the write path -- which has now been
tried and caught nothing across four runs.


#### Two working contexts, and a whitelist instead of exceptions

The shell was Theia with Gearbox panels bolted on, and it showed: `Catalogue` and `Product` buttons
beside the menu bar repeating two Gearbox menu entries while meaning something else, `Type Hierarchy`
in the bottom panel, `Workspace` and `Terminal` as top-level concerns. ADR-0011 had left the door
open for exactly this correction -- "If the two perspectives turn out to be one... collapse them" --
and the condition was met.

**A context is not a perspective.** `StudioContextService` owns what Studio is working on and derives
it from what is actually open; `PerspectiveService` only arranges panels. The direction is one-way,
and it has to be: a perspective can be restored from a saved snapshot with no product behind it, so a
Product menu keyed off `activePerspectiveId` would offer actions with nothing to act on. The contexts
are named after the PRD's actors -- integrator, gear author -- not after the application's own panels.
`gear` is declared in the type although nothing enters it yet, so every `switch` is already exhaustive
and the compiler finds the branches to add rather than leaving them to a wrong default.

The toolbar became the header it should always have been: the product, its profile, and whether it
resolved. The profile is *shown*, not switched -- §9 keeps that control in the Product panel because
it must be reachable while a resolution is in flight, and a row rendered from the resolved product
cannot be. This is the line Arduino draws by putting the selected board in both its toolbar and its
menu: current state may repeat, navigation may not.

**The menu is a whitelist, and it has to be.** The old version removed three top-level menus by id
and left the rest as a general editor built them. `typehierarchy`, `callhierarchy`, `notebook`,
`timeline`, `bulk-edit`, `console` and `outline-view` all arrive through `@theia/plugin-ext`, which is
present so the VS Code git extension can run -- so half of ADR-0011's strategy, narrowing by
dependency set, cannot reach them at all. `ShellPolicy` prunes the bar to `ALLOWED_TOP_LEVEL`, removes
forbidden families from **every** menu including the editor's context menu, and re-prunes on
`onDidChange`, because plugin contributions arrive after startup and a one-shot prune is correct only
until the first extension activates. The menu bar is now asserted by *equality*, so anything an
upgrade adds fails a claim instead of appearing unannounced.

Three mistakes in this work are worth keeping, because each was found by a test rather than by
reading:

* A `protected key = this.contextKeys.createKey(...)` **field initializer** runs in the constructor,
  before inversify has injected anything, so it threw on `undefined` and took the container down. The
  symptom was the catalogue never settling and ninety seconds of nothing -- no hint that a context key
  was to blame. The comment warning about it was written before the code that ignored it.
* The header reused `data-profile`, which the Product view already owns and means "a profile you may
  select". One locator matched two elements. That was the fifth shared-selector collision in this
  shell, and it happened after checking every *other* attribute the header introduced.
* `FORBIDDEN_COMMAND_PREFIXES` said `typeHierarchy` and `callHierarchy` in camelCase. The real ids are
  `typehierarchy:toggle` and `callhierarchy:open`, lowercase, so the list matched **nothing** and the
  menu would have looked cleaned. The claim that reads the rendered View menu is what caught it; every
  prefix was then read out of the installed packages instead of guessed.

**What is left of this, precisely.** Explorer, Search and Source Control toggles already sit under
`View -> Views`, because `AbstractViewContribution` registers every toggle there -- including ours. So
"Advanced Tools" is a *regrouping* of that submenu into domain views and tool views, and it needs each
tool's toggle command id. `fileNavigator:toggle` and `scmView:toggle` are confirmed; search's and the
terminal's are not, and guessing them is the mistake made twice above. A terminal tab also opens at
the bottom on boot, which the same regrouping should stop.


#### Re-reading the catalogue, and what is not watched

**`Reload Catalogue` re-reads the sources, and it does so more strongly than "drop a cache":**
`CatalogueStore.load()` calls `initialize()`, and the backend's `initialize()` disposes the engine and
spawns a fresh process. A new process has no cache at all, so a corpus edited outside Studio is
certainly picked up. The cost is a process restart per reload, which is the existing contract -- the
reconnect path depends on the same call.

**Nothing detects an external change, and that is stated rather than implied.** The engine caches
`state.catalogue` and drops it only on `initialize`; there is no filesystem watch on the source roots.
So a running Studio holds the catalogue it scanned. This was observed the hard way: a `gear.gdl` in
`gears-rust` acquired two typographic quotes before its `#`, the engine correctly refused it with
`GBX0101`, the catalogue fell to 13 gears, and four claims that name `api-gateway` failed -- while the
file on disk had already been repaired. The repair was invisible until the catalogue was reloaded.

An automatic staleness indicator is therefore **out of scope**: it needs a watcher, a digest or a known
write event, and showing one without a detector would claim that Studio notices external edits when it
does not. When a *Studio* operation is the writer -- scaffolding, generation -- the indicator becomes
possible and honest, and that is where it belongs.

**Amended 2026-09-10: that argument covers the catalogue's source roots and `.gearbox/**`, and it was
being read as covering the product description too.** It does not. A `product.gdl` lives inside the
workspace and Theia watches it -- `files.watcherExclude` names only `.git`, `node_modules`, `target`
and `.gearbox` -- so for that one file the detector this paragraph says is missing has been there all
along, delivering events nobody had subscribed to. The consequence was not a missing indicator but a
wrong panel: every Gearbox surface renders from `ProductStore` or `CatalogueStore`, so saving an edit
in the editor left all twelve of them showing the previous resolution, and the complaint arrived as
"the Inspector does not update".

`DescriptionWatchService` now subscribes, debounced, and skips while a buffer holds unsaved edits --
the same reason the write gates refuse there. The paragraph above stands unchanged for the two cases
it was actually written about: `.gearbox/**` is excluded from the watcher deliberately, and the source
roots are not watched at all, so a `gear.gdl` edit still needs `Reload Catalogue` and still costs an
engine restart. The asymmetry is the engine's catalogue cache, not an inconsistency.

#### No test may leave a product description changed

Two claims edit `products/payments-demo/product.gdl` on purpose and put it back. Twice, something else
also wrote to it mid-run -- a stray `use_gear("grpc-hub")` once and a stray `use_gear("types-registry")`
another time -- and the consequence was failures in unrelated files, because every claim about the
resolution is a claim about that description. Neither failure named the cause.

**The cause has not been found.** It did not reproduce across three full runs with a stack trace armed
on `ProductEditService`, the only code path that writes, and the mechanism that seemed most likely was
checked and ruled out: the catalogue's in-product toggle is the first focusable child of each row, so
`Enter` on a focused toggle would open a write dialog -- but no test presses `Tab`, the only `Enter`
presses land on a row or in the command palette, and a modal dialog would have prevented the palette
from opening at all.

So the guard does the next best thing instead of pretending the problem is solved. `global-setup`
refuses to start a run against a modified description; a hook in the fixture checks after **every**
test and **fails the test that did it**, printing the diff that names the gear, then restores the tree
so the rest of the run is still worth reading. Silently restoring -- which the first version did --
would have hidden it a third time.

Two hazards were closed while looking. The dialog clicks in `adr-0010-ownership-tiers.spec.ts` were
unscoped `.theia-button.main`, which is every Theia dialog's default button: a click would have
accepted whichever dialog happened to be open, and *this* dialog writes to a description. They are now
scoped by the preview pane that makes the dialog theirs. And moving the mutating tests onto a copy of
the workspace, which would remove the hazard class entirely, is blocked until a product can be opened
from outside `products/*/` -- the picker globs that directory, and a second product there changes the
"open it if it is the only one" behaviour. That belongs with the product session.


#### The lock records what was read, not who asked

`ResolvedSource.digest` was `path:<declared location>` -- the caller's own spelling of the source
root. That put the spelling into `lock_hash`: the CLI run from the repository declared
`../gears-rust` while Studio's backend declared an absolute path, so **one product and one profile
serialised to two different locks**, observed as two files on disk with different hashes. A field
whose job is to make "did anything change" a byte comparison (`cpt-gearbox-nfr-determinism`) cannot
depend on how the question was phrased, and an absolute path in a lock is not portable to another
machine at all.

Both halves are fixed:

* **`digest` is a content digest** over the descriptions actually read -- `gearbox_engine::content_digest`,
  blake3 over sorted relative paths *and* bytes. Paths as well as bytes, because a description that
  moves is a change; lengths written before each field, because otherwise (`ab`, `c`) and (`a`, `bc`)
  collide. It is computed in the catalogue loader after discovery, since the reader is the only thing
  in a position to say what was read -- `SourceRoot::to_resolved` reports `unread` until then, which
  is visible in a lock rather than a plausible-looking hash of nothing.
* **`location` is relative to the description** in a lock, via `gearbox_engine::lock_sources`, which
  also takes the digest from the catalogue rather than recomputing it. That is the form
  `gearbox-lock`'s own fixture already hand-wrote (`../../../gears-rust`), and it means the same
  thing on every machine.

`lock_sources` replaced the same three-line map at three call sites, so the CLI's `resolve`, the
CLI's `generate` and the RPC can no longer drift apart in how they describe a source.

**Two consequences.** Studio no longer generates into a tree of its own: it had been writing
`.gearbox/studio/<product>/<profile>/` purely so the two clients would not rewrite each other's
lock, and with that gone the engine's default `.gearbox/<product>/<profile>/` stands -- one tree, and
it is the tree §12 step 2 builds and runs. And **the Lock view's diff against the lock on disk (§9,
still `test.fixme`) is now honest to build**: it would previously have reported a CLI-written lock as
permanently stale.

Proved two ways: `cargo test -p gearbox-engine --test source_digest` holds the properties
individually, and generating the same product twice -- once with a relative root and a relative
description, once with both absolute -- produces byte-identical trees, `product.lock` included.

#### One selection, and the two panels that could not share it

`Gear detail` rendered a `GearDescriptor` from a catalogue row. `Explain` rendered the resolver's
`because` sentences from a product focus. They sat side by side in the bottom bar, and the ordinary
act -- click a gear in the product tree -- filled the second and left the first saying "select a gear
in the catalogue". Two panels, one of them always apologising, and no way to tell from either which
selection it was about.

The fix is not wiring: it is that there was one selection all along and two places holding it. So the
value moved out of both stores into `SelectionService`, and `CatalogueStore.selected` and
`ProductStore.focus` became views onto it. A **projected** catalogue row is normalised to
`{kind: "gear", id}` at the moment it is chosen, so picking `cluster` in the catalogue and picking it
in the product tree are the same selection and both views highlight it. `catalogue-row` survives only
for a row with no `GearId` yet, which under ADR-0009 is every row until S2 has run on it.

The two panels then became sections of one **Inspector**, and the merge bought a claim that could not
be written before: selecting `api-gateway` in the product tree says *both* what it is and why it is
there. The sections keep their old class names -- they are still the detail and the explanation -- so
every assertion written against their markup still tests the same thing.

Two columns rather than stacked, because the bottom panel is wide and short: stacking put "why" below
the fold for anything longer than a two-line gear, which would have reproduced the original problem in
a single panel.

#### Conflicts as a screen, and a `subject` that nothing had ever set

The diagnostics used to print under the product tree, where nobody could act on them, and
`ResolutionMarkers` put the same array into Problems. eCos's Config Tool makes conflicts a domain
screen; this now does too, and the Product view keeps a one-line summary that opens it -- one array in
`ProductStore`, two renderers, so they cannot disagree about what the resolution said.

What the screen can show that a marker cannot is the `help` sentence, the `related` locations, the
`evidence` citation, and the `subject`: "the graph node this concerns, **so a client can select it**".
Building the screen was how it emerged that `Diagnostic::about` -- the builder that sets `subject` --
**had no callers anywhere in the engine**. The field had been on the wire, documented, and always
`None`.

So the claim would have been permanently unobservable, and the honest fix was upstream of the UI. One
diagnostic now sets it: GBX0409, "the endpoint override for X on Y cannot come from an environment
variable", whose subject is unambiguous -- exactly one binding, named by consumer and contract, and the
explanation graph holds a node for every resolved binding. Deliberately only that one: most
diagnostics concern a resolution as a whole ("two severable edges stay local because the profile is
single application" names no node), and inventing a subject for those would send a reader to a node that
does not explain them.

That also produced a smaller correction worth keeping. The binding node key -- `{consumer}|{contract}`
-- was formatted inline in the graph builder, and a diagnostic naming the same node would have had to
format it again. Two format strings for one wire convention is how a client ends up asking about a
node that is not there, silently, because a missing node reads as "no provenance recorded". It is
`gearbox_ir::binding_key` now, used by both.

#### Home is a screen, not an empty area

The Home context was the catalogue and an empty main area, which reads as an application that failed
to load something. STM32CubeMX opens on New / Load / Recent and only then shows domain views; the
**Start** screen is that for products: **New Product…**, **Clone**, Open, discovered and recent.
Catalogue on Home is secondary (Browse), not the only story. New Gear / Open Gear stay off Start
until the scaffold lands — a fake button is worse than two honest ones.

One mechanical trap, and it is the second time this shape of thing has cost a debugging session:
`applyViewPlacements` runs on a perspective's **first** activation only, so on the second visit to Home
the placement is a no-op -- and `shell.activateWidget` on a widget nobody has built does nothing at
all, silently. Coming back to Home after closing a product therefore has to *open* the Start screen,
not activate it, which is why `GearboxPerspectives` reaches for the view contribution rather than
making do with the shell it is handed.

#### The catalogue, called instead of browsed

ADR-0011 made the catalogue a perspective equal to the product, and its own revisit clause named the
condition for collapsing that: "if in practice nobody uses the catalogue except while editing a
product". Which is the case. So it is a **source of components** now -- secondary Browse on Home
(Start carries New/Open Product), and `Find Gear…` in the product context, where the panel had been
taking the whole left side while the product itself sat in a secondary tab.

Finding a gear selects it, which fills the Inspector. Adding one used to be a bare toggle in that
panel; it opens the Add Gear configurator now -- see below.

#### The palette was never narrowed, and the file said it was

`ShellPolicy`'s own header claimed three surfaces: the menu bar, every other menu, and "the command
palette and keybindings -- reached by unregistering the command itself". The code did the first two.
It called `unregisterMenuAction`, which removes menu nodes; the palette does not read menus. It reads
`CommandRegistry.getAllCommands()` filtered by `isVisible && isEnabled`
(`quick-command-service.js:191`).

So every suppressed view was one `Ctrl+Shift+P` away, and a saved keybinding still opened it, for the
whole life of that code. The assertion that read the rendered View menu -- the one that caught the
camelCase mistake in the prefix list -- could not see this, because the palette is not a menu.

Commands are now unregistered outright, and the sweep runs again on `onCommandsChanged` under its own
guard, because plugins register late and `unregisterCommand` itself fires that event. A conformance
claim reads the palette per family. The lesson is the narrow one: *a whitelist has to be checked on
every surface it claims*, and the check has to use that surface's own mechanism.

`Explorer` and `Source Control` moved into `View > Advanced Tools` in the same pass -- kept, because
the work ends in generated crates someone will read and diff, and demoted, because they are tools
rather than one of the two things this application is about. Search has no entry: the package is not
installed, and a menu item for an absent package is a rule about nothing.

#### A confirmation that any Enter could answer

The stray `use_gear(...)` write into `products/payments-demo/product.gdl` -- three occurrences, no
reproduction across four instrumented runs -- has a mechanism now, found by reading Theia rather than
by another run.

`DialogOverlayService` adds its Enter listener to **`document.body`**
(`@theia/core/lib/browser/dialogs.js:82`), and `AbstractDialog.handleEnter` accepts unless the event
came from a textarea. `onActivateRequest` focuses the accept button. So while an edit confirmation is
open, **any** Enter anywhere in the application writes to the description -- and the test fixture's
`runCommand` pressed Enter blind whenever no palette entry matched what it typed. The page is
worker-scoped and the suite runs one worker serially, so a dialog left open by any earlier test
outlives it and waits for a keystroke meant for something else.

Four changes, in order of how much they matter:

1. **The write dialog no longer accepts on ambient Enter.** `EditPreviewDialog.handleEnter` does not
   accept, and the *cancel* button takes the initial focus. A focused accept button still activates on
   Enter, natively, so the keyboard path to Yes survives -- only the ambient one is gone. That is the
   right default for a dialog whose Yes edits a file, and the wrong one for "do you want to reload?",
   which is why it is a subclass rather than a change to `ConfirmDialog`.
2. **Every fact is re-established after the dialog returns.** The four checks used to run *before* it
   and the write happened *after*, with nothing keeping the world still in between. `stillTrue` now
   re-checks the store's revision, the open product's path and the buffer's cleanliness, and re-runs
   the dry run and compares it byte for byte -- so an answer about a state that has gone is refused
   with a message saying so, instead of being applied to whatever is there now. This is a real defect
   independent of the P0.
3. **The fixture throws instead of pressing Enter**, listing what the palette actually offered. It
   also refuses to drive the application while a dialog is open, at the two doors every test goes
   through. A leftover dialog is not untidy, it is armed.
4. **A stack trace is logged at the moment of the write**, so if it happens again the cause is named
   rather than inferred.

What was *not* done, and the reason: the plan called for moving the mutating tests onto a temporary
copy of the workspace. That would have protected the two tests that already restore the file and left
the actual exposure untouched -- the stray write lands in tests that never edit anything, because the
shared session has the real product open. Containing it properly means moving the whole suite's engine
workspace, which breaks the claim that reads generated output through the Explorer and every assertion
about a path under `.gearbox/`. So the mechanism was removed instead of the blast radius, and the three
guards stay as a net rather than as the defence.

#### The fourth surface, and a menu trimmed by group

The menu bar had been narrowed to five names, and the names still held a general editor's verbs:
`File` offered `Open Workspace` and `New Window`, `View` offered `Editor Layout` and the toggles of
views this application does not have. Two of those were not clutter but a contradiction: the Theia
workspace is an internal set of source roots that `ProductSessionService` derives from the open
product, so `Open Folder` and `Close Workspace` offered to move the ground the product stands on,
behind the session's back.

**Trimmed by group.** `File` keeps `0_product`, `3_save`, `5_settings` and `6_close`; `View` keeps
`0_primary`, `2_views` and `9_advanced`. Groups rather than command ids because groups are declared
once in `common-menus.js` and survive upgrades, while ids do not: three in `ShellPolicy` had to be read
out of the packages after a guess was wrong, and a fourth turned up while writing this --
`outlineView:toggle` never matched a prefix spelled `outlineView.`, so the Outline toggle had been
sitting in `View` the whole time, one dot away from removal. Two exceptions are named individually,
`Save As…` and `Close Workspace`, because they share groups with entries that stay.

**`Open View…` is a fourth surface, and it was found the same way as the third.** After the menu bar,
every other menu, and the palette, there is `QuickViewService` -- filled by every
`AbstractViewContribution` (`view-contribution.js:114`) and reading neither menus nor commands. The
`Plugins` view had been removed from the left bar, from `View`, and from the palette, and `Open View…`
still offered it. Hidden there by label now.

Four surfaces, and each one was discovered because something suppressed everywhere else showed up on
it. There is no reason to think this is the last, which is the whole argument for claims that read
what is rendered.

**Moving a submenu taught something about the API.** `Appearance` and `Editor Layout` had to travel
into `Advanced Tools`, and Theia has no move -- a node belongs to whoever registered it. The obvious
version, copy then remove, silently undid itself: `unregisterMenuAction(id, path)` removes every node
with that id **anywhere in that path's subtree** (`menu-model-registry.js:202`), and `Advanced Tools`
is inside `View`, so removing `1_appearance` from `View` also removed the copy just made underneath it.
The probe said the source had two children and the destination was empty, which is exactly what that
looks like from outside. It takes a snapshot -- plain data, which nothing the registry does afterwards
can reach -- then removes, then replants.

**The `Plugins` view.** A window onto the plugin host, listing the one VS Code extension there is,
which is git. It never opened itself: `@theia/plugin-ext` binds the contribution and nothing more. So
everyone who saw it in the left bar saw their own saved layout -- the same shape as `Type Hierarchy`
and the boot `zsh`, and invisible to the suite for the same reason, because every run starts with a
clean profile. `LayoutMigration` version 3 closes it.

#### The product session that had never once opened a product

`ProductSessionService` was written to own the engine's roots and its write
boundary, and its documented first step is: initialize with no roots at all, because
evaluating a description needs no catalogue. The engine disagrees. Drive
`gearbox rpc --stdio`, send `initialize({roots: []})` and then `product/load`, and
the answer is:

```text
no source root is open; pass `roots` to `initialize` or `--root` to the CLI
```

So step 2 threw, the open returned false, and the session never reached the steps
that derive the roots from the product's `sources`. Every product opened in this
application was opened by the *fallback* engine -- the one the backend spawns with
its built-in roots and the repository as workspace -- and the session's whole
purpose, "the roots and the write boundary come from the product", had never once
run to completion.

Nothing noticed because the fallback is correct for this repository: one product,
one corpus, the neighbour path the backend hard-codes. The suite passed on the
engine the session had failed to replace.

It surfaced only when serialising the catalogue's loads removed the race that used
to leave a *different* engine running by the time step 2 ran. The lesson is the
one this plan keeps relearning: **a step whose failure is recovered by an accident
elsewhere is a step nobody has tested.** The refusal is now what step 1 avoids --
it carries the roots already open, which at boot are the backend's defaults and
after a previous product are that product's, and step 4 replaces them with the ones
the description declares.

Two smaller defects fell out of the same investigation, both mine:

**The load queue read its session too late.** Serialising `CatalogueStore.load` --
necessary, because `initialize` respawns the engine and two in the air mean one
dies under the other's request -- was written to read `this.session` when the load
*ran*. The boot load is queued first and a product session opening in the same tick
sets that field to its own no-roots session, so the boot load initialized with no
roots and the catalogue reported "no source root is open" before anything had gone
wrong. The session is captured when the load is *asked for* now, and remembered for
later bare calls, which are two different jobs that one field was doing.

**The write boundary was the product's own folder**, so the engine's output root
became `products/payments-demo/.gearbox/payments-demo/dev` -- inside the
descriptions directory, and not the tree `gearbox generate` writes from the
repository root. One product, two trees: exactly the divergence that removing
Studio's private output tree had closed. The boundary is the **workspace folder
containing the product** now: it contains the description, so an edit is inside it;
it is the root the CLI is run from, so both clients write one tree; and it is the
folder the person opened rather than one derived behind their back.

#### A native module built for the wrong Node, and a bundle that kept a copy

Restarting the application produced `Process from config.webServer exited early` and a backend log that
stopped after `loading modules...`. Worth recording because none of it was in this project's code.

The backend was segfaulting in `dlopen`. `drivelist` -- required by `@theia/core`'s
`env-variables-server` -- had been built on 2026-08-29 against **Node 24 headers, N-API 10**
(`build/config.gypi` names the nodedir), while the installed Node is **20.20.1, N-API 9**. Loading an
N-API-10 binary in an N-API-9 runtime is an immediate SIGSEGV, which `theia start` reports as a child
that exited. `npm rebuild drivelist` fixes it.

The second half took longer and is the part worth remembering: **the webpack backend build copies
native modules into `browser-app/lib/backend/native/`**, so rebuilding the package is not enough --
`lib/backend/native/drivelist.node` was still the Node-24 binary, and `theia start` uses the bundled
backend rather than `src-gen`. `npm run build --workspace browser-app` after the rebuild is what
actually fixes the startup.

It also explains why nobody noticed: a process that has already loaded the module keeps running, so the
application worked for days and only a *fresh* start failed. Any Node major-version change in this
repository needs `npm rebuild` followed by a bundle rebuild, and a backend that dies at
`loading modules...` with no error is the signature.

#### Three sessions, and the difference between a fix and a checkbox

A UX pass went through Home → New Product → Clone → Open Product → Product → Profile → Inspector →
Conflicts and came back with a verdict of *request changes*. The findings were not cosmetic: the
engine was down and the shell went on offering New Product, Resolve and Generate, so the wizard opened
as a blank tab; opening a product left the centre empty because the Product perspective only
*activated* a widget that did not exist; `Cancel` on a config edit left the typed value on screen while
the file kept the old one; adding a profile blocked the whole application on `window.prompt`.

What that list has in common is that **the commands had no session behind them**. Each was independently
plausible and the set of them did not add up to a state a person could name. So the answer was three
states -- Home, Product, Gear -- and the work that makes each of them true:

* an `EngineConnectionService` every engine-dependent command reads, so "the engine is gone" is a
  thing the shell *says* rather than something the person infers from a wizard that does nothing;
* `openView` in the perspective's `onActivate` for Product and Inspector, not `activateWidget` --
  `applyViewPlacements` runs on first activation only, and activating a widget nobody built is a
  silent no-op, which is the third time that trap has cost a session in this file;
* a **draft** in `ProductEditService` with `Apply changes` / `Discard`, and one `gearbox/product/applyEdits`
  batch behind it: several fields become one dry run, one confirmation, one write. `Discard` restoring
  the saved value is what closes the "Cancel lies" hole for good, because the inputs are no longer
  uncontrolled;
* `Add Gear` as a configurator rather than a `+` that writes immediately;
* `gearbox/gear/scaffold` -- a real tier-0 file plan (`gear.gdl`, `Cargo.toml`, `src/lib.rs`) written
  atomically -- so Home has four operations rather than two and a promise.

**Two of the eight phases were marked done and were half done**, which is worth recording because the
repository did not join in. The Add Gear configurator had six of its nine intended sections: the
hypothetical re-resolve and the application/contract diff were not built, and the widget said so on screen
("the full resolution closure appears after Add"). `config_schema` is still `Option<RelPath>` with no
projection into the IR, so configuration values are strings and the claim that would prove otherwise is
still `test.fixme` with the reason written in it. The plan's checkboxes were wrong; the code, the UI and
the conformance table were not. That asymmetry is the useful part -- a checkbox is a claim about
yesterday, and the only claims worth trusting are the ones something re-checks.

#### The missing sections, and the one that is not coming

The re-resolve is now built (ADR-0013 amendment 2026-09-03). `gearbox/product/resolvePreview` applies
`add_gear` and the panel's edits **to the description text in memory**, resolves that, and returns an
ordinary `ResolveResult`; the panel subtracts the resolution already on screen from it and renders the
difference as section 6, "What changes". The engine change was one split -- `load_product` reads the
file and delegates to a new `eval_product_text` -- and it is resolution-neutral: `lock_hash` for `dev`,
`local` and `prod` is byte-identical across it.

Two things about that call are worth keeping. It writes nothing, so it needs no write gate: the panel
asks on a debounce, and a preview that wrote would be indistinguishable from the act it previews. And
it takes the panel's `edits`, not just the gear -- a plugin *is* a gear, so a preview that ignored the
Plugins section would understate the closure by exactly the amount the section exists to reveal.

`Add to Product` stays enabled when the proposed resolution has errors, with the count beside it.
Building a product is add-a-gear-then-bind-it; blocking the first step until the second is done makes
the intermediate state unreachable, and that state is where most of the work happens.

The ninth section, typed controls projected from `config_schema`, was **not** built, and the reason
was a measurement rather than a preference: of the twelve `gear.gdl` files in the corpus, zero
declared `config_schema` -- every match in the tree was documentation. A projection built against no
examples would be a guess wearing the clothes of a feature.

#### The ninth section, and the measurement that unblocked it

It is built now, and what changed first was the corpus rather than the code. Nine of the fourteen
`gear.gdl` files declare `config_schema`; the other five are correct to say nothing --
`service-discovery` reads no configuration at all and only *rejects* a stale key,
`api-contracts-consumer` ignores its context, and `api-contracts`'s config struct is a unit struct
nothing deserializes. Two more, `types-registry` and `cluster`, declare configuration with **zero**
scalar fields between them -- three `Vec`s and a `BTreeMap` -- so they get no controls either. That
is five gears where absence is the answer, and it was worth counting before building a form.

**The split is the interesting part, and it moved a row of ADR-0002's table.** A field's name, type,
requiredness, default and doc comment are Rust facts and are projected; what a description declares
is only *which* fields are worth showing an integrator. `ApiGatewayConfig` has fourteen fields and
the platform's own configuration files set five. And `exposes` is checked against the struct on every
load (`GBX0212`), so the selection cannot quietly stop matching -- which is what keeps a curated list
from decaying into the second copy the ADR exists to prevent.

Finding the struct turned out to need the same lesson the vendor defaults already taught: read
**both** spellings. The `Gear` trait has no associated `Config`, so the only link is the single
`ctx.config*()` call in `init` -- written as a turbofish by `api-gateway` and as a binding annotation
by the other ten. Reading one would have reported "no configuration" for almost the whole corpus, and
looking for `src/config.rs` instead would have missed `grpc-hub`, whose struct lives in `src/gear.rs`.

Two things surfaced only because the projection ran against real gears. A typed control could not
write a typed value at all: the wire carried `Option<String>` and the dict editor quoted at three
sites, so a checkbox would have written `"True"`. And the evaluator knew no floats and capped
integers at `i32`, so a number control could have produced a description that parsed and then refused
to evaluate -- the one way this work could have made things worse than the strings it replaced. Both
were fixed in the commit that introduced the typed wire, not after it.

Reading the nine gears also caught three wrong defaults, all from one arm that took any path
expression as a string: `None` became `"None"`, a `const` became its own identifier, and an enum
variant became its Rust ident (`AcceptAll`) rather than its wire spelling (`accept_all`) -- a
placeholder offering a value the gear rejects. A projection is only worth what the corpus proves
about it.

Carrying the value through to the runtime was its own step, and it turned up the last defect worth
recording. Once a product's config reached the generator, a key an endpoint derives -- `bind_addr` --
was overwritten by the projected port **in silence**. The precedence is right, since a hand-written
port describes a product that was not resolved, but the silence was not: that is `GBX0114`, a
warning naming the key and the endpoint.

One prediction in the plan was wrong and is worth correcting rather than quietly dropping. Part B was
supposed to move `lock_hash`; it does not, because the new field is skipped when empty and the demo
product sets no config. Measured against a worktree at the previous commit, all three profiles hash
identically. A product that *does* set config gets a different hash, which is the behaviour that was
actually wanted -- two products differing only in a config value are different products.

What remains of that work after M7 is the live `kind` install, which this repository does not
run: `helm lint` and `helm template` plus the schema `--set` rejection are the acceptance. The
demo product's lock still has an empty `cluster` list -- no corpus gear requires a primitive --
so `existingSecret` is asserted against a lock fixture, not against `payments-demo`.

#### M7

A Kubernetes profile now emits a Dockerfile per application, an umbrella chart with
one subchart per application, `values.yaml` as `OperatorOwned`, and
`values.schema.json` with `additionalProperties: false`. `dev`'s `lock_hash` is
byte-identical to the commit before this milestone. `local` moved because
`GBX0604` now carries the evidence its own `requires_evidence` flag always
demanded -- the same `Diagnostic::validate()` pass that `GBX0603` needed -- not
because image, subchart or Service DNS leaked off Kubernetes. `prod` moved for
those plus the filled `endpoint` values that make `consumer_wiring` real.

The live `kind` install is still out of scope; `helm lint` / `helm template`
and the schema `--set` rejection are the acceptance that ran.

One thing was fixed the wrong way first. The toolbar showed `Toggle Gearbox Generate`, and the repair
was a map from command id to caption inside `ToolbarWidget` -- which is exactly the drift that file's
first paragraph rules out, since the menu and the palette would have gone on saying the long phrase.
The caption belongs on the command: `GenerateViewContribution` registers its toggle with
`shortTitle: "Generate"`, the way `RESOLVE_PRODUCT` always did, and the header renders
`shortTitle ?? label` for everything with no special cases left.

#### M6, and the gear it turned out not to need

The milestone reads "new gear lands and passes its own test *by hand first*; then generated worker
crate". The first half was skipped, and the measurement is why: `products/payments-demo` resolved
for `local` already produces a worker. `api-contracts` leaves the gateway because its contract edge
is **severable** — the pin `application("audit", ...)` is scoped to `prod` and had nothing to do with
it. So the generator had a real second application to build against without anyone writing Rust.

It is also the right one. `api-contracts` / `api-contracts-consumer` is the only pair in
`gears-rust` using `#[toolkit::provides]` / `#[toolkit::consumes]`, which makes it the only contract
severable over REST through the directory with no change to a gear's source. The resolver picked it
unaided.

Three things were missing, and each was inert without the other two, which is why none of them had
been noticed. `ResolvedApplication::spawns` was hard-coded empty and nothing ever wrote it, so
`write_spawns` and its `runtime: {type: oop}` section were correct and dead. A worker had no address
at all: its gears mount on a REST host in the monolith and a worker has none, so it serves through
the out-of-process runtime's own listener — a top-level `oop_http` section, not a gear key, and
without it the runtime silently takes the legacy gRPC path and never registers. And `write_spawns`
looked its gear section up and skipped when absent, which it always was.

That last one is worth keeping. **A spawned gear is deliberately not linked into the host**: the
runtime discovers gears through `inventory` and takes no notice of `runtime.type`, so a gear both
linked and marked `oop` runs twice — in-process and as a child. The example server in `gears-rust`
has exactly that bug for `calculator`. Configured and linked are therefore different sets, and the
host's config is the one place they differ: it is configured to start a gear it does not contain.
The generator gets this right because `registered_gears.rs` is built from `application.gears` and the
partition already removed the anchor, but that is now asserted rather than relied upon.

`Generated::skipped` is gone rather than emptied. The dispatch on `ApplicationKind` is exhaustive, so a
kind this cannot generate is a compile error at the `match` instead of a value at run time — and the
field reached the Studio, where it told operators that worker entry points were unbuilt. An
always-empty report is one nobody can read.

**What is deliberately not done:** `payments-audit`. (The live run of §12 step 3 has since been
done; see M6.) The reason is
not caution. `oop_http` is fully implemented in the runtime and covered by its own tests, but
**nothing in `gears-rust` uses it** — the runtime's own design note says there are no checked-in
`oop_http` configs, and its one real worker, `calculator-oop`, takes the legacy gRPC path. A
generated REST worker would be that path's first consumer anywhere. The generation is verifiable
offline; the run is pioneering, and the risk is not in this repository.

Both generated binaries do compile against the real tree, which is the same bar M5 held.

#### A terminal that was never openable, and a claim that passed for the wrong reason

The shell stopped opening a `zsh` at the bottom of a fresh window. That was asked for: a shell nobody
requested, holding a slot in the bottom bar, is part of what made this application read as an IDE with
panels rather than a tool for building products.

Suppressing it is done where it is opened -- `HiddenTerminal` overrides
`TerminalFrontendContribution.initializeLayout` with a NOOP -- which is the mechanism ADR-0011 already
names for Debug and Test. It is emphatically **not** done by closing the widget, and that distinction
was learned by doing it wrong first: `LayoutMigration` closed `terminal-0` along with Type Hierarchy,
and `widget.close()` disposes the widget while `WidgetManager` keeps its entry under the same id, so
the next request returned the disposed instance and nothing appeared. *Not opening* and *closing* are
different acts, and only the first is reversible.

Which surfaced something worse. The claim "a terminal can be opened" had been passing, and it had been
passing for the wrong reason: it asserted that a `zsh` tab was **already** at the bottom of a fresh
shell, and that clicking it rendered an xterm. Both were true -- because Theia opened it. Nothing ever
exercised *creating* one. With the boot terminal gone the claim became testable for the first time, and
failed: `Terminal: Create New Terminal` creates nothing, by command and by keybinding alike, with no
error and no notification.

Two measurements were taken before writing that down, to avoid both claiming a regression that was not
one and disowning one that was. Built with the suppression removed: the boot terminal returns, and
creating one **still** does nothing -- so the suppression did not cause it. And `node-pty`'s binary is
present while the boot terminal did attach a pty -- so the install is not broken either. A third
detail explains why nobody noticed: `.xterm` is absent even when an inactive `zsh` tab exists, because
the canvas materialises on activation, and the old test activated it with a click.

So ADR-0011's consequence -- a live shell in the bottom panel, kept on purpose -- is **currently not
true**. The only terminal that ever worked was the one nobody asked for. The claim is `test.fixme`
carrying these measurements rather than deleted, because the choice is not the implementer's: either
creating a terminal is fixed, or the ADR stops promising a terminal. What is *not* acceptable is the
state this replaced, where a green claim implied a capability that had never been exercised.

#### The second UX pass, and the three mechanisms under it

A second pass went through Home -> New/Open Product -> New/Open Gear -> Product -> Add Gear ->
Inspector -> profiles -> Conflicts -> Generate -> Lock -> Graph and came back at 6.5/10, *request
changes*. The verdict was not that the shell is wrong; it was that a few remaining inconsistencies
**undermine trust in the configuration**. Three findings carried the weight, and each turned out to
be one mechanism rather than one symptom.

**Opening a product left Home in the centre.** The header switched, the catalogue marked the
selected gears, and the middle of the screen still offered to open a product -- reachable only by
`View -> Gearbox Product`. `PerspectiveService.switchPerspective` returns early when its target is
already the active perspective (`perspective-service.js:114`), and `ShellLayoutRestorer` sets
`activePerspectiveId` from persisted state before any Gearbox code runs
(`shell-layout-restorer.js:189`). So a session that had a product open came back with
`gearbox.product` active and *no product open*; `StudioContextService.recompute` derived `home`,
compared it with its own previous `home`, and returned early -- leaving the shell's answer and the
context's answer disagreeing. The next open then asked for the perspective the shell already
believed was active and got nothing: no `onActivate`, and `gearbox-perspectives.ts` was **the only
code in the application that put the Product widget in the centre**.

Three things follow from that, and only the first is the fix. The context now reconciles against
*the shell's* answer rather than against its own previous one -- but only after `ready`, because
`FrontendApplication.start` runs every `onStart` before `attachShell` and before `initializeLayout`
(`frontend-application.js:59-66`), and a perspective switched from `onStart` is applied to a shell
nobody has attached and then overwritten. That was measured the hard way: the first version
reconciled in `onStart` and broke the scaffold preview and both git claims, because the workspace
roots had not landed yet. Second, `ProductViewContribution` opens its own view from
`ProductSessionService.onDidChangeOpening` -- the symmetric twin of what `StartViewContribution`
already did for Home -- so the centre no longer depends on a layout event at all, and the ~3 s of
two engine spawns is a `Loading <product>...` state in the panel that is going to hold the answer.
Third, Start **closes** when the context leaves Home, because a Start screen behind a product is the
hybrid state the pass objected to.

**`ensureOpen` had to go with it.** The Product widget's constructor opened the only product it
could find, which made it a rule about widget construction rather than about intent: a restored
layout naming the Product view put a product back on screen with nobody having asked, and Home was
unreachable while one existed. It is a Continue card on Start now -- named, timed, one click -- and
`openProduct` in the fixture opens a product the way a person does instead of inheriting one from
boot. Every test that assumed an open product at startup was relying on that accident.

**`Discard` cleared the badge and left the typed value on screen.** One draft, product-wide, and
*two* private remount counters: the Inspector and the Product view each rendered
`Apply changes`/`Discard` gated on `hasDraft()` and each bumped its own `editEpoch` on Discard. So
discarding through one panel remounted that panel's inputs and left the other showing text the file
did not contain -- the interface asserting an edit was dropped while displaying it. A React input
whose `value` prop is unchanged between two renders is not rewritten, which is why the remount is
needed at all and why it cannot be per-panel.

The claim that covers this **was already green**: `adr-0011-session-trust.spec.ts` asserts the DOM
value, and it clicked `[data-draft-discard]`**.first()** -- which happened to be the panel whose
counter its own click bumped. That is the third time in this file a claim has passed for the wrong
reason. The epoch belongs to `ProductEditService` now, there is exactly one Apply/Discard pair and
it lives in the header beside the `modified` badge it already rendered, and the claim asserts the
count as well as the value. Which controls hold the unapplied edits is marked on the controls
(`data-field-modified`), because one badge in the header cannot say *where*.

**Add Gear offered a plugin to a gear that declares no extension point.** Selecting
`types-registry` printed "Extension points: none declared." three lines above a list of every
plugin in the catalogue; choosing `oidc-authn-plugin` reported it joining the closure as a "plugin
of types-registry". Both halves of the join key were already on the wire -- the host's
`extension_points`, the plugin's `fills.point` -- so the offer was the defect, and the list is
filtered on the pair now, grouped by point when a host declares several. A choice that cannot be
right is not offered rather than offered and then refused; that is the eCos lesson the Conflicts
screen already follows.

The engine was no better, and a client is not a boundary. `report_orphan_plugins` asks whether
*some* selected gear expects the plugin's point, which is the right question for a plugin selected
as an ordinary gear and the wrong one for a plugin written into a specific host's
`plugins = [...]`: with `authn-resolver` also in the product, the misplacement passed every check
while meaning nothing at all. That gap is **GBX0518**.

Two smaller things in the same pass, both the same shape -- a control offered for an operation that
cannot work. `Apply` in Generate stayed enabled over an all-`unchanged` plan, because every gate on
it was about permission or correctness and none about whether the plan writes anything; it now says
"generation is up to date -- 12 files unchanged" through the block list it already had, and
`FileAction::writes()` finally has a caller on this side. And a config key with a space in it was
written cleanly -- a key is a *quoted dict key* in the description, so the span surgeon has no
opinion -- and refused three steps later at resolve as GBX0115. The shape is checked where the caret
is now, and the schema is *reported* rather than enforced, because a curated `exposes` is
deliberately narrower than the struct.

#### The same pass, the rest of the way: focus, and the shell that kept moving

The pass's second and third tiers were about *focus* rather than trust, and two of them turned out to
sit on the same Theia behaviour, which is worth stating once because it will recur with every upgrade.

**`ApplicationShell.activateWidget` is slow in a way nothing here can see.** It waits on
`waitForRevealed`, which polls with **no timeout**, and on `waitForActivation`, which gives up after
2.25 s -- so a widget that never becomes visible costs that much, and
`PerspectiveService.applyViewPlacements` activates every entry in `viewPlacements` and then every
entry in `primaryViews`. Measured: a boot switch to Home landed its last activation 2.5 s after a
product had been opened and Add Gear opened on top of it, and brought Home to the front of both. Ten
Add Gear claims failed at once, each reporting the panel as attached but hidden.

Three things came out of that, and only the first is a fix to *our* code:

* the perspective descriptors carry **empty** `viewPlacements` and no `primaryViews`; every widget
  already declares its area in `defaultWidgetOptions`, so nothing moves, and *when* a view comes
  forward is `onActivate`'s decision -- which can be declined;
* a perspective's `onActivate` checks the context before acting, because it is fire-and-forget and can
  land after the context has moved on. "A perspective cannot promote itself into a context" now also
  means its callbacks may not act against one;
* `ProductViewContribution.mayTakeTheFront` refuses to steal the main area from another Gearbox
  surface -- Add Gear, Generate, Lock and the Graph are things a person navigated to -- while Start
  and an editor lose to it. The two wizards therefore *ask* for the product when they finish
  (`SHOW_PRODUCT`), which is also the "return to the Product workspace" the pass asked for.

**Closing a widget is not the way to hide a screen, twice over.** Start was closed when the context
left Home, and it cost two suite runs: closing during a perspective switch loses a race and poisons
the snapshot, so the product perspective then held a Start tab that `setLayoutData` restored as
*current*; and closing after the layout settles is late enough that Lumino's "activate a sibling"
rule steals the front from whatever the person has since opened. The invariant is about what is
*visible*, so it is held by opening the subject: Start stays a tab and is never in front of a product.
`conformance/ux-navigation.spec.ts` asserts `toBeHidden`, not `toHaveCount(0)`, and says why.

**The menu bar is rebuilt on a menu-model change and not on a context-key change**
(`browser-menu-plugin.js:45-54`). An *open* menu re-evaluates `when` and `isEnabled` as it opens,
which is why the entries inside `Product` were always right; the top-level label's enabled state is
decided when the bar is built, so it was correct at boot and stale forever after. That is the
mechanism behind the pass's "the Product menu is there with no product open", and the claim that named
it asserted only the half that was true. `ShellPolicy` touches the registry when the context changes;
the label is `aria-disabled` with no product, and the claim asserts that. `Find Gear…` and
`Reload Catalogue` moved to `View > Catalogue` in the same pass: neither is a verb on a product, and
their presence is half of why that menu looked like it had work to offer.

**Where things live now.** The Inspector is the right panel: the bottom strip showed one configuration
field at an ordinary window height, in the panel that *is* the gear configurator, and the two-column
layout that once justified the bottom becomes one column in a side panel. Problems and Outline join
Debug, Test and the terminal in `initializeLayout(): NOOP` -- markers are load-bearing, the panels are
not, and Conflicts is the domain screen for the array Problems also receives. A repeated sweep
detaches the widgets a *snapshot* brings back, which is the terminal the pass saw activate itself
after a close; detached rather than closed, for the reason `CLOSED_ON_MIGRATION` already records about
`terminal-`. And the Product view is four stages -- `Overview · Gears · Topology · Validation` -- with
Generate a link out rather than a fifth tab, because a tab holding a file plan and an Apply button
would be a second answer to a question the Generate view already answers.

**A gear created for a product now ends up in it.** The flow used to end in a notification asking the
person to add the gear themselves, and the reason was structural rather than lazy:
`writable_out_root` refuses to scaffold inside a source root (ADR-0010 tier 5), so a gear created for
a product is *always* in a directory that product does not read, and `use_gear` cannot reach it. So
`ProductEdit::AddSource` exists, `edit_call::add_source` writes the entry, and the finish is one batch
-- declare the folder, add the gear -- with one preview and one confirmation. The source id is derived
from the folder rather than asked for, and `SourceId`'s own rule validates it: the first version used
the GDL identifier rule and refused `gears-rust`, the id every product in the corpus uses. The
idempotence test is what caught that, which is what an idempotence test is for.

**And the scaffold has three shapes.** `service | plugin | minimal`, because seven of the fourteen
described gears are plugins and none is a bare crate with a name. What differs is which declarations
the file offers -- there is no compiler here and no way to know where the toolkit or an SDK lives, so
generated code would be written against a dependency this method cannot add. The plugin shape's two
load-bearing fields, `sdk` and `plugin_interface`, are written as **comments** with the sentence that
says what decides them: a `plugin_interface` naming no `pub trait` is refused (GBX0516) and an `sdk`
locator pointing nowhere fails the load, so a placeholder would hand its author a description to
repair rather than one to fill in.

Smaller things, each a control that said less than it knew: features are checkboxes from the crate's
projected `[features]` (7 of the 14 crates declare one); a config key with a space is refused at the
row rather than at the next resolve; `discovery` is a select over the two values `Discovery` has; a
field says whether its value is the product's, the resolver's or the gear's default, and an explicit
one can be reset; the catalogue's `+` announces which gear and which product; the lock opens on a
summary with the canonical text behind a tab; and every button in the Studio now has a `type` and, if
it is icon-only, a name -- with a source-grep claim so the next one does too.

One finding was declined, and it is worth recording as a disagreement rather than an omission. The
pass asked that `Add to Product` be blocked on blocking semantic errors. ADR-0013's amendment of
2026-09-03 decided the opposite for resolution errors and the reason still holds -- building a
product is add-a-gear-then-bind-it, and refusing the first step until the second is done makes the
intermediate state unreachable. What the pass actually hit was not a warning it wanted blocked but a
configuration the UI should never have offered, which is what the filter above removes. The button's
disabled state still means only what that ADR says it means, plus one honest addition: input that
could not be written at all.

A second finding is declined on a measurement. The pass reported `prefix_path = bad` as accepted
"though the description requires a leading slash", and the description does say that --
`api-gateway`'s own doc comment reads *"Must start with a leading slash"*. The runtime disagrees with
its own prose: `normalize_prefix_path` (`gears-rust`, `api-gateway/src/gear.rs:321`) trims, collapses
duplicate slashes and **prepends one when it is missing**, and the Helm generator does the same
(`generate/helm.rs:1063-1072`). So `bad` is a legal value that becomes `/bad`, and there is nothing
here to refuse: a client-side regex would be a rule this repository does not have, enforced against a
gear that accepts the value. What is wrong is one sentence in another repository's doc comment, which
is where a fix belongs.

#### Validation is a screen, and one row renders a diagnostic everywhere

The third UX pass called the Validation stage "a screen-transition": a centre panel holding
`0 errors / 2 warnings` and two nearly identical buttons -- `Open Conflicts` and `Show conflicts` --
while the rows that carry the code, the remedy and the location lived only on the Conflicts panel at
the bottom. A stage whose entire content is a way to leave it is not a stage, and a person who
navigated to it had navigated to the wrong place by definition. eCos shows the full list beside its
counter and its suggested fixes; DaVinci treats validation as a step of its own before generation.

The cause was not the stage, though. `Diagnostic[]` had **five consumers and four renderers**: the
Conflicts row, the Product view's counts, the Add Gear panel's key-value lines, the catalogue's
code-and-message, and `ResolutionMarkers` turning the same array into Problems markers. Only the
first showed `help`, `location`, `related` and `evidence` -- so in three places out of four a
diagnostic was a sentence with no way to act on it, and the panel a diagnostic happened to land in
decided whether its remedy was visible.

So the row moved to `browser/diagnostics/diagnostics-list.tsx`, unchanged, and the four renderers
became one. Validation opens with the summary and then shows the list; `Open Conflicts` survives as
one demoted link, because the bottom panel is still where the list is read *while* looking at the
tree that caused it -- duplication was never the objection, a screen made only of navigation was.
The one-line summary that appears on every other stage is suppressed on Validation, where it would
be a second, smaller copy of the summary that stage now opens with.

Two adopters gained something they had been missing rather than merely changing shape. The Add Gear
panel's "what this would introduce" now carries the location of the line that causes each new
diagnostic, at the moment a person is deciding whether to accept it; the catalogue's failed
projections now link to the `gear.gdl` that failed, which is the most actionable thing about them.
`density` distinguishes an aside inside another panel from a screen, and it is **spacing only** --
a compact list that hid `help` would give back exactly what one renderer exists to prevent.

#### Opening is staged, and Overview is the product at a glance

Two complaints from the third UX pass, and they are the same complaint at two
moments: the panel had nothing to say. During an open the centre was blank for
several seconds while the header said `resolving…`; after it, the stage every open
lands on held a profile and a link to a file.

**The wait.** An open is two engine spawns and a catalogue load -- `initialize`
with the product's own directory, evaluate the description, `initialize` again
with the roots it declares, then load the catalogue and resolve. Roughly three
seconds on this corpus, which is long enough that *which* three seconds matters:
one line reading `Loading payments-demo…` cannot tell a slow catalogue from a
description that will never evaluate. `ProductSessionService` now publishes an
`OpeningState` -- a discriminated union, not a stage beside a boolean, because the
two can disagree -- and the panel renders the four steps with the one in flight
marked. All four are shown from the start: a list that grew as it went would hide
how much is left, which is the question a wait raises. A step not yet reached is
an outline rather than a tick, because a checklist that pre-ticks its steps is a
progress bar in a costume.

The `resolve` step is the one nobody sees, and that is correct: `ProductStore.open`
sets the open product before resolving, so by then the panel is the product's own
and its `resolving…` line has taken over. The checklist covers getting *to* the
product.

A refusal stops the list at the step that refused and keeps its reason -- a
`git(...)` source and a description with no roots belong to `describe`, not to
`catalogue`, because nothing has been loaded and what is wrong is what the
description says. The message service still gets the reason, since a refusal
nobody saw looks like a hang, but the screen that was counting the steps is where
the answer belongs. It is left standing when the open returns, so it needs a way
out.

**And the way out re-opens the previous product rather than clearing the
screen.** The first step of an open re-initializes the engine on the *new*
product's folder -- that is what the `workspace` step is -- so a refusal at
`describe` or `catalogue` leaves the store holding A while the engine is pointed
at B. Dismissing alone would show A looking healthy while Resolve, an edit and
Generate went through a session configured for a product that never opened, with
B's write boundary. So `Back to A` is the same act as opening A in the first
place. With no previous product there is nothing to restore and `Dismiss` is the
whole of it.

**Three of those attributions were wrong when this first landed, and a review
found all three.** They shared a cause: the failure paths could not be reached
from a browser, so nothing checked them.

* `CatalogueStore.load` **does not reject.** It records a failure as
  `status: "error"` on its own state and returns normally, so awaiting it and
  carrying on blamed an engine that never started on whichever step failed next.
* `ProductStore.open` sets `open` in its **first** update and leaves it set on
  failure -- deliberately, so the panel can render the error beside the product it
  is about. So `open !== undefined` was true whatever happened: an unresolvable
  product reported a successful open and went into Recent, the list a person
  trusts to reopen things that worked.
* And the panel showed an open's progress only when *nothing* was open, so
  switching from one product to another showed the old one for the whole three
  seconds and hid a refusal completely. It compares identity now -- and that
  branch runs **before** the one that renders a product's error, which is the same
  mistake from the other side: a store holding a failed A answered `error` first,
  so opening B kept A's error on screen for the whole open and then in place of
  B's refusal. A product's error is the product's, and must not outlive the moment
  another product becomes the subject.

The decisions live in `browser/shell/opening-outcome.ts`, which imports nothing
but types, and the sequence in the service calls them. That split is what makes
them checkable: `ProductSessionService` cannot be constructed outside a browser --
it injects `MonacoTextModelService` and `WorkspaceService` as tokens, and loading
those in Node reaches Monaco's ESM `.css` imports -- so `scripts/store-smoke.mjs`
checks the decisions directly, and `npm run verify` runs it.

**There is no browser claim for a refused open, and that is a finding rather than
an omission.** Killing the engine looks like the way in and is not: `initialize`
spawns a new engine on every call, so an open that begins with `catalogue.load`
gets a fresh one and succeeds. Verified by trying it. A `⚪ not observed` row
would suggest a later run might see it, and none can.

What that leaves unmeasured is stated rather than glossed: the refusal *screen* --
its reason, and that `Back to A` restores A's session -- rests on the decisions
in `opening-outcome.ts` plus review, because reaching it needs two products and a
product that fails. The branch order is pinned against the source, since an
ordering bug of that shape returns silently; a second product in the corpus would
turn all of it into behaviour.

**The stage after.** Overview now reports what the product *is*, from data
`ProductStore` already holds: how many gears and how many of those nobody asked
for, how many applications and bindings, whether a generated tree exists for this
resolution, and which source roots the description declares. The figures are
buttons into the stage that can act on them, because a count with no way through
is trivia.

Two deliberate restraints. The generation status is **read from
`GenerateService`'s cache and never planned from here** -- `ensurePlan` is a round
trip, and a render that asked for one would do it on every repaint of a panel that
repaints on every store change, so "not planned for this resolution yet" is an
answer rather than a reason to go and find out. That rule is asserted against the
source rather than against the screen, because the observable version is not
sound: a plan that completed between two readings leaves the status looking
untouched, so "the status did not change" passes when the rule is broken.

The declared source roots are shown **as written and not as links**: a source root
is a directory, the link component opens a file, and a link resolving to a folder
either does nothing or opens something arbitrary inside it. The description itself
is one row above and is openable, which is where a person goes to change any of
this.

And Overview does **not** list the
diagnostics, for a reason about this stage rather than about the renderer: an
overview says what the product is, and Validation is the stage where diagnostics
are *read*. The one-line summary below already says how many there are and leads
there. (The shared row of §9.1 above removed four differing implementations of one
row; it says nothing about how many places may show a list, and citing it here
would be borrowing an argument that does not apply.)

#### Free keys are behind Advanced, and the checks that are possible happen at the field

Three findings from the third UX pass, and they share a shape: a control that was available invited
use, and the only answer came from the other side of the screen.

**A gear with no schema still offered a bare `Add key`.** So the obvious thing to do with it was
type something, and the answer was `Could not resolve the product with this gear added`. Free-form
keys now sit under an `Other keys` section -- open whenever it already holds something, because a
key somebody set is not advanced any more -- and a gear that exposes no configuration says so
instead of offering a box. The section stays, because a curated `exposes` is narrower than the
struct it came from and a key outside it may still be one the gear reads.

**The refusal said nothing.** `previewResolution` swallowed the engine's reason and the panel
invented that sentence, which is true of every failure and useful for none. The reason travels now,
and the no-toast rule is unchanged -- this runs on every keystroke's debounce, so the panel reports
it in place. Two kinds of failure end up in a configurator and they belong in different places: one
is about a field, one is about the whole proposal, and a whole-proposal failure must not be attached
to whichever row was edited last.

**And nothing was checked before the engine was asked.** What is checkable is narrower than it
looks: `ConfigFieldDecl` carries `name`, `type`, `required`, `default`, `doc` and `secret` -- no
pattern, no bounds, no format -- so only what the declaration states can be checked here. A rule
this repository does not have, enforced against a gear that accepts the value, is worse than no
check at all; that is the `prefix_path` finding, where the doc comment promised a leading slash and
`normalize_prefix_path` prepends one. So: enum membership against the engine's own variant list, and
an integer field given a fraction. Both refuse at the field, and both suppress the dry run -- the
engine's answer to an invalid value is a refusal about the whole proposal, which would sit next to a
field that already says what is wrong with it.

**One thing that looked checkable and is not**: a `required` field with nothing set. That is the
state every configurator opens in -- the panel has just been told which gear -- and the resolver may
supply the value from a profile or a default the declaration does not carry. Treating it as invalid
blocked the dry run on every proposal, which is a panel that previews nothing; it was caught by the
suite hanging on five claims at once. It is a note beside the field now, not an error.

### 9.2 Writing to a description, and the four refusals

The catalogue toggle is the only thing in Studio that writes a file a person owns, so the checks in
front of it are the substance rather than the plumbing. They run in this order, and each one refuses
instead of guessing:

1. **A product must be open**, or there is nothing to add to.
2. **The description must have no unsaved changes.** If `product.gdl` is open and modified, writing
   under the author destroys their edit -- the exact failure ADR-0010 exists to prevent, and worse
   than the one it worries about, because the tool would be the author of the loss. Saving on their
   behalf is not the answer either: that commits an edit they had not finished. So this refuses and
   names the file.
3. **The engine must agree.** `product/addGear` and `product/removeGear` take `dry_run`, and the
   preview is that call. The engine declines rather than guessing when there is no span to edit --
   no `gears` argument, or a list built by a helper instead of written literally.
4. **The person must agree**, seeing the line that will be written.

Two more refusals live in the engine, not in the client, because a client is not a security
boundary (`cpt-gearbox-fr-rpc-writes-opt-in`): a mutating call fails unless the client declared
`allow_writes` at initialize, and any path outside the declared workspace and source roots is
refused. `writable_path` fails closed. The write itself goes to a temporary file in the same
directory and is renamed, so a crash cannot leave a truncated `product.gdl`.

**The edit is span-surgical, and this is the constraint that shaped the whole feature.** The demo
description is 105 lines of which 29 are comments, and those comments carry the reasoning -- why
`vendor` is better left alone, why the grpc hub is not listed, why profiles are data. Evaluating the
file and re-serialising it would erase all of them. So `gearbox-gdl`'s editor walks the AST to the
`gears` list, computes byte offsets from `starlark_syntax::codemap::Pos`, and splices; indentation
comes from the neighbouring entries rather than from a constant. Adding a gear the description
already names is a no-op, as `members` is in ADR-0010.

The proof is the inverse: `cargo test -p gearbox-gdl` adds a gear to the real
`products/payments-demo/product.gdl`, asserts every one of the 29 comments survived and that the
file grew by exactly one line, then removes it and compares byte for byte. `npm run conformance`
does the same round trip *through the interface* and checks `git diff --stat` reports one insertion
and no deletions. A re-serialising editor could pass the first assertion and could never pass the
last.

**Configuring gears is not part of this.** `config = {…}`, per-profile plugin selection and bindings
each need their own form and their own edit; the mechanism is worth proving on the simplest case
first, and add/remove exercises all of it.

---

## 10. The slice

**Real gears (pre-existing, untouched except for an added `gear.gdl`):**

| Gear | Path | Role in the slice |
|---|---|---|
| `api-gateway` | `gears/system/api-gateway` | the only `rest_host`; its `deps` silently pull in 3 more gears — proves closure |
| `grpc-hub` | `gears/system/grpc-hub` | the only `grpc_hub`; publishes the endpoint `run_oop_spawn_phase` waits on |
| `authn-resolver` | `gears/system/authn-resolver/authn-resolver` | closure depth 2 |
| `types-registry` | `gears/system/types-registry/types-registry` | closure depth 3 |
| `service-discovery` | `gears/system/service-discovery` | `DirectoryService` server |
| `api-contracts` | `examples/toolkit/api-contracts/api-contracts` | provides `PaymentApi@v1`+`@v2` over `[local, rest]`; also the `lib_ident != gear_snake` case |
| `api-contracts-consumer` | `.../api-contracts-consumer` | two real `#[toolkit::consumes]` edges |
| `cluster` | `gears/system/cluster/cluster` | the provider registry |

**New custom gear: `payments-audit`** — `gears-rust/gears/payments-audit/{payments-audit-sdk,payments-audit}/`

```rust
#[toolkit::gear(name = "payments-audit", deps = [cluster],
                capabilities = [rest, stateful],
                lifecycle(entry = "serve", stop_timeout = "15s"))]
#[toolkit::provides(contract = payments_audit_sdk::PaymentsAuditApi,
                    local = Self::build_local, transports = [local, rest])]
#[toolkit::consumes(contract = api_contracts_sdk::PaymentApi, from = "api-contracts")]
#[derive(Default)] pub struct PaymentsAudit { /* OnceLock<Arc<AuditService>> */ }
```

`kebab(PaymentsAudit) == "payments-audit" == name`, so the `consumer_wiring` override key resolves.
The leader (via `LeaderElectionV1`) reconciles the trail every 5 s through `PaymentApi`, writing back
via `ClusterCacheV1` CAS; followers serve `trail` from cache.

Why this shape: it is the only gear that simultaneously exercises **a cuttable contract edge**
(`api-contracts ∉ closure`), **an uncuttable co-location edge** (`cluster`), **a cluster capability
requirement with a real provider decision**, and **a provided contract that becomes a
`cuttable_if_declared` candidate** for `api-contracts-consumer`. Every resolver branch in §5 is
reachable from it.

**Nine additive `gear.gdl` files** in `gears-rust` (one per slice gear), plus two `Cargo.toml`
workspace-member entries. Nothing else in that repo changes.

---

## 11. Milestones

| # | Deliverable | Verification | Parallel |
|---|---|---|---|
| **M0** | **PRD** (§14) — short, `docs/PRD.md` | reviewed against `gears-rust/docs/checklists/PRD.md`; every FR/NFR has an ID and a p-tier; every acceptance criterion maps to a §12 step | — |
| **M1** — **done** | Workspace + IR + lock | `cargo test -p gearbox-ir -p gearbox-lock` incl. a proptest asserting byte-stability over 1000 shuffled input orderings; `make ts && git diff --exit-code` | — |
| **M2** — **done** | GDL evaluator | `gearbox catalogue --root ../gears-rust --format json \| jq '.gears \| length'` == 14; a fixture per GBX01xx code; dialect + blacklist + `load()` sandbox tests | M3 |
| **M3** — **done** | `gearbox validate` | `gearbox validate --root ../gears-rust` → 0 errors, and with `--product` → 0 errors on the real product; GBX0208 and GBX0301 each proved against the real tree (`bss-ledger` is undescribed, `api-gatewey` is a typo); GBX0209 proved on temporary trees because the repository has no wrong declaration to point at; GBX0207 retired with a differential test in its place | M2, M8a |
| **M4** — **done** | Resolver + explain + lock | all three profiles diff clean against `fixtures/*/product.lock`; every GBX03xx–06xx code reachable; determinism loop | M8a |
| **M5** — **done** | Crate + config generators; **embedded runs** | acceptance §12 step 2 in full | — |
| **M6** — **done** | Self-hosted | `make oop-run`: the generated host starts the generated worker, the worker serves its own probes, and the `PaymentApi@v1` binding resolves **remote through the directory**. `payments-audit` was never needed -- the corpus supplied a severable pair. The run now also starts
the cluster gear against a real postgres, because `api-contracts-consumer` requires the
`event-broker` scope and `local` binds it to that backend: the backend runs its migrations and
creates `cluster.cluster_cache` and `cluster.cluster_lock`. Two generator defects had to be fixed
before it would start, and neither was reachable until something required a scope -- `secret_ref`
was written as a string where the gear reads a `SecretRef { name }`, and a `serde_json::Number`
leaked its private representation into the YAML. The evidence is `readiness: dependency resolved`, not the `wire_outcome=Remote` line §12 named: that one is DEBUG and unreachable (the runtime builds its filter from the configuration's `logging` targets and `RUST_LOG` only caps it), while the readiness line is emitted **only** for a directory-resolved remote -- a local or statically-overridden dependency is marked resolved without ever reaching that loop | M7 |
| **M7** — **done** | Docker + Helm + `values.schema.json` | acceptance §12 step 4 minus `kubeconform`/`kind` (neither is installed); `helm lint`/`helm template`, schema `--set` rejection, GBX0603, ConfigMap `api-contracts` | M6 |
| **M8a** — **done** | JSON-RPC + TS types | `node ide/scripts/rpc-smoke.mjs` drives initialize → catalogue over real framing, 15/15; `cargo test -p gearbox-rpc`; stdout carries nothing but JSON-RPC | from M1 |
| **M8b** — **partly done** (§9.1) | Theia Studio | Catalogue, Inspector, Graph, Product, Conflicts, Lock, Generate, Start, the product header and the two working contexts are built and checked headlessly: `cd ide && npm run verify`. Conformance against the documents is generated into `docs/conformance.md`. Electron is still open | after M4 + M8a |
| **M9** | **DESIGN + ADRs** (§14) — written *after* the prototype runs | reviewed against `docs/checklists/{DESIGN,ADR}.md`; every claim cites either a `gearbox` symbol or a `gears-rust` `file:line`; every §13 gap has a home | — |

**What M6 found by running, and could not have found any other way.**

- **The worker's executable path was computed from the wrong base.** The resolver copied
  `target_dir` -- which the description expresses relative to *itself* -- into `executable_path`,
  and the runtime resolves that relative to the host's working directory, which is the generated
  tree. The two differ by one level, so the host looked for the worker in a directory that does not
  exist. The path is a fact about where the tree landed, not about the resolution, so it moved:
  `SpawnSpec` carries `bin_name`, `target_dir` travels as a profile setting, and the generator
  composes the path with the same function that writes `build.target-dir` into a generated
  `.cargo/config.toml`. One function, so the two cannot disagree again.

- **`plugin(config = {...})` was discarded in full.** `resolve::product` read `selected_gears` and
  never descended into `plugins`, so a plugin's configuration reached neither the lock nor the
  generated YAML. The demo product set `issuer` on `oidc-authn-plugin` and the file carried
  `config: {}`; the host then refused to start on a field the description had supplied. Suspected
  during the `cargo-gears` comparison, proved by a product that would not run.

- **A profile can declare a plugin that cannot start there, and nothing says so.**
  `OidcAuthNGearConfig.jwt` has no default and `oidc-authn-plugin` does not expose it, so **no
  description can supply it** -- `local` now takes the static plugin, as `dev` does. The general
  form is a limit of the curation model: `exposes` is a subset, a required field may be left out of
  it, and then `ConfigFieldDecl.required` never sees the field it would have complained about.

**Three things M5 left behind, recorded here because nothing else covers them.**

- **`ResolvedApplication.cargo_features` is written and never read.** `partition.rs` fills it and no
  generator consults it, so it is dead weight in every lock — and its shape is wrong anyway: a flat
  `BTreeSet<String>` across every crate in the process, when a Cargo feature belongs to a specific
  dependency. Two crates asking for a feature of the same name are indistinguishable in it. Either
  it becomes per-crate and something uses it, or it goes; leaving it is the one option that costs
  lock bytes for nothing.
- **The real-tree tests skipped silently from a git worktree — fixed, and the workaround that hid it
  is gone.** They located the corpus as `$CARGO_MANIFEST_DIR/../../../gears-rust`, the sibling of the
  *repository* root, which from `.claude/worktrees/<name>/crates/…` resolves to nothing. An agent in
  a worktree ran the suite, saw green, and had exercised none of the real corpus.

  What made this hard to see is that it *appeared* to work: someone had created
  `.claude/worktrees/gears-rust` as a symlink to the real checkout, so the counted `..` happened to
  land on it. A machine-local symlink, invisible to git, standing in for a path the code got wrong.
  Demonstrated by moving it aside: at the old code the test printed
  `skipping: ../gears-rust not present`; with the locator fixed it ran.

  The locator now walks up until it finds a `gears-rust` containing `gears/`, which works from either
  layout and stops at the filesystem root instead of guessing a depth. It is one function in
  `gearbox-project` (`src/test_corpus.rs`, used by six test modules) and the same body inlined in the
  ten `gearbox-engine` integration tests, because those cannot share a module without each declaring
  one. **That duplication is the remaining debt here** — ten identical copies, correct but repeated.
  The symlink has been deleted; nothing needs it.
- **Open question, and it needs an ADR rather than a decision in passing: may a product draw gears
  from more than one source root?** `ProductIntent.sources` is a map, so the IR says yes, and nothing
  yet says what it means for two roots to offer the same `GearId`, or which root wins, or whether
  that is an error. The generators assumed one root without saying so.

Critical path M0 → M1 → M2 → M4 → M5 → M6 → M9. M8a needs only types, so its widgets can be
stubbed against `fixtures/*/product.lock` until M4 lands.

**Sequencing constraint worth stating explicitly:** M6 requires writing real Rust in `gears-rust`
that must compile and pass its own integration test **before** the generator is trusted. Do it by
hand first with a throwaway app crate modelled on
`examples/toolkit/api-contracts/api-contracts-consumer/tests/provider_consumer.rs`, then delete the
throwaway and let the generator reproduce it. Otherwise a generator bug and a gear bug are
indistinguishable.

---

## 12. Verification

Prerequisites: `rustup toolchain install 1.97.0`; `brew install kubeconform kind yq`
(`helm`, `docker`, `jq` are already present).

**Step 0 — baseline.** `make oop-example` is stale; re-establish Profile 2 by hand
(`cargo build -p calculator --features oop_module`, then run the example server with
`config/oop-example-master+follower.yaml` and curl the calculator route) before trusting any
generated output.

**Step 1 — validate.** Done and green:
```bash
gearbox validate --root ../gears-rust                                  # 0 errors
gearbox validate --root ../gears-rust --product products/payments-demo/product.gdl
# GBX0208: a real gear with no description, with the cargo(...) line to write
gearbox validate --root ../gears-rust --product <product selecting "bss-ledger">
# GBX0301: nothing declares it, with a spelling hint when one is close
gearbox validate --root ../gears-rust --product <product selecting "api-gatewey">
```
Then the projection checks ADR 0002 needs (all cheap, all offline):

```bash
# every projected field really came from Rust: strip the attributes' values from a
# copy of the tree, re-run, and assert the catalogue changes rather than not noticing
gearbox catalogue --root ../gears-rust --format json > /tmp/base.json

# restatement is refused, once per projected field
for f in id runtime_caps colocated_deps lifecycle client_trait; do
  gearbox validate --root fixtures/negative/restate-$f 2>&1 | grep -q GBX0210 || echo "MISS $f"
done

# the attribute locator: mini-chat is the positive case (3 gears, 1 crate)
gearbox catalogue --root ../gears-rust --format json   | jq -r '.gears | keys[] | select(startswith("static-mini-chat") or . == "mini-chat")' | sort   | diff - <(printf 'mini-chat\nstatic-mini-chat-audit-plugin\nstatic-mini-chat-model-policy-plugin\n')
gearbox validate --root fixtures/negative/attr-ambiguous 2>&1 | grep -q GBX0211
gearbox validate --root fixtures/negative/attr-missing   2>&1 | grep -q GBX0211
```

The load-bearing one is the `mini-chat` diff: it is the only case in the repository that proves the
locator does real work rather than defaulting its way to a right answer.

**Step 2 — embedded.** Resolve + generate + `cargo build --bin gbx-api-gateway`, then the oracles:
```bash
./target/debug/gbx-api-gateway --list-registered-gears | sort \
  | diff - <(gearbox lock gears --application api-gateway --order name --with-deps)
./target/debug/gbx-api-gateway --config config/api-gateway.yaml --dump-gears-config-yaml \
  | diff - fixtures/dev/effective-gears.yaml
```
**The first oracle used to be flaky, and the flakiness was the plan's, not the code's.** It diffed
against `--order topo`, but the runtime's order is *a* topological order and not a canonical one:
`GearRegistry::build_dependency_graph` seeds Kahn's algorithm from `self.core.keys()` on a `HashMap`
(`gears-rust/libs/toolkit/src/registry.rs:552`), so two runs of the same binary legitimately disagree
about gears that do not depend on one another. Demonstrated: two consecutive runs of
`gbx-api-gateway` gave `types-registry api-contracts grpc-hub …` and
`api-contracts grpc-hub types-registry …`. The generated `main.rs` already says so in the doc comment
above `list_registered_gears` -- "Compare sorted output, not this order" -- so the plan was
contradicting the generator it was checking.

`--order name --with-deps` is not merely the stable form, it is the **stronger** one: both sides emit
`name<TAB>deps`, so the comparison covers the dependency edges the linker produced and not just the
set of names. Verified stable over three consecutive runs.
Run it; assert both REST surfaces answer and `WireOutcome::Local` appears for all three bindings
with no readiness gate.

**Step 3 — self-hosted.** `make oop-run`. **Done**, and the step as first written could not have
been: it named `gbx-payments-audit`, a gear nobody wrote. It was also right about Postgres for the wrong
reason: at the time no gear demanded a cluster primitive, so the declared postgres profile resolved
to nothing. `api-contracts-consumer` now requires the `event-broker` scope, `local` binds it to
postgres, and the step needs a reachable database -- `PG_HOST` and `PG_PASSWORD` in the
environment. The directory is still an in-memory map inside the host.

Against what the corpus actually supplies: host `gateway`, worker `api-contracts` on
`127.0.0.1:8090`. Asserts that the binary the host is configured to start is the one the build
produced, that the worker is running, that it answers `/readyz`, that the host logs
`readiness: dependency resolved dep=api-contracts`, and that the host's own `/readyz` opens --
which it cannot until that remote dependency resolves.

The last assertion is the one worth reading twice. `wire_outcome=Remote` is a DEBUG record and
DEBUG is unreachable here, so the proof is the readiness line instead: `host_runtime` marks a
dependency resolved immediately when the implementation is local, and immediately again when a
static override stands in for discovery; **only** a binding that must be looked up in the directory
joins the background probe list that line comes from.

**Step 4 — kubernetes.** `helm lint` + `helm template` + `kubeconform`, then the hard assertions:
```bash
! grep -qrE '\blookup\b|randAlphaNum|genCA' payments-demo/templates payments-demo/charts
! grep -qiE 'password:|apiKey:' payments-demo/values.yaml payments-demo/values.generated.yaml
yq '.. | select(has("consumer_wiring")).consumer_wiring' /tmp/rendered.yaml | grep -q 'api-contracts:'
! grep -q 'APP__GEARS__.*CONSUMER_WIRING' /tmp/rendered.yaml     # grounded fact: env can't express it
! helm template payments-demo payments-demo --set audit.replicaCount=two   # schema rejects garbage
gearbox resolve … --format json | jq -e '.diagnostics[] | select(.code=="GBX0603")'
```
Optional: `kind` + `kind load docker-image` + `helm install --wait`.

**Step 5 — negative cluster case.** **Executable now**, and executed: add `cluster_cap.prefix_watch`
to the cache requirement in `api-contracts-consumer/gear.gdl` and the resolve exits non-zero with no
lock and `GBX0502`, naming the provider that cannot answer (`postgres: missing
cluster.cache.prefix-watch`). `GBX0503` is the neighbouring case -- `standalone` is process-local and
`audit` has replicas=2 -- reachable by binding dev's provider in a spread profile.

**Step 6 — determinism.** Resolve three times → one distinct `blake3`. Apply generate twice →
`git status --porcelain` empty the second time.

**Step 7 — gear src unchanged (the whole point).**
```bash
cd ../gears-rust
test -z "$(git status --porcelain -- ':!**/gear.gdl' ':!gears/payments-audit' \
                                    ':!Cargo.toml' ':!Cargo.lock')"
for p in gears/system/api-gateway gears/system/grpc-hub gears/system/service-discovery \
         gears/system/authn-resolver gears/system/types-registry gears/system/cluster \
         examples/toolkit/api-contracts; do
  git diff --exit-code HEAD -- "$p/**/*.rs" "$p/**/Cargo.toml" || { echo "MUTATED: $p"; exit 1; }
done
git diff --exit-code HEAD -- gears/payments-audit/payments-audit/src   # same src, 3 topologies
```

**Step 8 — TS anti-drift.** `make ts && git diff --exit-code ide/.../generated`;
`gearbox rpc-schema | diff - fixtures/rpc-schema.json`.

**Step 9 — Studio.** `cd ide && npm ci && npm run verify` (rpc smoke, build, then the conformance
suite, which starts the application itself). The catalogue, detail and co-location graph assertions
are automated and green; the ones below still need M4's widgets, and each is a named `test.fixme` in
`ide/tests/conformance/` rather than prose to be checked by hand. Catalogue lists **14** gears --
this step said 9, which contradicted ADR-0009 and §9.1, and 14 is what the corpus has; the profile
dropdown has dev/local/prod; switching to prod surfaces
GBX0603 + GBX0507 in Problems; the Graph "applications" view shows 2 boxes with `cluster` inside
`audit`; clicking the `payments-audit → api-contracts` edge opens Explain with the DowngradedBy
narrative; Generate shows 0 conflicts, and after hand-editing `values.yaml` a re-apply reports it
`unchanged` (the 3-way merge preserved it).

---

## 13. Honest gaps — what this will NOT prove

1. **Multi-host anything.** `LocalProcessBackend` is the only spawn backend (GBX0604).
2. **A real Kubernetes control loop.** Nothing in the runtime knows about K8s. Helm output is a
   static manifest set; the only k8s-aware behaviour is Service DNS pinned into a ConfigMap and
   optionally `POD_IP` from the downward API.
3. **gRPC across a process boundary via declared contracts.** `#[toolkit::consumes]` emits a REST
   client only; GBX0402 downgrades rather than emitting config that silently does nothing.
4. **Roles, shards, per-instance addressing.** GBX0318/0602 refuse to pretend; `declared_roles` is
   stored for forward-compat and contributes nothing to the lock.
5. **Cluster coordination beyond `standalone` + `postgres`.** `redis` registers a cache and a lock
   now, but declares no capability: it reads its consistency off the server it connects to, so
   GBX0520 says so and it satisfies only a requirement asking for none. Leader election still has
   *zero* registered providers and always resolves to the SDK CAS default.
6. **`prefix_watch` in any distributed setting.** GBX0502 is the correct answer, not a workaround.
7. **The cluster gear in a running product** — ~~it is wired into no runnable app today~~. Done, and
   the friction was where this predicted it: `ClusterProfile` scope naming (the marker is the join
   key, and GBX0508 is what catches a name nothing implements) and the shape of the generated
   configuration (two defects, neither reachable until a scope was required). What remains untried
   is a *consumer* resolving the facades: the requester declares the marker and the backend runs,
   but nothing calls `ClusterCacheV1::resolver(hub)` yet.
8. **Cutting the interesting real-world edges.** 33 of 39 gears use `deps`; `types_registry` is
   pulled by ~22. The resolver never cuts an undeclared edge — it *reports* it with the literal
   annotation to add. That report is the deliverable; actually cutting those edges needs the
   annotations to exist first.
9. **Byte-identical Rust vs hand-written references.** Generated `registered_gears.rs` has no
   `#[cfg(feature)]` gates by design; validation is the `--list-registered-gears` oracle, not `diff`.
10. **Speed.** Path-deps into a large repo; a cold generated build is minutes and `docker build`
    needs a context spanning both repos.
11. **Starlark API churn.** `starlark-rust` is pre-1.0; pin exactly and let nothing outside
    `gearbox-gdl` name a `starlark::` type.
12. **The `runtime.type: oop` double-instantiation hole.** The registry has no runtime-kind
    awareness, so a gear both linked into the host *and* marked `oop` is instantiated locally **and**
    spawned as a child — which is what `calculator` does today. The generator sidesteps this by
    never linking a worker's anchor crate into the host, which in turn means the host cannot declare
    `deps = [<worker anchor>]` or `build_topo_sorted` hard-fails. This is a genuine structural limit
    of Profile 2: **a gear can move out of process only if nothing left in the host declares it as a
    `deps` target.** The resolver enforces it (that *is* `CutBlocker::ColocationClosure`), so
    `calculator-gateway`-shaped designs cannot be reproduced by the generator — and should not be,
    because they are the double-instantiation bug.
13. **A product in the user's own folder.** New Product and Clone write only inside the workspace
    the Studio declared, which is the repository (ADR-0013 §Where the file is created, and its
    creation boundary, Amendment 2026-09-16). A product in a folder of the user's choosing, which
    is how a user would expect to work, is refused. Opening a product from anywhere by file works;
    creating one there does not. This is a known limit, recorded 2026-09-29 at the user's request.
    The fix is deliberately postponed: widening the boundary means deciding what the session's
    workspace and source roots are for a product outside the repository.

Every "not supported" diagnostic (GBX0402, 0409, 0505, 0601–0606) carries an `evidence` field with a
real `file:line` in `gears-rust`, so a reader can verify the claim in ten seconds instead of
trusting the tool.

---

## 14. Spec artefacts — PRD now, DESIGN after

Both live in `gearbox/docs/` and follow `gears-rust/docs/spec-templates/gears-sdlc/`
verbatim, so a Gears reviewer needs no context switch. `system` slug = **`gearbox`**, giving IDs
`cpt-gearbox-fr-…`, `-nfr-…`, `-actor-…`, `-usecase-…`, `-design-…`, `-adr-…`. Priority tiers are
`p1`/`p2`/`p3` (the templates forbid SHOULD/MAY — use a tier instead), requirement text uses
**MUST**, and the TOC placeholder is filled by `cfs toc`. No `UPSTREAM_REQS.md` exists for this
system, so the `Covers:` field is omitted throughout.

### M0 — `docs/PRD.md` (before any code)

Short on purpose: it is an alignment artefact, not a substitute for the vision doc. Sections per
the template, with the content already derived above:

| Template section | Content source |
|---|---|
| 1 Overview / Problem / Goals | vision §1–3, §119–122; the goal is "configure intent, derive implementation, explain every decision" |
| 1.4 Glossary | Catalogue / Intent / ResolvedProduct / DeploymentProfile / ClusterScope / co-location vs contract consumption — the terms the vision insists must not be conflated (§14, §24–26, §30) |
| 2 Actors | `cpt-gearbox-actor-gear-author`, `-integrator`, `-platform-engineer`, `-external-agent` (via RPC/MCP), `-theia-studio` (system actor), `-cargo` / `-helm` (system actors) |
| 3 Operational Concept | offline, no cluster access at render time, no network at resolve time, deterministic |
| 4 Scope | in: GDL/resolver/lock/generators/RPC/Studio for the §10 slice. out: MCP, TUI, Rego, registry sources, SAT/SMT, scoring, migration tooling, CI, mass gear migration |
| 5 FRs | one per resolver responsibility and generator output — e.g. `-fr-gdl-declarative` (§3.2), `-fr-derive-binding-from-placement`, `-fr-never-cut-undeclared-edge`, `-fr-report-cuttable-if-declared`, `-fr-cluster-capability-match`, `-fr-generate-application-crate`, `-fr-values-schema`, `-fr-no-secrets-in-values`, `-fr-diagnose-unsupported` |
| 6 NFRs | `-nfr-determinism` (same input ⇒ byte-identical lock), `-nfr-explainability` (every automatic choice answers "why" without an LLM), `-nfr-provenance-during-resolution`, `-nfr-render-without-cluster`, `-nfr-operator-values-preserved`, `-nfr-engine-has-no-frontend-deps`, `-nfr-evidence-cited` (every "unsupported" diagnostic carries a real `file:line`) |
| 7 Public Library Interfaces | the JSON-RPC surface (§8) + the `gearbox` CLI verb set — this is the external contract |
| 8 Use Cases | `-usecase-switch-profile` (the canonical one), `-usecase-explain-provider-choice`, `-usecase-diagnose-invalid-topology` |
| 9 Acceptance Criteria | **each criterion maps 1:1 to a §12 verification step**, so the PRD is falsifiable rather than aspirational |
| 10–13 Deps / Assumptions / Risks / Open Questions | §13 gaps become Risks; the OLD doc's "Key Assumptions to Validate" list becomes Assumptions; vision §118's 20 open questions are filtered to the ones this slice actually answers |

Deliberately **not** in the PRD: crate layout, IR types, resolver algorithm, GDL syntax — those are
DESIGN, and writing them now would be guessing.

### M9 — `docs/DESIGN.md` + `docs/ADR/` (after the prototype runs)

Written last, so every statement is a report rather than a forecast. Sections map onto this plan:
§1.1–1.3 ← §2 (crate DAG, layering, the engine/frontend boundary test); §2 ← §2.2 design rules;
§3.1 Domain Model ← §4 IR; §3.2 Component Model ← §2.1 + §5 resolver stages; §3.3 API Contracts ←
§8 RPC + the GDL host API; §3.5 External Dependencies ← the pinned crate list *with the versions
that actually worked*; §3.6 Interactions ← the resolve and generate sequences; §3.8 Deployment
Topology ← §7 generators and the three profiles. `product.lock`'s schema is the "data model", so
§6 of this plan graduates into DESIGN §3.1 rather than being restated.

ADRs only where the rationale genuinely needs recording (the template warns against "everything is
a decision"). The set worth writing, as `docs/ADR/NNNN-cpt-gearbox-adr-<slug>.md`.
**0002, 0009, 0010 and 0011 are the exceptions to "after the prototype runs"** — each decides what a
later milestone builds, so each is
written before it: 0002 what M2 builds, 0009 what M8 builds, 0010 what M5's writer is allowed to
touch, and 0011 what the Studio becomes. The remaining numbering below is kept as reserved slots:

| # | Slug | The dilemma |
|---|---|---|
| 0001 | `gdl-starlark-over-toml` | vision says Starlark; the repo-grounded predecessor says TOML + syn and calls Starlark "a language subsystem with no consumer". Record why the prototype chose Starlark *and* what would justify reverting. |
| 0002 | `macro-projected-catalogue` | **Written already**, ahead of M2 rather than at M9, because it constrains the GDL surface and the scanner before either exists — see `docs/ADR/0002-cpt-gearbox-adr-macro-projected-catalogue.md`. Records why the macro keeps every fact it already expresses, why GDL declares only the disjoint remainder, and the three rejected alternatives (GDL-authoritative-with-cross-check, GDL-generates-the-annotations, macro-only). Includes the three-confidence-level model that was rejected. |
| 0003 | `colocation-is-a-closure-not-a-partition` | the `MissingDeps` finding, why `deps` edges are uncuttable, and why applications overlap. The most consequential correction to the vision. |
| 0004 | `product-lock-canonical-serialization` | TOML + `blake3` over the body, `lock_hash` elided; why arrays-of-tables and BTreeMap iteration are load-bearing, not cosmetic. |
| 0005 | `rpc-jsonrpc-stdio-lsp-framing` | stdio LSP framing vs local HTTP vs WASM; why one server backs both the Studio and the `.gdl` language client. |
| 0006 | `template-text-serialize-data` | minijinja for text, serde for data, `<< >>` for Helm sources; the YAML-indentation failure mode being avoided. |
| 0007 | `k8s-static-endpoint-resolution` | no K8s-DNS `EndpointResolver` exists; ConfigMap-pinned `consumer_wiring` + GBX0603 vs waiting for a runtime feature vs faking it. |
| 0008 | `unsupported-is-a-diagnostic-not-an-omission` | roles/shards/providers/profiles are parsed and then explicitly refused with cited evidence, rather than being absent from the grammar. |
| 0009 | `staged-catalogue-loading` | **Written already**, because it constrains the RPC surface and the Catalogue widget before either exists. Five stages with measured costs; why "not yet computed" is a separate `pending` list rather than tri-state fields or a `stage` on the gear; and why the projected `GearId` makes staging a requirement rather than an improvement — see `docs/ADR/0009-cpt-gearbox-adr-staged-catalogue-loading.md`. |
| 0010 | `authoring-ownership-tiers` | **Written already**, because it decides what M5's writer is permitted to do and what PRD §4.2 says, both of which precede the code. Six ownership tiers surveyed across Cargo, Kubebuilder, Angular, Rails, dotnet, Yeoman, Copier, Gazelle and Arduino IDE; why creating a new crate leaves the single-authorship invariant intact while editing an existing attribute would destroy it; and why marker-based injection is not needed here — see `docs/ADR/0010-cpt-gearbox-adr-authoring-ownership-tiers.md`. |
| 0011 | `domain-specific-ide-shell` | **Written already**, because it constrains the Studio's shape before the reshaping starts. Why Theia is narrowed by dependency set *and* by rebinding rather than by either alone; the mechanism table against 1.75, where Arduino IDE's is 1.57; why `.gdl` highlighting is contributed natively rather than as a VSIX; and the two perspectives — see `docs/ADR/0011-cpt-gearbox-adr-domain-specific-ide-shell.md`. |

Each ADR's Traceability section links back to the `cpt-gearbox-fr-*`/`-nfr-*` IDs from M0 and the
`-design-*` elements from M9, and each Confirmation section names the test that enforces it (e.g.
0003 → the `--list-registered-gears` oracle; 0004 → the byte-stability proptest; 0008 → the
per-diagnostic fixtures).

---

## Critical files

- [docs/vision.md](../vision.md) — §12 (declarative rule), §22
  (lock contents), §47–48 (application crates), §51–58 (templating + Helm) are the normative constraints
- `gears-rust/libs/toolkit/src/registry.rs` — `Registrator`, `build_topo_sorted`, `MissingDeps`;
  why `deps` is uncuttable and the source of the `--list-registered-gears` oracle
- `gears-rust/libs/toolkit/src/runtime/host_runtime.rs` — lifecycle phase order, `consumer_wiring`
  static override, `WireOutcome` consumption, `run_oop_spawn_phase`
- `gears-rust/libs/toolkit/src/bootstrap/config/mod.rs` — `AppConfig`, `GearRuntime`,
  `ExecutionConfig`, `OopHttpConfig`, and `remap_gear_env_key`
- `gears-rust/libs/toolkit-contract-macros/src/consumes.rs` — REST-only resolving client,
  `owner_gear` from struct ident, no topo dep
- `gears-rust/gears/system/cluster/cluster/src/gear.rs` — `provider_registry()`, the hardcoded set
  the resolver matches against
- `gears-rust/gears/mini-chat/deploy/{helm/mini-chat,docker/mini-chat.Dockerfile}` — the only chart
  and container build in the repo; golden references
- `cargo-gears/design/ideas/gears-product-configurator-OLD.md` — the repo-grounded predecessor whose
  conclusions (co-location collapse, "unannotated is never split", template-text/serialize-data,
  Helm must-haves) are folded into this plan
- `gears-rust/docs/spec-templates/gears-sdlc/{PRD,DESIGN,ADR}/template.md` and
  `gears-rust/docs/checklists/{PRD,DESIGN,ADR}.md` — the exact form and review bar for §14
