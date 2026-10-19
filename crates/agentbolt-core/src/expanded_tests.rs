//! Expanded-plan behavior through the service boundary. No installed daemon or network is used.
use super::*;
use crate::board::{BoardOp, PostInput, RequestInput};
use crate::holds::{Config, HoldOp, Scan, WorkingCopy};
use crate::phone::{self, ResultValue, Secret, Work};
use crate::protocol::{
    DeliveryPage, DeliveryQuery, DirectoryPage, DirectoryQuery, HistoryQuery, HistoryResult,
    InboxResult,
};
use std::collections::HashSet;

fn board(t: &T, handle: &str, action: BoardOp) -> Result<Value, String> {
    t.mcp(McpOp::Board {
        handle: handle.into(),
        action,
    })
    .map(|r| r.0)
}

#[test]
fn board_message_machine_survives_cross_machine_dedupe_and_sender_expiry() {
    let t = T::new();
    let remote: RegisterResult = de(t
        .mcp(McpOp::RegisterRemote {
            machine: "laptop".into(),
            registration: crate::protocol::RemoteRegistration {
                owner: Some("shared-messages".into()),
                client: "remote-fixture".into(),
                session_ref: None,
                cwd: None,
                doing: None,
                provisional_key: None,
            },
        })
        .unwrap()
        .0);
    let local = t.register("shared-messages");
    let input = PostInput {
        to: Default::default(),
        title: "Read the originating file".into(),
        body: "Fixture evidence".into(),
        references: vec![
            "C:/DEV/evidence.md".into(),
            "https://example.com/evidence".into(),
        ],
        thread: None,
        dedupe_key: Some("same-message".into()),
    };
    let posted = board(
        &t,
        &remote.handle,
        BoardOp::Post {
            input: input.clone(),
        },
    )
    .unwrap();
    let deduped = board(&t, &local, BoardOp::Post { input }).unwrap();
    assert_eq!(deduped["id"], posted["id"]);
    assert_eq!(deduped["existing"], true);
    board(
        &t,
        &local,
        BoardOp::Post {
            input: PostInput {
                to: Default::default(),
                title: "Local evidence".into(),
                body: "Same path, different computer".into(),
                references: vec!["C:/DEV/evidence.md".into()],
                thread: None,
                dedupe_key: None,
            },
        },
    )
    .unwrap();
    t.svc
        .conn_for_tests()
        .execute("DELETE FROM agents WHERE handle=?1", [&remote.handle])
        .unwrap();
    let read = board(
        &t,
        &local,
        BoardOp::Read {
            after: 0,
            thread: None,
        },
    )
    .unwrap();
    assert_eq!(read["messages"][0]["sender_machine"], "remote:laptop");
    assert_eq!(read["messages"][1]["sender_machine"], "testbox");
    let snapshot: crate::board::Snapshot = de(t.desk(DeskOp::Board).unwrap().0);
    let message = snapshot
        .messages
        .iter()
        .find(|m| json!(m.id) == posted["id"])
        .unwrap();
    assert_eq!(message.sender_machine.as_deref(), Some("remote:laptop"));
    assert!(!json!(message).to_string().contains(&remote.handle));
    let mut legacy = json!(message);
    legacy.as_object_mut().unwrap().remove("sender_machine");
    let legacy: crate::board::BoardMessage = de(legacy);
    assert_eq!(legacy.sender_machine, None);
}
fn request(t: &T, handle: &str, title: &str) -> Value {
    board(
        t,
        handle,
        BoardOp::Request {
            input: RequestInput {
                title: title.into(),
                body: "A concrete handoff".into(),
                to_role: None,
                repo: Some("example/project".into()),
                paths: vec!["src/**".into()],
                dedupe_key: Some(title.into()),
                beads_issue: None,
            },
        },
    )
    .unwrap()
}
fn holds(t: &T, handle: &str, action: HoldOp) -> Result<Value, String> {
    t.mcp(McpOp::Holds {
        handle: handle.into(),
        action,
    })
    .map(|r| r.0)
}
fn configure_copy(t: &T) {
    t.desk(DeskOp::SetHolds {
        config: Config {
            working_copies: vec![WorkingCopy {
                repo: "example/project".into(),
                path: "C:/test/working-copy".into(),
            }],
            excluded_paths: vec![],
        },
    })
    .unwrap();
    t.svc
        .save_hold_scan(Scan {
            path: "C:/test/working-copy".into(),
            repo: "example/project".into(),
            at_ms: store::to_ms(t.now()),
            ..Default::default()
        })
        .unwrap();
}

