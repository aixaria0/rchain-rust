//! The rgov governance contracts, vendored as genesis content — the **testnet** setup.
//!
//! Upstream deploys this set at runtime (`scripts/bootstrap-rgov.ts`): a funded key deploys the
//! classes in dependency order, records the resulting URIs, feeds them back into the dependents and
//! builds a master directory. The recorded master URI is a product of that deployment, so it shifts
//! on every chain and goes stale silently. Genesis installs the set instead, with **fixed keys**, so
//! every URI is a constant of the chain (`spec/GENESIS.md` is the manifest).
//!
//! What is installed, in order (`governance_deploys`):
//!
//! 1. the eight classes (`RGOV_CORE`), each registering exactly as upstream wrote it;
//! 2. the **rooted-name master dictionary** ([`master_dictionary_source`], issue #99) — the chain's
//!    name layer. It replaced the testnet *master-directory template* and our `extraSlots` term:
//!    where the template wrote seven (then ten) *slots* into a map reachable only through a parked
//!    capability, the dictionary has a **rooted tier** (a name is `<revAddr>/<path>`, derived from
//!    the caller's deployer id) and a **governed alias tier** holding the short names. It publishes
//!    three facets, parks its own admin handle for the ceremony key, publishes each class under the
//!    operator's root, and aliases the names to those paths;
//! 3. the `GetMe`/`SendThem` feature, which is what a client's first call resolves. Under the
//!    dictionary it *publishes* the two features into the operator's own root (rather than writing
//!    them into a directory's map) and the alias tier points `GetMe`/`SendThem` at them.
//!
//! **Two registrations must not be conflated.** A *class* registers with `insertArbitrary`
//! (upstream's shape) and keeps that shape: every consumer destructures the **bare** value
//! (`for (Dir <- lookCh)`), so converting it to `insertSigned` — whose value is `(nonce, value)` —
//! breaks the consumers silently (a deploy that "processes with success" and produces nothing, which
//! is how this was found). To give clients a hardcodable key anyway, each class *publishes* the URI
//! it was given and genesis copies the entry onto the constant key ([`contract_uri_for`]). Everything
//! this module changes is asserted against the vendored text, so a drift upstream fails the build.
//!
//! **Who holds the admin handle — the ceremony key.** Step 3 is signed by the key that creates the
//! genesis block (`create_genesis_block`'s `ValidatorIdentity`), which is the standard
//! genesis-ceremony arrangement: the handle belongs to the network's operator, who runs the ceremony,
//! and nothing else on the chain can redirect a client's first governance call.
//!
//! It has to be *one* key, and that is not a detail: the dictionary parks
//! `@[*deployerId, "MasterContractAdmin"]` for its own deployer and the feature's registration is
//! gated on reading it back. Signed by a different key the gate never opens, the feature registers
//! nothing, the name layer answers `Nil` for `GetMe`, and a client gets silence — verified on a node:
//! the handshake reached "directory answered GetMe" and never entered `getMe`.
//!
//! It must also be a key whose private half is **not** public. An earlier revision used a key derived
//! from a string literal in this module (`blake2b256("rnode/genesis/rgov/testnet-governance")`), which
//! meant anyone reading the source could exercise that handle — on any network that installed it.
//! That is why the ceremony identity is threaded in rather than a constant.
//!
//! Signing by the ceremony key does **not** move anything a client hardcodes: the eight class keys are
//! the classes' own fixed keys, and the dictionary's three facet keys are derived from *named strings*
//! (`masterdict_*_uri`), not from the deploy's RNG state or the signer —
//! `the_published_keys_are_constants` asserts that the three are distinct and hardcodable.
//!
//! **Still open for a public network:** a network that would rather each client run its own dictionary
//! (rather than the operator holding the shared one) must not install steps 2–3 at all — that is the
//! genesis flag-gate to land next. `spec/GENESIS.md` carries the full list.

use rchain_crypto::hash::blake2b256::hash as blake2b256;
use rchain_crypto::private_key::PrivateKey;
use rchain_crypto::signatures::secp256k1::Secp256k1;
use rchain_crypto::signatures::signatures_alg::SignaturesAlg;
use rchain_crypto::signatures::signed::Signed;
use rchain_models::casper::protocol::casper_message::{DeployData, SignedDeployData};
use rchain_rholang::registry::build_uri;
use rchain_shared::base16;

use crate::validator_identity::ValidatorIdentity;

const KUDOS_RHO: &str = include_str!("resources/rgov/Kudos.rho");
const INBOX_RHO: &str = include_str!("resources/rgov/Inbox.rho");
const DIRECTORY_RHO: &str = include_str!("resources/rgov/Directory.rho");
const MEMBER_ID_GOV_REV_RHO: &str = include_str!("resources/rgov/memberIdGovRev.rho");
const ISSUE_RHO: &str = include_str!("resources/rgov/Issue.rho");
const BALLOT_RHO: &str = include_str!("resources/rgov/Ballot.rho");
const CHAT_RHO: &str = include_str!("resources/rgov/Chat.rho");
const GROUP_RHO: &str = include_str!("resources/rgov/Group.rho");
/// The `GetMe`/`SendThem` feature: what a client's first governance call (`newInbox`) looks up.
const MEMBER_DIRECTORY_RHO: &str = include_str!("resources/rgov/MemberDirectory.rho");
/// The **rooted-name master dictionary** (issue #99) — the term that owns the chain's name layer.
///
/// **Ours, not vendored.** It has no upstream, so it sits beside the port's rgov set rather than in
/// `vendored-originals/`; `tools/audit-vendored-sources.sh` reports that class by count.
///
/// It replaces two things the testnet setup installed: the master-directory *template* (whose seven
/// slots the alias tier now holds) and the `readcap`/`grantcap` pair (whose reader is now the
/// dictionary's `resolve` facet, and whose one-key writer has no work — a rooted name is derived,
/// so there is nothing to grant). See [`masterdict_resolve_uri`].
const MASTER_DICTIONARY_RHO: &str = include_str!("resources/rgov/MasterDictionary.rho");

