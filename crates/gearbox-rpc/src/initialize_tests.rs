//! What `initialize` does to the cached catalogue, and why the empty case is
//! safe.
//!
//! **Written because an audit reported the empty-roots branch as a bug and it is
//! not one**, and nothing in the tree said so. `state.roots` is assigned in
//! exactly one place and `state.catalogue` is cleared in exactly one, and those
//! two statements share an `if` on purpose: the cache therefore cannot outlive a
//! change of roots. That is a property of where two lines sit, which is the
//! least durable kind of invariant there is — nobody would notice if one of them
//! moved, and the next reader would file the same report.
//!
//! So the decisions are pinned here rather than argued in a comment. Each test
//! is one sentence of the contract:
//!
//!   * naming no roots means "keep what you have", including the catalogue,
//!     because the catalogue still belongs to the roots that did not change;
//!   * naming different roots drops it, which is what the branch exists for;
//!   * naming the *same* roots drops it too, deliberately.
//!
//! And the load's own guard: a staged load that stopped early must not be
//! cached, or `resolve` reads a catalogue missing most of its gears.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "clippy.toml's allow-unwrap-in-tests covers #[test] fns but not the helpers here"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use lsp_server::{Connection, RequestId};

use super::*;
use crate::protocol::InitializeParams;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A directory that exists, so `open_roots` yields a root rather than a failure.
fn scratch(label: &str) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under the workspace");
    let nth = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = repo.join(format!(
        "target/gbx-initialize-{label}-{}-{nth}",
        std::process::id()
    ));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

/// A catalogue distinguishable from a freshly loaded one.
///
/// Identity is all these tests need: the question is whether *this* value
/// survives, not what is in it.
fn marked_catalogue() -> gearbox_ir::Catalogue {
    let mut catalogue = gearbox_ir::Catalogue::default();
    catalogue.sources.insert(
        gearbox_ir::SourceId::new("marker").unwrap(),
        gearbox_ir::ResolvedSource {
            id: gearbox_ir::SourceId::new("marker").unwrap(),
            kind: gearbox_ir::SourceKind::Path,
            location: "/marker".to_owned(),
            digest: String::new(),
        },
    );
    catalogue
}

fn state_with(root: &Path) -> State {
    let (roots, failed_roots) = open_roots(&[root.to_path_buf()]);
    State {
        roots,
        registry_plans: BTreeMap::new(),
        catalogue: Some(marked_catalogue()),
        failed_roots,
        creation_boundary: None,
        initialized: false,
        allow_writes: false,
        workspace: None,
        documents: BTreeMap::new(),
    }
}

fn params(roots: &[&Path]) -> InitializeParams {
    InitializeParams {
        roots: roots.iter().map(|r| r.display().to_string()).collect(),
        allow_writes: false,
        workspace: None,
        creation_boundary: None,
    }
}

fn is_marked(state: &State) -> bool {
    state.catalogue.as_ref().is_some_and(|c| {
        c.sources
            .contains_key(&gearbox_ir::SourceId::new("marker").unwrap())
    })
}

/// Naming no roots keeps them, and keeps the catalogue with them.
///
/// The case reported as a bug. It is not: the roots did not change, so the
/// cached catalogue is still the catalogue *of these roots*. Clearing it here
/// would throw away a valid cache and force the next `resolve` through a full
/// rescan, which is a cost with nothing bought.
///
/// Nor can a client even reach it today — the Studio backend substitutes the CLI
/// defaults for an empty list before the wire. That makes this a statement about
/// the protocol rather than about a caller, which is the reason to write it down.
#[test]
fn naming_no_roots_keeps_the_roots_and_the_catalogue() {
    let dir = scratch("empty");
    let mut state = state_with(&dir);
    let before: Vec<_> = state.roots.iter().map(|r| r.id.clone()).collect();

    let response = initialize(&mut state, RequestId::from(1), &params(&[]));
    assert!(response.response_result.is_ok(), "{response:?}");

    assert_eq!(
        state.roots.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        before,
        "an empty `roots` means keep them, not open nothing"
    );
    assert!(
        is_marked(&state),
        "the catalogue belongs to roots that did not change, so it survives"
    );
}

/// Naming different roots drops the catalogue. The branch's whole purpose.
///
/// A reconnecting client sends a second `initialize`, and before the clear
/// existed the rest of that session was answered out of a cache built from the
/// first tree.
#[test]
fn naming_different_roots_drops_the_catalogue() {
    let first = scratch("first");
    let second = scratch("second");
    let mut state = state_with(&first);

    let response = initialize(&mut state, RequestId::from(1), &params(&[&second]));
    assert!(response.response_result.is_ok(), "{response:?}");

    assert!(
        !is_marked(&state),
        "a catalogue must never outlive the roots it was scanned from"
    );
    assert_eq!(state.roots.len(), 1);
    assert!(state.roots[0].root.ends_with(second.file_name().unwrap()));
}

