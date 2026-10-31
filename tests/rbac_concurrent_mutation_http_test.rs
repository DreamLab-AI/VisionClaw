//! ADR-2010 — concurrent role mutations over HTTP.
//!
//! Drives `PUT`/`DELETE /api/admin/rbac/users/{pubkey}/role` through the real
//! stack (`RbacGate` → `verify_admin` → handler → `RoleStore` on a SQLite
//! connection) with genuine NIP-98 signatures, firing requests concurrently
//! (`join_all` on one runtime, so admission, caller-role resolution and the
//! mutation transaction of different requests interleave at their await
//! points). It asserts the invariants the ADR states:
//!
//! 1. **Privilege ceiling** — an Admin can never mint or touch an Admin/Owner,
//!    however many such requests race; Editors and Viewers can assign nothing.
//! 2. **No lost update** — two Admins writing different roles to one target
//!    leave exactly one of the two writes, with `assigned_by` agreeing with the
//!    role (a torn row would pair one writer's role with the other's name).
//! 3. **Caller freshness** — a caller demoted concurrently with its own
//!    mutation yields only a serialisable outcome: the mutation fully applied
//!    (caller first) or fully refused (demotion first), never a mutation made
//!    with authority the caller no longer held.
//! 4. **Last-Owner guard** — two Owners demoting each other, removing each
//!    other, or demoting themselves concurrently never leave zero Owners and
//!    never both succeed.
//!
//! One test function: the process-global role store is a `OnceCell` and the
//! last-Owner scenario needs the Owner population to itself. NIP-98 event ids
//! are single-use, so every request carries a unique query string.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use actix_web::http::StatusCode;
use actix_web::{test, web, App};
use futures::future::join_all;
use nostr_sdk::prelude::Keys;
use tokio_rusqlite::Connection;
use visionclaw_server::handlers::admin_rbac_handler;
use visionclaw_server::middleware::RbacGate;
use visionclaw_server::models::rbac::UserRole;
use visionclaw_server::services::nostr_service::NostrService;
use visionclaw_server::services::role_store::{set_global_role_store, RoleStore};
use visionclaw_server::utils::auth::nip98_request_url;
use visionclaw_server::utils::nip98::{build_auth_header, generate_nip98_token, Nip98Config};

static NONCE: AtomicUsize = AtomicUsize::new(0);

fn hex(k: &Keys) -> String {
    k.public_key().to_hex()
}

/// One role-mutation request: who signs, what verb, which target, which role.
#[derive(Clone)]
struct Mutation {
    caller: Keys,
    target: String,
    /// `Some(role)` = `PUT`; `None` = `DELETE`.
    role: Option<&'static str>,
    /// Scheduler yields before the request is sent, to sweep the interleaving.
    yields: usize,
}

impl Mutation {
    fn after(mut self, yields: usize) -> Self {
        self.yields = yields;
        self
    }
}

fn put(caller: &Keys, target: &str, role: &'static str) -> Mutation {
    Mutation {
        caller: caller.clone(),
        target: target.to_string(),
        role: Some(role),
        yields: 0,
    }
}

fn del(caller: &Keys, target: &str) -> Mutation {
    Mutation {
        caller: caller.clone(),
        target: target.to_string(),
        role: None,
        yields: 0,
    }
}

/// Build and sign a request for a [`Mutation`] over exactly the URL the server
/// rebuilds. A macro because `actix_http::Request` is not nameable from here.
macro_rules! request {
    ($m:expr) => {{
        let m: &Mutation = $m;
        let uri = format!(
            "/api/admin/rbac/users/{}/role?n={}",
            m.target,
            NONCE.fetch_add(1, Ordering::SeqCst)
        );
        let method = if m.role.is_some() { "PUT" } else { "DELETE" };
        let probe = test::TestRequest::default().uri(&uri).to_http_request();
        let token = generate_nip98_token(
            &m.caller,
            &Nip98Config {
                url: nip98_request_url(&probe),
                method: method.to_string(),
                body: None,
            },
        )
        .expect("sign NIP-98 token");
        let builder = match m.role {
            Some(_) => test::TestRequest::put(),
            None => test::TestRequest::delete(),
        }
        .uri(&uri)
        .insert_header(("Authorization", build_auth_header(&token)));
        match m.role {
            Some(role) => builder
                .set_json(serde_json::json!({ "role": role }))
                .to_request(),
            None => builder.to_request(),
        }
    }};
}

/// Fire every mutation at once and return the statuses in input order.
macro_rules! fire {
    ($app:expr, $batch:expr $(,)?) => {{
        let app = &$app;
        let batch: &[Mutation] = $batch;
        join_all(batch.iter().map(|m| async move {
            for _ in 0..m.yields {
                tokio::task::yield_now().await;
            }
            test::call_service(app, request!(m)).await.status()
        }))
        .await
    }};
}