/// `accounting.MAX_VALUE` — the nonce every system contract registers with (the element consumers
/// destructure and ignore: `for (@(_, X) <- ch)`).
const PHLO_LIMIT: i64 = i32::MAX as i64;

/// The upstream deployment order (`bootstrap-rgov.ts`'s `STEPS`, from the original `Jakefile.js`):
/// `memberIdGovRev` imports `directory.rho` and `inbox.rho`, so those must be installed first.
///
/// `ballot`, `chat` and `group` are not in upstream's `STEPS`, but the master directory has slots
/// for them here: the wallet's editor asks the directory for those three class names, and a slot
/// that was never filled answers `Nil` — which a client cannot tell from "broken". Installing them
/// is the testnet scope decision (`spec/GENESIS.md`).
pub const RGOV_CORE: &[&str] = &[
    "kudos",
    "inbox",
    "directory",
    "roll",
    "issue",
    "ballot",
    "chat",
    "group",
];

/// The fixed private key for a vendored contract, *derived* rather than pasted: a hardcoded hex
/// constant would be unauditable (nobody can check it was not chosen adversarially), whereas a named
/// hash can be recomputed by anyone reading this file. It is the same value on every chain, which is
/// what makes the resulting `rho:id` a constant.
pub fn contract_key(name: &str) -> String {
    base16::encode(&blake2b256(format!("rnode/genesis/rgov/{name}").as_bytes()))
}

/// The fixed timestamp for a vendored contract. Genesis deploys carry no real time; a constant makes
/// the deploy (and therefore its terms' ordering) reproducible. `spec/GENESIS.md` records the value.
const RGOV_TIMESTAMP: i64 = 1_700_000_000_000;

/// The `rho:id` each vendored class is **published under** — a key this node chooses, derived from a
/// named string so anyone can recompute it, and constant for every chain of a given shard id.
///
/// It is *not* the key the deploy registers under. Upstream registers with
/// `rho:registry:insertArbitrary`, and that shape is what every consumer destructures
/// (`for (Dir <- lookCh)` — the bare value, *not* the `(nonce, value)` tuple `insertSigned` stores);
/// converting the registration to `insertSigned` breaks the master-directory template silently, which
/// is how this was found (a template deploy that "processed with success" and produced nothing).
///
/// So the registration stays exactly as upstream wrote it, and each class **publishes** the URI it
/// was given ([`URI_PUBLISH_CHANNEL`]); genesis copies the entry onto this key
/// (`seed_rgov_class_aliases`). A client hardcodes *this*, and the copy is what makes the key
/// independent of the deploy order — the registered URI is `blake2b256` of the deploy's RNG state,
/// deterministic here but moved by inserting any deploy before it.
pub fn contract_uri_for(name: &str) -> Result<String, String> {
    Ok(build_uri(&blake2b256(
        format!("rnode/genesis/rgov-class/{name}").as_bytes(),
    )))
}

/// The key the master dictionary's **resolve facet** is published under — what a governance client
/// resolves to read a name (`resolve`, `resolveAt`, `targetOf`, `aliases`, …).
///
/// This is where the read capability went. It was `readcap_uri`, minted by the testnet template and
/// published under its own constant; it is now a facet of the dictionary, so a client reaches the
/// name layer the same way it reaches everything else. `spec/GENESIS.md` records the replacement.
pub fn masterdict_resolve_uri() -> Result<String, String> {
    Ok(build_uri(&blake2b256(b"rnode/genesis/masterdict-resolve")))
}

/// The key the **publish facet** is published under — the self-scoped write side.
///
/// Every verb derives the caller's address from the id it is handed
/// (`rho:rev:address("fromDeployerId", …)`), so a write outside the caller's own root is not
/// *refused* so much as **inexpressible**: the caller never supplies the owner prefix.
///
/// This is where the grant capability went, and it is the point of #99. `grant(@key, ret)` existed
/// to answer "who may claim a name" by handing out a writer bound to one key; a rooted name is
/// **derived** from the caller's identity, so there is nothing to claim and nothing to grant — and
/// the admission policy nobody had written is no longer needed.
pub fn masterdict_publish_uri() -> Result<String, String> {
    Ok(build_uri(&blake2b256(b"rnode/genesis/masterdict-publish")))
}