/// Naming the *same* roots drops it too, and that is intended.
///
/// Telling "the same roots" from "different roots that happen to resolve alike"
/// needs a comparison this handler does not do, and a spurious rescan is the
/// cheap side of that trade. Pinned so the rescan is not read later as an
/// oversight and "optimised" into the staleness the previous test describes.
#[test]
fn naming_the_same_roots_drops_the_catalogue_anyway() {
    let dir = scratch("same");
    let mut state = state_with(&dir);

    let response = initialize(&mut state, RequestId::from(1), &params(&[&dir]));
    assert!(response.response_result.is_ok(), "{response:?}");

    assert!(
        !is_marked(&state),
        "naming roots at all re-opens them, and the cache goes with them"
    );
}

/// A root that cannot be opened is reported, with its cause, in the result.
///
/// **Both failure branches of `open_roots`, and the field that carries them.**
/// Nothing opened a missing directory or a directory whose name is not a usable
/// source id, so neither `FailedRoot` push ran and `failed_roots` was asserted
/// nowhere -- a change back to the `.ok()` this replaced would have kept every
/// test here green while roots vanished from the list with nothing reported to
/// the client. The client is the only party that can fix a bad root, so the
/// cause has to reach it rather than the log.
#[test]
fn a_root_that_cannot_be_opened_is_reported_with_its_cause() {
    let dir = scratch("failed");
    let missing = dir.join("not-here");
    // The id is derived from the directory's own name, so an underscore in it is
    // the shape `SourceId` refuses -- the other branch, reached without the
    // filesystem having anything to do with it.
    let unusable_id = dir.join("bad_root");
    std::fs::create_dir_all(&unusable_id).unwrap();

    let mut state = state_with(&dir);
    let response = initialize(
        &mut state,
        RequestId::from(1),
        &params(&[&missing, &unusable_id]),
    );
    let value = response
        .response_result
        .expect("a bad root is reported, not fatal");
    let result: InitializeResult =
        serde_json::from_value(value).expect("the initialize result decodes");

    assert!(
        result.roots.is_empty(),
        "neither root opened, so none is claimed: {:?}",
        result.roots
    );
    let reported: Vec<String> = result
        .failed_roots
        .iter()
        .map(|failed| format!("{}: {}", failed.path, failed.error))
        .collect();
    assert_eq!(
        reported.len(),
        2,
        "both failures must be named, not the shorter list: {reported:?}"
    );
    assert!(
        reported.iter().any(|line| line.contains("not-here")
            && (line.contains("does not exist") || line.contains("exist"))),
        "the missing directory and why: {reported:?}"
    );
    assert!(
        reported
            .iter()
            .any(|line| line.contains("bad_root") && line.contains("kebab")),
        "the unusable source id and why: {reported:?}"
    );

    // And the session is then in the state the guards are about: no root is open.
    let refusal = require_ready(&state, &RequestId::from(2)).expect("no root is open");
    let message = match refusal.response_result {
        Err(e) => e.message,
        Ok(_) => panic!("a session with no open root cannot serve a request"),
    };
    assert!(
        message.contains("bad_root") && message.contains("not-here"),
        "the guard repeats the causes, so a client that ignored the result still learns \
         them: {message}"
    );
}