async fn roles(store: &RoleStore) -> HashMap<String, (UserRole, Option<String>)> {
    store
        .list()
        .await
        .expect("list roles")
        .into_iter()
        .map(|a| (a.pubkey, (a.role, a.assigned_by)))
        .collect()
}

async fn seed(store: &RoleStore, k: &Keys, role: UserRole) {
    store.set(&hex(k), role, None).await.expect("seed role");
}

#[actix_web::test]
async fn concurrent_role_mutations_over_http_keep_the_lattice_invariants() {
    // Default deployment posture: enforce, no dev bypass, no legacy power users.
    std::env::remove_var("POWER_USER_PUBKEYS");
    std::env::remove_var("RBAC_GATE_MODE");
    std::env::remove_var("RBAC_DEFAULT_ROLE");
    std::env::remove_var("VISIONCLAW_DEV_MODE");

    let conn = Arc::new(Connection::open_in_memory().await.expect("open db"));
    let store = Arc::new(RoleStore::new(conn).await.expect("role store"));
    assert!(set_global_role_store(store.clone()).is_ok());

    let nostr = web::Data::new(NostrService::new());
    let app = test::init_service(
        App::new().app_data(nostr.clone()).service(
            web::scope("/api")
                .wrap(RbacGate::from_env())
                .configure(admin_rbac_handler::configure_routes),
        ),
    )
    .await;

    let root = Keys::generate();
    seed(&store, &root, UserRole::Owner).await;

    // ---- 1. Privilege ceiling ------------------------------------------------
    let admin = Keys::generate();
    let editor = Keys::generate();
    let viewer = Keys::generate();
    let peer_admin = Keys::generate();
    seed(&store, &admin, UserRole::Admin).await;
    seed(&store, &editor, UserRole::Editor).await;
    seed(&store, &viewer, UserRole::Viewer).await;
    seed(&store, &peer_admin, UserRole::Admin).await;

    let mut batch = Vec::new();
    let mut expect = Vec::new();
    for _ in 0..4 {
        for (role, ok) in [
            ("owner", false),
            ("admin", false),
            ("editor", true),
            ("viewer", true),
        ] {
            let t = Keys::generate();
            batch.push(put(&admin, &hex(&t), role));
            expect.push(if ok {
                StatusCode::OK
            } else {
                StatusCode::FORBIDDEN
            });
            // Neither an Editor nor a Viewer may assign anything.
            batch.push(put(&editor, &hex(&Keys::generate()), "viewer"));
            expect.push(StatusCode::FORBIDDEN);
            batch.push(put(&viewer, &hex(&Keys::generate()), "viewer"));
            expect.push(StatusCode::FORBIDDEN);
        }
    }
    // An Admin may not demote or remove a peer Admin either.
    batch.push(put(&admin, &hex(&peer_admin), "viewer"));
    expect.push(StatusCode::FORBIDDEN);
    batch.push(del(&admin, &hex(&peer_admin)));
    expect.push(StatusCode::FORBIDDEN);

    let got = fire!(app, &batch);
    for (i, (g, e)) in got.iter().zip(&expect).enumerate() {
        assert_eq!(g, e, "privilege ceiling: request {i} expected {e}, got {g}");
    }
    let state = roles(&store).await;
    let (ceiling_role, _) = &state[&hex(&peer_admin)];
    assert_eq!(
        *ceiling_role,
        UserRole::Admin,
        "peer Admin must be untouched"
    );
    let minted_above_editor = state
        .iter()
        .filter(|(pk, (r, by))| {
            by.as_deref() == Some(hex(&admin).as_str())
                && *r > UserRole::Editor
                && **pk != hex(&admin)
        })
        .count();
    assert_eq!(
        minted_above_editor, 0,
        "an Admin minted a role above Editor"
    );
    assert!(
        state
            .values()
            .all(|(_, by)| by.as_deref() != Some(hex(&editor).as_str())
                && by.as_deref() != Some(hex(&viewer).as_str())),
        "an Editor or Viewer wrote a role"
    );

    // ---- 2. No lost update ---------------------------------------------------
    let (a1, a2) = (Keys::generate(), Keys::generate());
    seed(&store, &a1, UserRole::Admin).await;
    seed(&store, &a2, UserRole::Admin).await;
    let mut winners: HashMap<&str, usize> = HashMap::new();
    for (w1, w2) in (0..4).flat_map(|x| (0..4).map(move |y| (x, y))) {
        let t = Keys::generate();
        let got = fire!(
            app,
            &[
                put(&a1, &hex(&t), "viewer").after(w1),
                put(&a2, &hex(&t), "editor").after(w2),
            ],
        );
        assert_eq!(
            got,
            vec![StatusCode::OK, StatusCode::OK],
            "both writers are authorised"
        );
        let state = roles(&store).await;
        let (role, by) = &state[&hex(&t)];
        match (role, by.as_deref()) {
            (UserRole::Viewer, Some(b)) if b == hex(&a1) => *winners.entry("a1").or_default() += 1,
            (UserRole::Editor, Some(b)) if b == hex(&a2) => *winners.entry("a2").or_default() += 1,
            other => panic!("torn or lost update: {other:?}"),
        }
    }
    println!("lost-update rounds, winners: {winners:?}");
    assert_eq!(
        winners.len(),
        2,
        "the sweep must let each writer win: {winners:?}"
    );

    // ---- 3. Caller freshness under concurrent demotion -----------------------
    let mut outcomes: HashMap<&str, usize> = HashMap::new();
    for (demote_after, grant_after) in (0..6).flat_map(|d| (0..6).map(move |g| (d, g))) {
        let (a, t) = (Keys::generate(), Keys::generate());
        seed(&store, &a, UserRole::Admin).await;
        let got = fire!(
            app,
            &[
                put(&root, &hex(&a), "viewer").after(demote_after),
                put(&a, &hex(&t), "editor").after(grant_after),
            ],
        );
        let state = roles(&store).await;
        assert_eq!(
            got[0],
            StatusCode::OK,
            "the Owner's demotion always applies"
        );
        assert_eq!(
            state[&hex(&a)].0,
            UserRole::Viewer,
            "the demotion must stand"
        );
        match got[1] {
            StatusCode::OK => {
                // Caller committed first: the grant is fully applied.
                assert_eq!(state[&hex(&t)].0, UserRole::Editor);
                *outcomes.entry("admin-first").or_default() += 1;
            }
            StatusCode::CONFLICT | StatusCode::FORBIDDEN => {
                // Demotion won (409 mid-request, or 403 at admission): the
                // grant must not exist — a demoted caller wrote nothing.
                assert!(
                    !state.contains_key(&hex(&t)),
                    "a demoted caller's mutation was applied ({})",
                    got[1]
                );
                let key = if got[1] == StatusCode::CONFLICT {
                    "demoted-mid-transaction-409"
                } else {
                    "demoted-before-admission-403"
                };
                *outcomes.entry(key).or_default() += 1;
            }
            other => panic!("non-serialisable outcome {other}"),
        }
    }
    println!("caller-freshness rounds: {outcomes:?}");
    // Guard against a vacuous sweep: every serialisable outcome, including the
    // mid-transaction `CallerAuthorityChanged` 409, must actually occur.
    for key in [
        "admin-first",
        "demoted-mid-transaction-409",
        "demoted-before-admission-403",
    ] {
        assert!(
            outcomes.contains_key(key),
            "outcome {key} never exercised: {outcomes:?}"
        );
    }

    // ---- 4. Last-Owner guard -------------------------------------------------
    seed(&store, &root, UserRole::Admin).await; // leave the Owner population to this scenario
    type Pair = fn(&Keys, &Keys) -> [Mutation; 2];
    let variants: [(&str, Pair); 3] = [
        ("mutual demotion", |o1, o2| {
            [put(o1, &hex(o2), "viewer"), put(o2, &hex(o1), "viewer")]
        }),
        ("mutual removal", |o1, o2| {
            [del(o1, &hex(o2)), del(o2, &hex(o1))]
        }),
        ("double self-demotion", |o1, o2| {
            [put(o1, &hex(o1), "viewer"), put(o2, &hex(o2), "viewer")]
        }),
    ];
    for (name, make) in variants {
        for lag in 0..6 {
            let (o1, o2) = (Keys::generate(), Keys::generate());
            seed(&store, &o1, UserRole::Owner).await;
            seed(&store, &o2, UserRole::Owner).await;
            let [first, second] = make(&o1, &o2);
            let got = fire!(app, &[first, second.after(lag)]);
            let state = roles(&store).await;
            let owners = state
                .values()
                .filter(|(r, _)| *r == UserRole::Owner)
                .count();
            assert_eq!(
                owners, 1,
                "{name}: exactly one Owner must remain, got {owners} ({got:?})"
            );
            let ok = got.iter().filter(|s| **s == StatusCode::OK).count();
            assert_eq!(
                ok, 1,
                "{name}: exactly one request may succeed, got {got:?}"
            );
            assert!(
                got.iter().all(|s| matches!(
                    *s,
                    StatusCode::OK | StatusCode::CONFLICT | StatusCode::FORBIDDEN
                )),
                "{name}: unexpected status in {got:?}"
            );
            // Clear the survivor so the next round again starts with two Owners.
            for k in [&o1, &o2] {
                seed(&store, k, UserRole::Viewer).await;
            }
        }
    }
}