/// The key the **root facet** is published under — the alias tier's governor (`alias`, `unalias`,
/// `setRootAuthority`).
///
/// Published like the other two on purpose: authority is checked *inside* the contract, by
/// comparing the caller's derived address to the dictionary's `root`, so a facet anyone can resolve
/// still refuses everyone but the holder. The root is the identity that installs the dictionary —
/// the genesis ceremony's key — and it can hand the role on with `setRootAuthority`.
pub fn masterdict_root_uri() -> Result<String, String> {
    Ok(build_uri(&blake2b256(b"rnode/genesis/masterdict-root")))
}

/// The channel a vendored class publishes its registered URI on, as `["<name>", <uri>]`.
///
/// The publish datums stay in the genesis state: they are the deterministic record of what was
/// installed, and nothing else reads this channel (it is a fixed, genesis-only name).
pub const URI_PUBLISH_CHANNEL: &str = "rnode:genesis:rgov-uri";

/// The genesis term for one vendored contract, adapted as `spec/GENESIS.md` and the NOTICE describe.
pub fn source(name: &str) -> Result<String, String> {
    match name {
        "kudos" => publish_registration(KUDOS_RHO, name, "deployId!([\"#define $Kudos\", uri])"),
        "issue" => publish_registration(ISSUE_RHO, name, "deployId!(uri)"),
        "directory" => {
            let source = publish_registration(DIRECTORY_RHO, name, "deployId!(uri)")?;
            // The file's tail exercises the contract it just registered.
            cut_from(&source, "} |\n    directory!(Nil, *ret)")
        }
        "inbox" => {
            let source = publish_registration(INBOX_RHO, name, "deployId!(uri)")?;
            // `read(ret)` — the zero-argument read — consumes the store datum and never puts it back,
            // so after one read the inbox answers nothing again, ever: every later `write`, `peek`
            // and `read` waits on a channel that is empty for the rest of the chain's life, silently.
            // Its two typed siblings (`read(type,…)`, `read(type,subtype,…)`) both restore it
            // (`box!(rest)` / `box!(*items)`), and the docstring is "read (and *remove*) all
            // messages" — the messages, not the container — so the omission is a defect in the
            // vendored text. The store is emptied, so the restore is `box!(Nil)`.
            let source = replace_once(
                &source,
                "      for (items <- box) {\n        ret!(*items)\n      }",
                "      for (items <- box) {\n        ret!(*items) |\n        box!(Nil)\n      }",
                "inbox.rho's zero-argument `read`",
            )?;
            // The class registration shares its block with a trailing test program (create an
            // instance, register its send capability, send test messages). The cut keeps the
            // registration and the `for` that reports it; the demo prints that precede it go too.
            let source = cut_from(&source, " |\n    lookup!(uri, *lookupCh) |")?;
            Ok(source.replace(
                "  stdout!(\"hello world\") |\n  stdout!([\"Unforgeable\", bundle+{*Inbox}]) |\n",
                "",
            ))
        }
        "roll" => {
            let source = publish_registration(MEMBER_ID_GOV_REV_RHO, name, "deployId!(uri)")?;
            // The dependencies are imported by URI, and those URIs are now constants.
            substitute_imports(&source)
        }
        "ballot" => {
            let source = publish_registration(BALLOT_RHO, name, "deployId!(uri)")?;
            // The tail drives four demo voters through a tally.
            cut_from(&source, "|\n  trace!(\"testing Ballot\")")
        }
        "chat" => {
            let source = publish_registration(CHAT_RHO, name, "stdout!([\"#define $Chat\", uri])")?;
            // The tail drives a listener through four messages.
            cut_from(&source, "} |\n    result!(\"testing\") |")
        }
        "group" => {
            let source = publish_registration(GROUP_RHO, name, "deployId!(uri)")?;
            // `Group.rho:6` binds `deployerId` at **registration** time, and `:55` peeks
            // `@[deployerId, "dictionary"]` — the locker `MemberDirectory`'s `createMe` writes *for
            // the key that ran it*. Upstream deploys the whole set from one funded key, so those are
            // the same identity; genesis signs each class with its own fixed key (`contract_key`), so
            // the dictionary this contract peeks is the class deploy's, which nothing ever writes.
            // The peek never fires, so nothing between `:49` and `:60` runs and `Group!("new", …)`
            // answers nothing — **silently**, because an unmatched receive is not an error (AUDIT
            // C25; the vendored contract was unusable, and the group slot with it).
            //
            // The repair addresses the name layer the way a client does: the dictionary's **resolve
            // facet** is published under a *constant* URI ([`masterdict_resolve_uri`], `spec/GENESIS.md`),
            // and `resolve("Directory")` is what the wallet's own `getMe` handshake asks the same
            // dictionary for. `lookup` is already bound at the file's head (`:8`), so the replacement
            // adds no dependency the contract did not have.
            //
            // `for (f <- capCh)` binds a **name**, not a value — `@f` would bind the contract as a
            // value and a value cannot be called.
            let resolve = masterdict_resolve_uri()?;
            let source = replace_once(
                &source,
                "          for(@{\"read\": *masterRead, ..._} <<- @[*deployerId, \"dictionary\"]) {\n            stdout!({\"read\": *masterRead}) |\n            masterRead!(\"Directory\", *ret)\n          } |",
                &format!(
                    "          new capCh in {{\n            lookup!(`{resolve}`, *capCh) |\n            for (masterRead <- capCh) {{\n              stdout!({{ \"read\": *masterRead }}) |\n              masterRead!(\"resolve\", [\"Directory\"], *ret)\n            }}\n          }} |"
                ),
                "Group.rho's `new` reading the deployer's dictionary",
            )?;
            // The tail creates two demo groups and looks them up.
            cut_from(&source, "} |\n  new return(`rho:io:stdout`)")
        }
        "memberDirectory" => {
            // The feature. Its registration epilogue ends with a demo `sendThem` to a hardcoded
            // address; the rest of the epilogue is what a client needs.
            //
            // Under the rooted dictionary (issue #99) it no longer writes into a directory's slot
            // map. `GetMe`/`SendThem` are **published into the operator's own rooted namespace**, at
            // `<rootAddr>/GetMe` and `<rootAddr>/SendThem`, and the dictionary's alias tier points
            // the short names there — the operator *is* the root authority, so the names are its own.
            // That is why the two writes had to become `publish` calls with a derived path, and why
            // the two reads had to change shape: the old `MCAread!(key, ret)` is the resolve facet's
            // `resolve` verb now. The gates around them (`for (@{"read"|"write": …} <<- @[*deployerId,
            // "MasterContractAdmin"])`) are unchanged — the dictionary parks that handle itself.
            let source = cut_statement(
                MEMBER_DIRECTORY_RHO,
                "sendThem!([\"1111NkGJcLb9UdKg27bE1MXhaXwd2Sdhssn3i3EcWnZy11VLyW3zH\"",
            )?;
            let source = replace_once(
                &source,
                "MCAread!(\"Inbox\", *inboxCh)",
                "MCAread!(\"resolve\", [\"Inbox\"], *inboxCh)",
                "the feature's Inbox read",
            )?;
            let source = replace_once(
                &source,
                "MCAread!(\"Directory\", *dirCh)",
                "MCAread!(\"resolve\", [\"Directory\"], *dirCh)",
                "the feature's Directory read",
            )?;
            // `RevAddress` is not bound at this file's head, so each substitution binds it.
            let source = replace_once(
                &source,
                "MCAwrite!(\"GetMe\", bundle+{*getMe}, *mcaUpdateRet1)",
                "new meCh1, RevAddress(`rho:rev:address`) in {\n\
                 \x20        RevAddress!(\"fromDeployerId\", *deployerId, *meCh1) |\n\
                 \x20        for (@me1 <- meCh1) {\n\
                 \x20          MCAwrite!(\"publish\", [*deployerId, me1 ++ \"/GetMe\", bundle+{*getMe}], *mcaUpdateRet1)\n\
                 \x20        }\n\
                 \x20      }",
                "the GetMe publish",
            )?;
            let source = replace_once(
                &source,
                "MCAwrite!(\"SendThem\", bundle+{*sendThem}, *mcaUpdateRet2)",
                "new meCh2, RevAddress(`rho:rev:address`) in {\n\
                 \x20        RevAddress!(\"fromDeployerId\", *deployerId, *meCh2) |\n\
                 \x20        for (@me2 <- meCh2) {\n\
                 \x20          MCAwrite!(\"publish\", [*deployerId, me2 ++ \"/SendThem\", bundle+{*sendThem}], *mcaUpdateRet2)\n\
                 \x20        }\n\
                 \x20      }",
                "the SendThem publish",
            )?;
            Ok(load(&source))
        }
        // Not a class: the master dictionary itself.
        "masterDictionary" => master_dictionary_source(),
        other => Err(format!("rgov: unknown vendored contract `{other}`")),
    }
}