/// A completed staged load fills the cache every later answer is built from.
///
/// The positive half of `a_load_that_stopped_early_is_not_cached`, which asserts
/// `state.catalogue.is_none()` -- also true of a load that never cached at all,
/// so on its own it could not tell the guard from the bug. "It was never being
/// filled" is a regression this repository has already had: the first
/// `product/resolve` after a load found `None` and rescanned the whole tree
/// non-staged, for a catalogue the client already had on screen.
#[test]
fn a_completed_load_is_cached() {
    let dir = scratch("cached");
    // A crate as well as a description: the second pass projects the gear from
    // `#[toolkit::gear]`, and a description with nothing behind it contributes a
    // diagnostic instead of a gear -- which would leave this asserting about an
    // empty catalogue.
    let crate_dir = dir.join("demo");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(
        crate_dir.join("gear.gdl"),
        "gear(\n  maturity = \"stable\",\n  name = \"Demo\",\n  category = \"example\",\n  \
         package = cargo(crate_name = \"demo\", lib = \"demo\", path = \".\"),\n)\n",
    )
    .unwrap();
    std::fs::write(
        crate_dir.join("src/lib.rs"),
        "#[toolkit::gear(\n    name = \"demo\",\n    capabilities = [stateless],\n    \
         lifecycle(entry = \"serve\")\n)]\npub struct DemoGear;\n",
    )
    .unwrap();

    let (server, client) = Connection::memory();
    // A peer that reads: the load sends its boundary response and a notification
    // per gear, and a client that never drained would be indistinguishable from
    // one that had gone.
    let drain = std::thread::spawn(move || client.receiver.iter().count());

    let (roots, failed_roots) = open_roots(&[dir]);
    let mut state = State {
        roots,
        registry_plans: BTreeMap::new(),
        catalogue: None,
        failed_roots,
        creation_boundary: None,
        initialized: true,
        allow_writes: false,
        workspace: None,
        documents: BTreeMap::new(),
    };

    assert!(
        catalogue_load(&server, &mut state, RequestId::from(1)).is_none(),
        "the boundary response answered the request, so the dispatcher must not send a second"
    );
    drop(server);
    assert!(drain.join().expect("the draining peer") > 0);

    let catalogue = state
        .catalogue
        .as_ref()
        .expect("a load that ran to the end is the cache");
    assert_eq!(
        catalogue.sources.len(),
        1,
        "the scanned root is in the cached catalogue"
    );
    assert!(
        catalogue
            .gears
            .contains_key(&gearbox_ir::GearId::new("demo").unwrap()),
        "and so is the gear it found: {:?}",
        catalogue.gears.keys().collect::<Vec<_>>()
    );
}

/// A staged load that stopped early is not cached.
///
/// Unreachable today: the load stops only when a notification cannot be sent,
/// which means the channel is closed, which ends the request loop — the
/// truncated catalogue dies with the process. The guard is for the cancellation
/// the code above `catalogue_load` promises ("cancellation is the answer to a
/// slow load"). The moment that exists, an early stop is routine, and caching
/// what it produced would have `resolve` report `GBX0301` for gears that are
/// there.
///
/// Driven by dropping the peer of an in-memory connection, which is exactly the
/// send failure the real path sees.
#[test]
fn a_load_that_stopped_early_is_not_cached() {
    let dir = scratch("stopped");
    // One description, so the load has something to announce and therefore
    // something to fail on. An empty root would finish without ever sending.
    std::fs::write(
        dir.join("gear.gdl"),
        "gear(\n  maturity = \"stable\",\n  name = \"Demo\",\n  category = \"example\",\n  \
         package = cargo(crate_name = \"demo\", lib = \"demo\", path = \".\"),\n)\n",
    )
    .unwrap();

    let (server, client) = Connection::memory();
    // The client is gone before the load begins, so the first notification
    // fails and the walk stops at the S1 boundary.
    drop(client);

    let (roots, failed_roots) = open_roots(&[dir]);
    let mut state = State {
        roots,
        registry_plans: BTreeMap::new(),
        catalogue: None,
        failed_roots,
        creation_boundary: None,
        initialized: true,
        allow_writes: false,
        workspace: None,
        documents: BTreeMap::new(),
    };

    drop(catalogue_load(&server, &mut state, RequestId::from(1)));

    assert!(
        state.catalogue.is_none(),
        "a partial catalogue must not become the cache every later answer is built from"
    );
}

/// `initialize` advertises the two assist providers.
///
/// The spelling of these keys is pinned in `tests/envelopes.rs`, but that test
/// builds a `Capabilities` literal -- it says how the field serialises, not that
/// this handler sets it. Setting `hover_provider: false` here would keep every
/// other test green while, per the field's own doc comment, a language client
/// then never sends the request and the feature silently does not exist.
#[test]
fn initialize_advertises_completion_and_hover() {
    let dir = scratch("capabilities");
    let mut state = state_with(&dir);

    let response = initialize(&mut state, RequestId::from(1), &params(&[&dir]));
    let value = response.response_result.expect("initialize answers");
    let result: crate::protocol::InitializeResult =
        serde_json::from_value(value).expect("the result decodes");

    assert!(
        result.capabilities.hover_provider,
        "a client that does not see this never sends `textDocument/hover`"
    );
    assert!(
        !result.capabilities.completion_provider.resolve_provider,
        "every label is already the text to insert, so there is nothing to resolve"
    );
    assert_eq!(
        result.capabilities.text_document_sync,
        crate::lsp::SYNC_FULL,
        "and the sync mode the document surface depends on"
    );
}