#[test]
fn desk_handoffs_resolve_claimants_outside_the_agent_page_and_after_inactivity() {
    let t = T::new();
    let requester = t.register("requester");
    for i in 0..55 {
        t.register(&format!("other-{i}"));
    }
    let claimant = t.register("off-page-reviewer");
    let id = request(&t, &requester, "Off-page claimant")["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let fence = board(&t, &claimant, BoardOp::Claim { id: id.clone() }).unwrap()["fence"]
        .as_u64()
        .unwrap();
    let snapshot: crate::board::Snapshot = de(t.desk(DeskOp::Board).unwrap().0);
    assert_eq!(snapshot.agents.len(), 50);
    assert!(
        snapshot
            .agents
            .iter()
            .all(|a| a.owner != "off-page-reviewer")
    );
    assert_eq!(snapshot.claimants.len(), 1);
    assert_eq!(snapshot.claimants[0].owner, "off-page-reviewer");
    assert_eq!(
        snapshot.claimants[0].session_ref.as_deref(),
        Some("off-page-reviewer-session")
    );
    assert_eq!(
        snapshot.claimants[0].id,
        snapshot.requests[0].claimant.clone().unwrap()
    );
    assert!(
        !json!(snapshot).to_string().contains(&claimant),
        "no bearer handle in Desk cards"
    );
    board(
        &t,
        &claimant,
        BoardOp::Done {
            id,
            fence,
            result: "Reviewed".into(),
            failed: false,
        },
    )
    .unwrap();
    t.advance(Duration::hours(5));
    let snapshot: crate::board::Snapshot = de(t
        .desk(DeskOp::BoardPage {
            query: crate::board::DeskQuery {
                include_closed: true,
                ..Default::default()
            },
        })
        .unwrap()
        .0);
    assert!(snapshot.agents.is_empty());
    assert_eq!(snapshot.claimants[0].owner, "off-page-reviewer");
    assert!(!snapshot.claimants[0].alive);
}

#[test]
fn twenty_agents_race_for_one_claim_and_stale_fences_never_finish_it() {
    let t = T::new();
    let handles: Vec<_> = (0..20).map(|i| t.register(&format!("racer-{i}"))).collect();
    let id = request(&t, &handles[0], "Race 104")["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let barrier = std::sync::Barrier::new(20);
    let winners = std::thread::scope(|scope| {
        let jobs: Vec<_> = handles
            .iter()
            .map(|handle| {
                scope.spawn(|| {
                    barrier.wait();
                    board(&t, handle, BoardOp::Claim { id: id.clone() })
                        .map(|v| (handle.clone(), v["fence"].as_u64().unwrap()))
                })
            })
            .collect();
        jobs.into_iter()
            .filter_map(|job| job.join().unwrap().ok())
            .collect::<Vec<_>>()
    });
    assert_eq!(winners.len(), 1, "exactly one atomic claimant");
    let (first, fence) = &winners[0];
    t.advance(Duration::hours(4) + Duration::seconds(1));
    let later = handles.iter().find(|h| *h != first).unwrap();
    let claim = board(&t, later, BoardOp::Claim { id: id.clone() }).unwrap();
    let new_fence = claim["fence"].as_u64().unwrap();
    assert!(new_fence > *fence);
    for op in [
        BoardOp::Renew {
            id: id.clone(),
            fence: *fence,
        },
        BoardOp::Done {
            id: id.clone(),
            fence: *fence,
            result: "late".into(),
            failed: false,
        },
    ] {
        assert!(board(&t, first, op).unwrap_err().contains("stale"));
    }
    let done = BoardOp::Done {
        id: id.clone(),
        fence: new_fence,
        result: "Dependency unavailable".into(),
        failed: true,
    };
    assert_eq!(
        board(&t, later, done.clone()).unwrap()["request"]["status"],
        "failed"
    );
    assert_eq!(board(&t, later, done).unwrap()["existing"], true);
    assert!(
        board(
            &t,
            later,
            BoardOp::Done {
                id,
                fence: new_fence,
                result: "Different result".into(),
                failed: true
            }
        )
        .is_err()
    );
    assert!(t.status().chain_ok);
}

#[test]
fn owner_caps_dedupe_profile_preservation_and_handle_liveness_are_independent() {
    let t = T::new();
    let a = t.register("builder");
    let other = t.register("builder");
    board(
        &t,
        &a,
        BoardOp::Profile {
            roles: Some(vec!["builder".into()]),
            repo: Some("example/project".into()),
            clear_repo: false,
        },
    )
    .unwrap();
    let profile = board(
        &t,
        &a,
        BoardOp::Profile {
            roles: None,
            repo: None,
            clear_repo: false,
        },
    )
    .unwrap();
    assert_eq!(profile["roles"], json!(["builder"]));
    assert_eq!(profile["repo"], "example/project");
    for i in 0..10 {
        request(&t, &a, &format!("Distinct job {i}"));
    }
    assert_eq!(request(&t, &other, "Distinct job 0")["existing"], true);
    let extra = BoardOp::Request {
        input: RequestInput {
            title: "Distinct job 11".into(),
            body: "Extra".into(),
            to_role: None,
            repo: None,
            paths: vec![],
            dedupe_key: None,
            beads_issue: None,
        },
    };
    assert!(board(&t, &other, extra).unwrap_err().contains("10 open"));
    let id = request(&t, &a, "Distinct job 0")["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let fence = board(&t, &a, BoardOp::Claim { id: id.clone() }).unwrap()["fence"]
        .as_u64()
        .unwrap();
    t.advance(Duration::hours(3));
    board(&t, &other, BoardOp::Who).unwrap();
    t.advance(Duration::hours(1) + Duration::seconds(1));
    assert!(
        board(
            &t,
            &a,
            BoardOp::Done {
                id,
                fence,
                result: "Too late".into(),
                failed: false
            }
        )
        .is_err(),
        "another handle cannot keep this claim alive"
    );
    let cleared = board(
        &t,
        &a,
        BoardOp::Profile {
            roles: Some(vec![]),
            repo: None,
            clear_repo: true,
        },
    )
    .unwrap();
    assert_eq!(cleared["roles"], json!([]));
    assert!(cleared["repo"].is_null());
}

#[test]
fn inbox_pages_do_not_start_unseen_defaults_and_draft_reconciliation_is_read_only() {
    let t = T::new();
    let mut all = HashSet::new();
    for i in 0..55 {
        let h = t.register(&format!("inbox-{i}"));
        all.insert(t.ask(&h, t.input(&format!("Question {i}"))).id);
    }
    let first: InboxResult = de(t.desk(DeskOp::InboxPage { after: None }).unwrap().0);
    assert_eq!(first.inbox.len(), 50);
    assert!(first.more);
    assert_eq!(first.total, 55);
    assert!(first.inbox.iter().all(|a| a.shown_at.is_some()));
    t.advance(Duration::days(2));
    t.tick();
    let second: InboxResult = de(t.desk(DeskOp::InboxPage { after: first.next }).unwrap().0);
    assert_eq!(
        second.inbox.len(),
        5,
        "off-page asks must not have defaulted unseen"
    );
    assert!(!second.more);
    assert_eq!(second.total, 5);
    let ids: HashSet<_> = first
        .inbox
        .iter()
        .chain(&second.inbox)
        .map(|a| a.id.clone())
        .collect();
    assert_eq!(ids, all);
    let states: crate::protocol::DraftStates = de(t
        .desk(DeskOp::DraftStates {
            ids: vec![
                first.inbox[0].id.clone(),
                second.inbox[0].id.clone(),
                "unknown".into(),
            ],
        })
        .unwrap()
        .0);
    assert_eq!(states.closed, vec![first.inbox[0].id.clone()]);
    assert!(
        t.desk(DeskOp::DraftStates {
            ids: vec!["x".into(); 101]
        })
        .is_err()
    );
}

#[test]
fn answer_history_and_relay_filters_reach_records_beyond_first_pages() {
    let t = T::new();
    let a = t.register("recipient");
    let relay = t.register("relay");
    let mut owner = store::owner(&t.svc.conn_for_tests(), "relay")
        .unwrap()
        .unwrap();
    owner.relay = true;
    t.desk(DeskOp::SetOwner { owner }).unwrap();
    let mut expected = HashSet::new();
    for i in 0..73 {
        let id = t.ask(&a, t.input(&format!("Answered {i}"))).id;
        t.answer(&id, "No").unwrap();
        expected.insert(id);
        t.advance(Duration::seconds(1));
    }
    let mut seen = HashSet::new();
    let mut before = None;
    loop {
        let page: AnswersResult = de(t
            .mcp(McpOp::AnswersPage {
                handle: a.clone(),
                before,
                wait_ms: 0,
            })
            .unwrap()
            .0);
        for ask in page.answers {
            assert!(seen.insert(ask.id));
        }
        if !page.more {
            break;
        }
        before = page.next;
    }
    assert_eq!(seen, expected);
    let mut before = None;
    let mut seen = HashSet::new();
    loop {
        let page: HistoryResult = de(t
            .desk(DeskOp::History {
                query: HistoryQuery {
                    before,
                    search: "recipient".into(),
                },
            })
            .unwrap()
            .0);
        assert_eq!(page.total, 73);
        for ask in page.asks {
            assert!(seen.insert(ask.id));
        }
        if !page.more {
            break;
        }
        before = page.next;
    }
    assert_eq!(seen, expected);
    let noise = t.register("other-recipient");
    for i in 0..110 {
        let id = t.ask(&noise, t.input(&format!("Noise {i}"))).id;
        t.answer(&id, "Yes").unwrap();
        t.advance(Duration::seconds(1));
    }
    let filtered = t
        .mcp(McpOp::RelayPage {
            handle: relay,
            owners: vec!["recipient".into()],
            before: None,
        })
        .unwrap()
        .0;
    assert_eq!(filtered["total"], 73);
    assert_eq!(filtered["answers"].as_array().unwrap().len(), 73);
    assert!(t.status().chain_ok);
}

#[test]
fn complete_delivery_history_stays_owner_scoped_and_bounded_in_answers() {
    let t = T::new();
    let a = t.register("delivery-owner");
    let stranger = t.register("stranger");
    let id = t.ask(&a, t.input("Delivery test")).id;
    t.answer(&id, "No").unwrap();
    for i in 0..125 {
        t.svc
            .record_delivery(&id, &format!("target-{i}"), false, Some("Not delivered"))
            .unwrap();
    }
    let answer = t.answers(&a, &[&id]).answers.remove(0).answer.unwrap();
    assert_eq!(answer.delivery_total, 125);
    assert_eq!(answer.deliveries.len(), 50);
    assert!(answer.more_deliveries);
    let mut next = None;
    let mut seen = HashSet::new();
    loop {
        let query = DeliveryQuery {
            ask_id: id.clone(),
            before: next,
        };
        let page: DeliveryPage = de(t
            .mcp(McpOp::DeliveryHistory {
                handle: a.clone(),
                query: query.clone(),
            })
            .unwrap()
            .0);
        let desk: DeliveryPage = de(t
            .desk(DeskOp::DeliveryHistory {
                query: query.clone(),
            })
            .unwrap()
            .0);
        assert_eq!(page.deliveries, desk.deliveries);
        assert_eq!(page.total, 125);
        assert!(
            t.mcp(McpOp::DeliveryHistory {
                handle: stranger.clone(),
                query
            })
            .is_err()
        );
        for record in page.deliveries {
            assert!(seen.insert(record.to));
        }
        if !page.more {
            break;
        }
        next = page.next;
    }
    assert_eq!(seen.len(), 125);
}

#[test]
fn directories_reach_older_owners_and_sessions_without_loading_them_all() {
    let t = T::new();
    for i in 0..115 {
        t.register(&format!("directory-{i:03}"));
    }
    let mut query = DirectoryQuery::default();
    let mut seen = HashSet::new();
    loop {
        let page: DirectoryPage<Owner> = de(t
            .desk(DeskOp::OwnerDirectory {
                query: query.clone(),
            })
            .unwrap()
            .0);
        assert_eq!(page.total, 115);
        for o in page.items {
            assert!(seen.insert(o.name));
        }
        if !page.more {
            break;
        }
        query.after = page.next;
    }
    assert_eq!(seen.len(), 115);
    let page: DirectoryPage<Agent> = de(t
        .desk(DeskOp::AgentDirectory {
            query: DirectoryQuery {
                search: "directory-000".into(),
                after: None,
            },
        })
        .unwrap()
        .0);
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].owner, "directory-000");
}

#[test]
fn collision_checks_examine_all_holds_and_stale_or_excluded_scans_never_clear() {
    let t = T::new();
    configure_copy(&t);
    let checker = t.register("checker");
    for i in 0..61 {
        let h = t.register(&format!("holder-{i}"));
        holds(
            &t,
            &h,
            HoldOp::Acquire {
                repo: "example/project".into(),
                paths: vec!["src/**".into()],
                note: "Work in progress".into(),
                id: None,
            },
        )
        .unwrap();
    }
    let result = holds(
        &t,
        &checker,
        HoldOp::Check {
            repo: "example/project".into(),
            paths: vec!["src/main.rs".into()],
            stage: Some("pr_open".into()),
        },
    )
    .unwrap();
    assert_eq!(result["total_collisions"], 61);
    assert_eq!(result["collisions"].as_array().unwrap().len(), 50);
    assert_eq!(result["more_collisions"], true);
    assert_eq!(result["clear"], false);
    let clear = HoldOp::Check {
        repo: "example/project".into(),
        paths: vec!["README.md".into()],
        stage: None,
    };
    assert_eq!(holds(&t, &checker, clear.clone()).unwrap()["clear"], true);
    t.advance(Duration::minutes(4));
    assert_eq!(holds(&t, &checker, clear).unwrap()["clear"], false);
    assert!(
        t.desk(DeskOp::SetHolds {
            config: Config {
                working_copies: vec![WorkingCopy {
                    repo: "example/project".into(),
                    path: "C:/test/working-copy".into()
                }],
                excluded_paths: vec!["C:/test/working-copy/private".into()]
            }
        })
        .is_err()
    );
}

#[test]
fn globs_keep_directory_boundaries_and_do_not_recurse_with_long_paths() {
    use crate::holds::matches;
    for (pattern, path) in [
        ("**/foo", "foo"),
        ("**/foo", "one/two/foo"),
        ("src/**/*.rs", "src/a.rs"),
        ("src/**/*.rs", "src/deep/a.rs"),
        ("a/?", "a/é"),
    ] {
        assert!(matches(pattern, path), "{pattern} {path}");
    }
    for (pattern, path) in [
        ("**/foo", "barfoo"),
        ("src/*.rs", "src/deep/a.rs"),
        ("a/?", "a//"),
        ("**/foo", "foo/bar"),
    ] {
        assert!(!matches(pattern, path), "{pattern} {path}");
    }
    assert!(matches(
        "**/end",
        &format!("{}end", "component/".repeat(5000))
    ));
}

#[test]
fn routed_message_pages_and_dedupe_preserve_references() {
    use crate::board::Destination;
    let t = T::new();
    let sender = t.register("sender");
    let reader = t.register("reader");
    let outsider = t.register("outsider");
    board(
        &t,
        &reader,
        BoardOp::Profile {
            roles: Some(vec!["reviewer".into()]),
            repo: None,
            clear_repo: false,
        },
    )
    .unwrap();
    for i in 0..60 {
        board(
            &t,
            &sender,
            BoardOp::Post {
                input: PostInput {
                    to: Destination::Role("unrelated".into()),
                    title: format!("Noise {i}"),
                    body: "Not for reader".into(),
                    references: vec![],
                    thread: None,
                    dedupe_key: None,
                },
            },
        )
        .unwrap();
    }
    let op = BoardOp::Post {
        input: PostInput {
            to: Destination::Role("reviewer".into()),
            title: "Review the change".into(),
            body: "Evidence attached".into(),
            references: vec!["https://example.test/change".into()],
            thread: Some("thread-1".into()),
            dedupe_key: Some("review-1".into()),
        },
    };
    let sent = board(&t, &sender, op.clone()).unwrap();
    let retry = board(&t, &sender, op).unwrap();
    assert_eq!(sent["id"], retry["id"]);
    assert_eq!(retry["existing"], true);
    let mut after = 0;
    let mut received = Vec::new();
    loop {
        let page = board(
            &t,
            &reader,
            BoardOp::Read {
                after,
                thread: None,
            },
        )
        .unwrap();
        received.extend(page["messages"].as_array().unwrap().iter().cloned());
        if page["more"] != true {
            break;
        }
        after = page["next"].as_u64().unwrap();
    }
    assert_eq!(received.len(), 1);
    assert_eq!(
        received[0]["references"],
        json!(["https://example.test/change"])
    );
    let page = board(
        &t,
        &outsider,
        BoardOp::Read {
            after: 60,
            thread: Some("thread-1".into()),
        },
    )
    .unwrap();
    assert!(page["messages"].as_array().unwrap().is_empty());
}

#[test]
fn active_handle_renews_claims_and_holds_but_shorter_ceiling_takes_effect() {
    let t = T::new();
    configure_copy(&t);
    let a = t.register("lease-owner");
    let b = t.register("lease-observer");
    let id = request(&t, &a, "Renewable job 42")["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let fence = board(&t, &a, BoardOp::Claim { id: id.clone() }).unwrap()["fence"]
        .as_u64()
        .unwrap();
    let hold = holds(
        &t,
        &a,
        HoldOp::Acquire {
            repo: "example/project".into(),
            paths: vec!["src/**".into()],
            note: "working".into(),
            id: None,
        },
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(holds(&t, &b, HoldOp::Release { id: hold.clone() }).is_err());
    t.advance(Duration::hours(3));
    board(&t, &a, BoardOp::Who).unwrap();
    t.advance(Duration::hours(2));
    assert_eq!(
        board(&t, &b, BoardOp::Get { id: id.clone() }).unwrap()["request"]["status"],
        "claimed"
    );
    let page = holds(&t, &b, HoldOp::List).unwrap();
    assert!(
        page["holds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == hold)
    );
    t.desk(DeskOp::SetLease {
        owner: "lease-owner".into(),
        seconds: 60,
    })
    .unwrap();
    assert!(
        board(
            &t,
            &a,
            BoardOp::Done {
                id,
                fence,
                result: "too late after shorter ceiling".into(),
                failed: false
            }
        )
        .is_err()
    );
    let page = holds(&t, &b, HoldOp::List).unwrap();
    assert!(
        !page["holds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == hold)
    );
}

#[test]
fn timed_mute_expires_but_indefinite_mute_survives_time() {
    let t = T::new();
    let a = t.register("timed-mute");
    t.ask(&a, t.input("Muted question"));
    t.desk(DeskOp::MuteFor {
        owner: "timed-mute".into(),
        seconds: 60,
    })
    .unwrap();
    assert!(t.list().inbox[0].muted);
    t.advance(Duration::seconds(61));
    t.tick();
    assert!(!t.list().inbox[0].muted);
    t.desk(DeskOp::SetMuted {
        owner: "timed-mute".into(),
        muted: true,
    })
    .unwrap();
    t.advance(Duration::days(10));
    t.tick();
    assert!(
        store::owner(&t.svc.conn_for_tests(), "timed-mute")
            .unwrap()
            .unwrap()
            .muted
    );
}

fn enable_phone(t: &T) -> String {
    let update = phone::Update {
        settings: phone::Settings {
            enabled: true,
            server: "https://push.example.test".into(),
            questions: "questions".into(),
            replies: "replies".into(),
        },
        daemon_token: Some(Secret("daemon-test-token".into())),
        reply_token: Some(Secret("reply-test-token".into())),
        forget_credentials: false,
    };
    let status = t.desk(DeskOp::SetPhone { config: update }).unwrap().0;
    assert!(!status.to_string().contains("test-token"));
    let ResultValue::State(state) = t.svc.phone_work("", Work::State).unwrap() else {
        panic!("state")
    };
    t.svc
        .phone_work(&state.config.generation, Work::Verified)
        .unwrap();
    state.config.generation
}

#[test]
fn phone_receipts_cannot_answer_until_desk_consumption_and_tokens_are_purpose_bound() {
    let t = T::new();
    let generation = enable_phone(&t);
    let a = t.register("phone-owner");
    let mut input = t.input("Phone choice");
    input.priority = Priority::P1;
    let id = t.ask(&a, input).id;
    let ResultValue::Notification(Some(notification)) =
        t.svc.phone_work(&generation, Work::Prepare).unwrap()
    else {
        panic!("notification")
    };
    assert_eq!(notification.ask_id, id);
    assert_eq!(notification.buttons.len(), 2);
    let token = notification.buttons[1].1.clone();
    assert!(
        t.token(&token.0).is_err(),
        "phone token must not work as a toast"
    );
    t.svc
        .phone_work(
            &generation,
            Work::Receive {
                messages: vec![
                    phone::Incoming {
                        id: "receipt1".into(),
                        token: token.clone(),
                    },
                    phone::Incoming {
                        id: "receipt2".into(),
                        token,
                    },
                ],
                cursor: Some("receipt2".into()),
            },
        )
        .unwrap();
    assert!(
        t.answers(&a, &[&id]).answers.is_empty(),
        "network work cannot answer"
    );
    let ResultValue::Pending { ids, .. } = t.svc.phone_work(&generation, Work::Pending).unwrap()
    else {
        panic!("pending")
    };
    assert_eq!(ids, vec!["receipt1"]);
    let op = DeskOp::PhoneReply {
        id: "receipt1".into(),
    };
    assert!(serde_json::from_value::<McpOp>(json!(op)).is_err());
    t.desk(op.clone()).unwrap();
    t.desk(op).unwrap();
    let answer = t.answers(&a, &[&id]).answers.remove(0).answer.unwrap();
    assert_eq!(answer.via, AnswerVia::Phone);
    assert_eq!(answer.choice.as_deref(), Some("No"));
    assert!(t.status().chain_ok);
}

#[test]
fn queued_phone_reply_cannot_replace_an_answer_given_elsewhere() {
    let t = T::new();
    let generation = enable_phone(&t);
    let a = t.register("phone-race");
    let mut input = t.input("Race a phone reply");
    input.priority = Priority::P1;
    let id = t.ask(&a, input).id;
    let ResultValue::Notification(Some(notification)) =
        t.svc.phone_work(&generation, Work::Prepare).unwrap()
    else {
        panic!("notification")
    };
    t.svc
        .phone_work(
            &generation,
            Work::Receive {
                messages: vec![phone::Incoming {
                    id: "late1".into(),
                    token: notification.buttons[0].1.clone(),
                }],
                cursor: Some("late1".into()),
            },
        )
        .unwrap();
    t.answer(&id, "No").unwrap();
    t.desk(DeskOp::PhoneReply { id: "late1".into() }).unwrap();
    let answer = t.answers(&a, &[&id]).answers.remove(0).answer.unwrap();
    assert_eq!(answer.via, AnswerVia::Desk);
    assert_eq!(answer.choice.as_deref(), Some("No"));
    let ResultValue::Pending {
        acknowledgements, ..
    } = t.svc.phone_work(&generation, Work::Pending).unwrap()
    else {
        panic!("pending")
    };
    assert!(acknowledgements[0].outcome.contains("not applied"));
    assert!(t.status().chain_ok);
}

#[test]
fn phone_settings_change_allows_a_fresh_reply_after_revoking_a_queued_one() {
    let t = T::new();
    let old_generation = enable_phone(&t);
    let a = t.register("phone-reissue");
    let mut input = t.input("Reply after settings change");
    input.priority = Priority::P1;
    let id = t.ask(&a, input).id;
    let ResultValue::Notification(Some(old)) =
        t.svc.phone_work(&old_generation, Work::Prepare).unwrap()
    else {
        panic!("old notification")
    };
    t.svc
        .phone_work(
            &old_generation,
            Work::Receive {
                messages: vec![phone::Incoming {
                    id: "oldreceipt".into(),
                    token: old.buttons[0].1.clone(),
                }],
                cursor: None,
            },
        )
        .unwrap();
    let generation = enable_phone(&t);
    assert!(t.svc.phone_work(&old_generation, Work::Prepare).is_err());
    let ResultValue::Notification(Some(new)) =
        t.svc.phone_work(&generation, Work::Prepare).unwrap()
    else {
        panic!("fresh notification")
    };
    assert_ne!(old.toast_id, new.toast_id);
    t.svc
        .phone_work(
            &generation,
            Work::Receive {
                messages: vec![
                    phone::Incoming {
                        id: "oldreplay".into(),
                        token: old.buttons[0].1.clone(),
                    },
                    phone::Incoming {
                        id: "freshreceipt".into(),
                        token: new.buttons[1].1.clone(),
                    },
                ],
                cursor: Some("freshreceipt".into()),
            },
        )
        .unwrap();
    let ResultValue::Pending { ids, .. } = t.svc.phone_work(&generation, Work::Pending).unwrap()
    else {
        panic!("pending")
    };
    assert_eq!(ids, vec!["freshreceipt"]);
    t.desk(DeskOp::PhoneReply {
        id: "freshreceipt".into(),
    })
    .unwrap();
    assert_eq!(
        t.answers(&a, &[&id]).answers[0]
            .answer
            .as_ref()
            .unwrap()
            .choice
            .as_deref(),
        Some("No")
    );
}

#[test]
fn activity_before_the_first_stored_event_is_unknown() {
    let t = T::new();
    let a = t.register("new-install");
    t.ask(&a, t.input("First recorded question"));
    t.advance(Duration::seconds(1));
    let report: crate::metrics::Report = de(t.desk(DeskOp::Metrics).unwrap().0);
    assert_eq!(report.current.submitted, 1);
    assert!(
        report
            .current
            .history_span_days
            .is_some_and(|days| days > 0.0)
    );
    assert!(report.previous.history_span_days.is_none());
}

#[test]
fn adoption_is_anchored_and_noise_is_a_share_of_submitted_questions() {
    let t = T::new();
    t.desk(DeskOp::StartBaseline {
        label: "Before rollout".into(),
    })
    .unwrap();
    t.advance(Duration::days(5));
    let a = t.register("measured");
    let id = t.ask(&a, t.input("Unnecessary 1")).id;
    t.desk(DeskOp::NotNeeded {
        ask_id: id,
        note: None,
    })
    .unwrap();
    t.ask(&a, t.input("Necessary 2"));
    t.advance(Duration::days(10));
    let a = t.register("measured");
    t.ask(&a, t.input("Outside observation 3"));
    t.advance(Duration::seconds(1)); // Periods are half-open; include this just-created ask.
    let report: crate::metrics::Report = de(t.desk(DeskOp::Metrics).unwrap().0);
    assert_eq!(report.observation_days, Some(14.0));
    assert_eq!(report.current.submitted, 3);
    let observed = report.observation.unwrap();
    assert_eq!(observed.submitted, 2);
    assert_eq!(observed.owners[0].not_needed_share, Some(0.5));
    assert!(
        t.desk(DeskOp::StartBaseline {
            label: "Overwrite".into()
        })
        .is_err()
    );
}