/// The order the whole governance block installs in, after the class libraries: all eight classes
/// (each publishing its URI), then the master directory that references them, then the three slots
/// upstream's template omits, then the `GetMe` feature that a client's first call resolves.
///
/// The last three are signed by **the genesis ceremony's key** (`ceremony`) — see the module doc: the
/// master directory's admin capability must belong to the network's operator, not to a key anyone can
/// derive from the source. The classes are unaffected (they hold no capability).
pub fn governance_deploys(
    shard_id: &str,
    ceremony: &ValidatorIdentity,
) -> Result<Vec<(&'static str, SignedDeployData)>, String> {
    let mut out = deploys_named(shard_id)?;
    for name in ["masterDictionary", "memberDirectory"] {
        out.push((name, signed_deploy(name, shard_id, &ceremony.private_key)?));
    }
    Ok(out)
}

/// The publish channel as a `SortedProc`, for the callers that read it.
pub fn publish_channel() -> rchain_models::sorted::SortedProc {
    rchain_models::sorted::SortedProc::new(rchain_models::par_ops::from_expr(
        rchain_models::ast::Expr::GString(URI_PUBLISH_CHANNEL.to_string()),
    ))
}

/// Parse a published `["<name>", <uri>]` datum into its two parts.
///
/// The list's **length** is not guaranteed: `RhoList::unapply` returns the items at any arity, so a
/// published `["name"]` or `[]` used to reach `items[1]`/`items[0]` and panic the genesis seeding
/// (H2b). Both reads are `Option` reads now; the caller (`genesis/mod.rs`'s `seed_rgov_aliases_from`)
/// skips a datum it cannot parse, so a short list is skipped rather than fatal.
pub fn published_uri(par: &rchain_models::ast::Par) -> Option<(String, String)> {
    use rchain_models::rholang::RhoType::{RhoList, RhoString, RhoUri};
    let items = RhoList::unapply(par)?;
    let name = RhoString::unapply(items.first()?)?.to_string();
    let uri = RhoUri::unapply(items.get(1)?)
        .or_else(|| RhoString::unapply(items.get(1)?))?
        .to_string();
    Some((name, uri))
}

/// Append the URI publish to a class registration's report, so the genesis seeder can copy the
/// registered entry onto the constant key. The marker is asserted and must be unique: a registration
/// that reports differently upstream must fail the build rather than silently stop publishing.
fn publish_registration(rho: &str, name: &str, marker: &str) -> Result<String, String> {
    let occurrences = rho.matches(marker).count();
    if occurrences != 1 {
        return Err(format!(
            "rgov: expected exactly one `{marker}` in {name}.rho, found {occurrences}"
        ));
    }
    Ok(load(&rho.replace(
        marker,
        &format!("{marker} | @\"{URI_PUBLISH_CHANNEL}\"!([\"{name}\", uri])"),
    )))
}

/// Remove a single trailing statement (and the `|` that joined it) from the end of the file.
///
/// Distinct from [`cut_from`], which truncates to the end: this drops one statement in the middle of
/// a block, keeping the block's structure. Used for a demo call that upstream runs last.
fn cut_statement(rho: &str, marker: &str) -> Result<String, String> {
    let occurrences = rho.matches(marker).count();
    if occurrences != 1 {
        return Err(format!(
            "rgov: expected exactly one `{marker}` statement, found {occurrences}"
        ));
    }
    let at = rho
        .find(marker)
        .ok_or_else(|| format!("rgov: `{marker}` vanished between the count and the find"))?;
    // Walk back over the `|` (and whitespace) that joined the statement to the previous one.
    let before = rho[..at].trim_end();
    let before = before.strip_suffix('|').unwrap_or(before).trim_end();
    let after = &rho[at..];
    let end = after.find('\n').map(|i| at + i + 1).unwrap_or(rho.len());
    Ok(format!("{before}\n{}", &rho[end..]))
}

/// Drop everything from `marker` to the end of the file, keeping the text before it — and close the
/// blocks the truncation left open.
///
/// Used for the deploy-time self-test programs upstream runs at the end of a file (see the NOTICE):
/// a chain's genesis must not send test traffic or print demo output. The kept prefix is usually
/// mid-block (the test program shares a `new … in { … }` with the registration it exercises), so
/// the brace balance of the prefix is appended rather than hand-counted here: the count is checked
/// by the tests that normalize every rendered source, and a wrong count fails the genesis build.
fn cut_from(source: &str, marker: &str) -> Result<String, String> {
    let at = source
        .find(marker)
        .ok_or_else(|| format!("rgov: marker `{marker}` not found for the self-test cut"))?;
    let kept = &source[..at];
    let open = kept.chars().filter(|c| *c == '{').count();
    let close = kept.chars().filter(|c| *c == '}').count();
    let mut out = kept.to_string();
    for _ in 0..open.saturating_sub(close) {
        out.push_str("\n}");
    }
    Ok(out)
}

/// Substitute the `match ("import", "./X.rho", \`rho:id:...\`)` markers with the installed
/// contracts' constants — this is what `bootstrap-rgov.ts` does at runtime by string replacement,
/// and doing it at build time is what makes the whole set's URIs stable.
fn substitute_imports(source: &str) -> Result<String, String> {
    let mut out = source.to_string();
    for dependency in ["directory", "inbox"] {
        let marker = format!("match (\"import\", \"./{dependency}.rho\", `rho:id:...`)");
        if !out.contains(&marker) {
            return Err(format!(
                "rgov: dependency marker for `{dependency}` not found"
            ));
        }
        let uri = contract_uri_for(dependency)?;
        out = out.replace(
            &marker,
            &format!("match (\"import\", \"./{dependency}.rho\", `{uri}`)"),
        );
    }
    Ok(out)
}

/// Append the loader comment the other blessed sources carry (`CompiledRholangSource.loadSource`).
fn load(source: &str) -> String {
    format!("{source}\n//Loaded from resource file <<rgov>>\n")
}

/// Replace `marker` with `replacement`, asserting the marker occurs **exactly once**.
///
/// For an adaptation that changes *behaviour* (rather than removing demo traffic), a marker that
/// upstream reworded must fail the build: a silent no-op would leave a file that looks adapted and
/// is not — the failure mode this module's other helpers already refuse (see [`publish_registration`]).
fn replace_once(rho: &str, marker: &str, replacement: &str, what: &str) -> Result<String, String> {
    let occurrences = rho.matches(marker).count();
    if occurrences != 1 {
        return Err(format!(
            "rgov: expected exactly one `{marker}` in {what}, found {occurrences}"
        ));
    }
    Ok(load(&rho.replace(marker, replacement)))
}

/// Build + sign the genesis deploy for one vendored **class**, with the class's own fixed key
/// (`contract_key`): a class holds no capability, and pinning its key keeps a rebuilt genesis
/// reproducible.
pub fn deploy(name: &str, shard_id: &str) -> Result<SignedDeployData, String> {
    let sk = PrivateKey::new(base16::unsafe_decode(&contract_key(name)));
    signed_deploy(name, shard_id, &sk)
}

/// Build + sign the genesis deploy for one vendored term with **an explicit key** — the ceremony
/// path ([`governance_deploys`]) signs the master directory, the extra slots and the `GetMe` feature
/// with the genesis ceremony's own identity.
pub fn signed_deploy(
    name: &str,
    shard_id: &str,
    sk: &PrivateKey,
) -> Result<SignedDeployData, String> {
    let data = DeployData {
        attachments: Vec::new(),
        term: source(name)?,
        timestamp: RGOV_TIMESTAMP,
        phlo_price: 0,
        phlo_limit: PHLO_LIMIT,
        valid_after_block_number: 0,
        shard_id: shard_id.to_string(),
    };
    let signed = Signed::new(data, &Secp256k1, sk).map_err(|e| e.to_string())?;
    Ok(SignedDeployData {
        data: signed.data,
        deployer: signed.pk.bytes().to_vec(),
        sig: signed.sig,
        sig_algorithm: signed.sig_algorithm.name().to_string(),
    })
}

/// The vendored set in install order.
pub fn deploys(shard_id: &str) -> Result<Vec<SignedDeployData>, String> {
    Ok(deploys_named(shard_id)?
        .into_iter()
        .map(|(_, deploy)| deploy)
        .collect())
}

/// The vendored set with its manifest names, in install order — so the genesis ordering check can
/// name the contracts it orders (`BLESSED_DEPENDENCIES`).
pub fn deploys_named(shard_id: &str) -> Result<Vec<(&'static str, SignedDeployData)>, String> {
    RGOV_CORE
        .iter()
        .map(|name| deploy(name, shard_id).map(|deploy| (*name, deploy)))
        .collect()
}

/// The `rho:id` of every vendored contract, in install order — the constants a consumer (or the
/// master directory template) hardcodes.
pub fn contract_uris() -> Result<Vec<(&'static str, String)>, String> {
    RGOV_CORE
        .iter()
        .map(|name| contract_uri_for(name).map(|uri| (*name, uri)))
        .collect()
}

/// `build_uri` over the deployer key (the `insertSigned` derivation). Kept because the *class* keys
/// are no longer derived this way — see [`contract_uri_for`] — and a reader comparing the two
/// derivations should be able to see both.
pub fn derive_uri(private_key_hex: &str) -> Result<String, String> {
    let sk = PrivateKey::new(base16::unsafe_decode(private_key_hex));
    let pk = Secp256k1.to_public(&sk).map_err(|e| e.to_string())?;
    Ok(build_uri(&blake2b256(pk.bytes())))
}

/// The master dictionary with the class URIs substituted for the installed constants.
///
/// Its genesis alias tier holds ten short names, and every one points at a *constant*: the eight
/// classes register with `insertArbitrary` (an rng-derived uri) and publish what they were given,
/// and genesis copies each entry onto [`contract_uri_for`]'s key. Substituting here rather than at
/// deploy time is what makes the dictionary's answers identical on every chain of this port.
///
/// `Echo` and `Log` name no class of their own — their `rho` files never call `insertArbitrary` —
/// so they point at `directory`, exactly as `bootstrap-rgov.ts` does with its `SLOT_ALIASES`.
pub fn master_dictionary_source() -> Result<String, String> {
    let aliases: [(&str, &str); 8] = [
        ("__URI_DIRECTORY__", "directory"),
        ("__URI_INBOX__", "inbox"),
        ("__URI_ISSUE__", "issue"),
        ("__URI_KUDOS__", "kudos"),
        ("__URI_ROLL__", "roll"),
        ("__URI_CHAT__", "chat"),
        ("__URI_BALLOT__", "ballot"),
        ("__URI_GROUP__", "group"),
    ];
    let mut out = MASTER_DICTIONARY_RHO.to_string();
    for (placeholder, class) in aliases {
        if !out.contains(placeholder) {
            return Err(format!(
                "rgov: the master dictionary no longer carries {placeholder}"
            ));
        }
        out = out.replace(placeholder, &contract_uri_for(class)?);
    }
    if out.contains("__URI_") {
        return Err("rgov: an unresolved placeholder survives in the master dictionary".into());
    }
    Ok(load(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed ceremony identity for the tests: the genesis ceremony's key, in-process.
    fn ceremony_identity() -> ValidatorIdentity {
        let sk = PrivateKey::new(vec![7u8; 32]);
        let public_key = Secp256k1
            .to_public(&sk)
            .unwrap_or_else(|e| panic!("a fixed 32-byte scalar is a valid secp256k1 key: {e}"));
        ValidatorIdentity {
            public_key,
            private_key: sk,
            sig_algorithm: "secp256k1".to_string(),
        }
    }

    /// Normalizing a blessed term needs the node's 32 MiB stack (these contracts recurse past the
    /// 2 MiB test default), matching `node/src/main.rs` and `node/tests/common/mod.rs`.
    fn normalizes(term: &str) -> bool {
        let term = term.to_string();
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || rchain_rholang::normalizer::source_to_adt(&term).is_ok())
            .expect("spawn")
            .join()
            .expect("join")
    }

    /// Every rendered term, classes and the three governance terms alike, parses and normalizes —
    /// the patches edit text and one of the terms is ours, so this is what catches an unbalanced or
    /// malformed adaptation.
    #[test]
    fn every_rendered_contract_parses_and_normalizes() {
        for name in RGOV_CORE
            .iter()
            .chain(&["masterDictionary", "memberDirectory"])
        {
            let term = source(name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                normalizes(&term),
                "{name}: the adapted term must parse and normalize"
            );
        }
    }

    /// The class registration keeps **upstream's shape** — `insertArbitrary`, whose stored value is
    /// the bare class — and every class publishes the URI it was given. This is the regression that
    /// matters: converting the registration to `insertSigned` stores `(nonce, value)`, and the
    /// master-directory template (and `memberIdGovRev`) destructure the bare value, so the template
    /// stalls silently.
    #[test]
    fn the_class_registration_keeps_upstreams_shape() {
        for name in RGOV_CORE {
            let term = source(name).unwrap();
            assert!(
                term.contains("insertArbitrary!("),
                "{name}: the class registration stays upstream's"
            );
            assert!(
                !term.contains("insertSigned!"),
                "{name}: a signed registration would store a `(nonce, value)` tuple, which the \
                 rgov consumers cannot destructure"
            );
            assert!(
                term.contains(&format!("@\"{URI_PUBLISH_CHANNEL}\"!([\"{name}\", uri])")),
                "{name}: the class must publish its URI for genesis to copy onto the constant key"
            );
        }
    }

    /// The two governance terms are signed by **one** key, and the classes are not: the dictionary
    /// parks its admin handle for its own deployer and the feature's registration is gated on reading
    /// it back, so a mismatch there leaves `GetMe` unregistered and answering `Nil` — a stall with no
    /// error anywhere (verified on a node).
    #[test]
    fn the_governance_terms_are_signed_by_the_ceremony_key() {
        let ceremony = ceremony_identity();
        let deploys = governance_deploys("root", &ceremony).expect("the governance set builds");
        for (name, deploy) in &deploys {
            if ["masterDictionary", "memberDirectory"].contains(name) {
                assert_eq!(
                    deploy.deployer,
                    ceremony.public_key.bytes().to_vec(),
                    "{name}: the admin handle must belong to the ceremony key"
                );
            } else {
                assert_ne!(
                    deploy.deployer,
                    ceremony.public_key.bytes().to_vec(),
                    "{name}: a class holds no capability, so it keeps its own fixed key"
                );
            }
        }
    }

    /// The keys genesis publishes, pinned. A change here is a genesis change and moves what a client
    /// hardcodes, so it must be reflected in `spec/GENESIS.md`.
    #[test]
    fn the_published_keys_are_constants() {
        let expected: &[(&str, &str)] = &[
            ("kudos", "PIN"),
            ("inbox", "PIN"),
            ("directory", "PIN"),
            ("roll", "PIN"),
            ("issue", "PIN"),
            ("ballot", "PIN"),
            ("chat", "PIN"),
            ("group", "PIN"),
        ];
        let uris = contract_uris().unwrap();
        assert_eq!(uris.len(), expected.len());
        for ((name, uri), (expected_name, expected_uri)) in uris.iter().zip(expected) {
            assert_eq!(name, expected_name);
            if *expected_uri == "PIN" {
                println!("{name} -> {uri}");
            } else {
                assert_eq!(uri, expected_uri, "{name}");
            }
            // Derived from a named string, so a reader can recompute it.
            assert!(uri.starts_with("rho:id:"), "{uri}");
        }
        // The dictionary's three facets (issue #99). They replace the `readcap`/`grantcap` pair, and
        // they must be *distinct* keys: a client that resolved the publish facet where it asked for
        // the resolve facet would get a library it cannot read through, and one that resolved
        // either where it asked for the root facet would get a governor it cannot govern with —
        // silently, since a call at the wrong arity does nothing (law 40 over law 38).
        let keys = [
            ("masterdict-resolve", masterdict_resolve_uri().unwrap()),
            ("masterdict-publish", masterdict_publish_uri().unwrap()),
            ("masterdict-root", masterdict_root_uri().unwrap()),
        ];
        for (name, uri) in &keys {
            println!("{name} -> {uri}");
            assert!(uri.starts_with("rho:id:"), "{uri}");
        }
        for (i, (n1, u1)) in keys.iter().enumerate() {
            for (n2, u2) in keys.iter().skip(i + 1) {
                assert_ne!(u1, u2, "{n1} and {n2} must be different keys");
            }
        }
    }

    /// `memberIdGovRev` imports its dependencies by URI; the install order guarantees those exist,
    /// and the substitution is what makes the whole set's URIs stable across chains.
    #[test]
    fn the_member_directory_imports_the_installed_contracts() {
        let term = source("roll").unwrap();
        for dependency in ["directory", "inbox"] {
            let uri = contract_uri_for(dependency).unwrap();
            assert!(
                term.contains(&format!(
                    "match (\"import\", \"./{dependency}.rho\", `{uri}`)"
                )),
                "roll must import {dependency} by its installed URI"
            );
        }
        assert!(
            !term.contains("rho:id:..."),
            "no unresolved import placeholder may remain"
        );
    }

    /// The master dictionary ships with the installed class URIs, and none of the recorded testnet
    /// URIs survive — those are the stale values the vendoring exists to remove.
    #[test]
    fn the_master_dictionary_carries_the_installed_uris() {
        let term = master_dictionary_source().unwrap();
        for name in [
            "directory",
            "inbox",
            "issue",
            "kudos",
            "roll",
            "chat",
            "ballot",
            "group",
        ] {
            let uri = contract_uri_for(name).unwrap();
            assert!(
                term.contains(&uri),
                "the dictionary must name the {name} URI"
            );
        }
        assert!(
            !term.contains("o9b5otixodhpkxgtbsz1ja5sak43gdhei69ukc9swp355qi8dkm3n7"),
            "no recorded testnet URI may survive"
        );
        assert!(
            !term.contains("__URI_"),
            "every placeholder must be substituted"
        );
        assert!(
            normalizes(&term),
            "the substituted dictionary must parse and normalize"
        );
    }

    /// **SECURITY.md's rule, as a source scan rather than a review.** A deployer id is a *bearer
    /// value*: "Nothing was forged. The identity was disclosed, and disclosure is transfer." So
    /// `*deployerId` may appear only as **the address derivation**, as its own binder, as the key of
    /// the admin handle the dictionary parks for itself, or forwarded to one of the dictionary's
    /// *own* facets — which derive the address and discard the id. Never as a state value, a
    /// returned value, or a body handed to a facet the dictionary does not control.
    ///
    /// A line scan is a floor, not a proof — it cannot see a value that reaches a client by another
    /// name — but it is the check a future verb would have to get past.
    #[test]
    fn the_deployer_id_is_only_ever_the_address_derivation() {
        for (i, line) in MASTER_DICTIONARY_RHO.lines().enumerate() {
            if !line.contains("deployerId") || line.trim_start().starts_with("//") {
                continue;
            }
            let t = line.trim();
            let permitted = t.contains("RevAddress!(\"fromDeployerId\", *deployerId")
                || t.contains("deployerId(`rho:rchain:deployerId`)")
                || t.contains("@[*deployerId, \"MasterContractAdmin\"]")
                || t.contains("rootFacet!(\"alias\", [*deployerId,")
                || t.contains("publishFacet!(\"publish\", [*deployerId,");
            assert!(
                permitted,
                "line {}: the deployer id may appear only in the address derivation, its binder, \
                 the admin-handle key, or a call to the dictionary's own root facet:\n  {t}",
                i + 1
            );
        }
    }

    /// The self-test traffic upstream runs at deploy time is gone: genesis must not send test
    /// messages or print demo output.
    #[test]
    fn the_deploy_time_self_tests_are_removed() {
        let inbox = source("inbox").unwrap();
        assert!(!inbox.contains("hello world"));
        assert!(!inbox.contains("\"values\",\"test\""));
        assert!(!source("directory").unwrap().contains("got capabilities"));
        assert!(!source("ballot").unwrap().contains("testing Ballot"));
        assert!(!source("group").unwrap().contains("got em"));
        // The feature's epilogue keeps its registration and drops its demo send. Under the rooted
        // dictionary the registration is a `publish` into the operator's own root, with the path
        // derived from the id — so the assertion names the new shape, not the old slot write.
        let feature = source("memberDirectory").unwrap();
        assert!(
            feature.contains("MCAwrite!(\"publish\", [*deployerId, me1 ++ \"/GetMe\""),
            "the feature must still register `GetMe` — as a rooted publish now"
        );
        assert!(
            feature.contains("MCAwrite!(\"publish\", [*deployerId, me2 ++ \"/SendThem\""),
            "and `SendThem` with it"
        );
        assert!(
            !feature.contains("sendThem!([\"1111NkGJcLb9UdKg27bE1MXhaXwd2Sdhssn3i3EcWnZy11VLyW3zH")
        );
    }

    /// **H2b.** A published datum is `["<name>", <uri>]`, and nothing bounds the list's *length*:
    /// `RhoList::unapply` returns the items at any arity, so a published `["name"]` — or `[]` —
    /// reached `items[1]`/`items[0]` and panicked the genesis seeding. The caller
    /// (`genesis/mod.rs`'s `seed_rgov_aliases_from`) skips a datum it cannot parse, so both must be
    /// `None`; the well-formed two-element form is the control, so this test can fail.
    #[test]
    fn published_uri_refuses_a_list_shorter_than_two() {
        use rchain_models::rholang::RhoType::{RhoList, RhoString, RhoUri};

        let one = RhoList::apply(vec![RhoString::apply("sample".to_string())]);
        assert_eq!(published_uri(&one), None, "a one-element datum has no uri");

        let empty = RhoList::apply(vec![]);
        assert_eq!(published_uri(&empty), None, "and neither has an empty one");

        let two = RhoList::apply(vec![
            RhoString::apply("sample".to_string()),
            RhoUri::apply("rholang://sample".to_string()),
        ]);
        assert_eq!(
            published_uri(&two),
            Some(("sample".to_string(), "rholang://sample".to_string())),
            "the well-formed datum still parses"
        );
    }
}
