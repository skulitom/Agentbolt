//! Tests of `StoreService` against the rules in PLAN.md, through the same `Service` calls the daemon makes.

use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use time::macros::datetime;
use time::{Duration, OffsetDateTime};

use crate::model::{
    Agent, Answer, AnswerVia, AskInput, AskKind, AskStatus, Evidence, Field, Owner, Priority,
    Settings,
};
use crate::protocol::{
    AgentsResult, AnswerResult, AnswersResult, AskResult, DeskOp, DigestResult, ListResult, McpOp,
    OwnersResult, RegisterResult, RelayListResult, StatusResult,
};
use crate::service::{ClientInfo, Effect, Service};
use crate::service_impl::{Clock, StoreService, UNKNOWN_HANDLE};
use crate::{rules, store};

const START: OffsetDateTime = datetime!(2026-10-19 09:00 UTC);

#[test]
fn an_ask_keeps_its_original_chat_after_agents_expire_or_register_elsewhere() {
    let t = T::new();
    let original = "01a1503a-5165-7e62-a9ba-88c35a72cfce";
    let reg = t.register_full(
        Some("persistent-owner"),
        Some(original),
        Some("C:\\original"),
    );
    let mut input = t.input("Durable chat context");
    input.reversible = false;
    let ask = t.ask(&reg.handle, input);
    t.register_full(
        Some("persistent-owner"),
        Some("a-later-session"),
        Some("C:\\different"),
    );
    t.advance(Duration::days(8));
    t.tick();
    let list = t.list();
    let stored = list.inbox.iter().find(|a| a.id == ask.id).unwrap();
    assert_eq!(stored.asking_session.as_deref(), Some(original));
    assert_eq!(stored.asking_client.as_deref(), Some("claude-code"));
    assert_eq!(stored.asking_cwd.as_deref(), Some("C:\\original"));
    let agents: AgentsResult = de(t.desk(DeskOp::Agents).unwrap().0);
    assert!(agents.agents.is_empty());
}

struct T {
    svc: StoreService,
    now: Arc<Mutex<OffsetDateTime>>,
}

fn de<R: DeserializeOwned>(v: Value) -> R {
    serde_json::from_value(v).expect("result decodes")
}

impl T {
    fn new() -> T {
        Self::with(StoreService::open_in_memory().unwrap())
    }

    fn with(svc: StoreService) -> T {
        let now = Arc::new(Mutex::new(START));
        let n = now.clone();
        let clock: Clock = Arc::new(move || *n.lock().unwrap());
        T {
            svc: svc
                .with_clock(clock)
                .with_channel("test-core")
                .with_machine("testbox"),
            now,
        }
    }

    fn now(&self) -> OffsetDateTime {
        *self.now.lock().unwrap()
    }

    fn set(&self, t: OffsetDateTime) {
        *self.now.lock().unwrap() = t;
    }

    fn advance(&self, d: Duration) {
        let t = self.now() + d;
        self.set(t);
    }

    fn tick(&self) -> Vec<Effect> {
        self.svc.tick(self.now())
    }

    fn mcp(&self, op: McpOp) -> Result<(Value, Vec<Effect>), String> {
        self.svc.mcp(
            &ClientInfo {
                pid: None,
                connection: 1,
            },
            op,
        )
    }

    fn desk(&self, op: DeskOp) -> Result<(Value, Vec<Effect>), String> {
        self.svc.desk(op)
    }

    fn register_full(
        &self,
        owner: Option<&str>,
        session: Option<&str>,
        cwd: Option<&str>,
    ) -> RegisterResult {
        let op = McpOp::Register {
            owner: owner.map(String::from),
            client: "claude-code".into(),
            session_ref: session.map(String::from),
            cwd: cwd.map(String::from),
            doing: None,
            provisional_key: None,
        };
        de(self.mcp(op).unwrap().0)
    }

    /// A registration with no owner and no host session, from the MCP process with this key.
    fn register_process(&self, key: Option<&str>) -> RegisterResult {
        let op = McpOp::Register {
            owner: None,
            client: "claude-code".into(),
            session_ref: None,
            cwd: Some("C:\\DEV\\Compass".into()),
            doing: None,
            provisional_key: key.map(String::from),
        };
        de(self.mcp(op).unwrap().0)
    }

    fn register(&self, owner: &str) -> String {
        self.register_full(Some(owner), Some(&format!("{owner}-session")), None)
            .handle
    }

    fn input(&self, title: &str) -> AskInput {
        AskInput {
            kind: AskKind::Choose,
            title: title.into(),
            body: "Some context.".into(),
            evidence: vec![],
            options: vec!["Yes".into(), "No".into()],
            fields: vec![],
            default: json!("Yes"),
            deadline: store::fmt_ms(store::to_ms(self.now() + Duration::days(1))),
            reversible: true,
            priority: Priority::P2,
            p0_reason: None,
            dedupe_key: None,
        }
    }

    fn due_in(&self, mut input: AskInput, d: Duration) -> AskInput {
        input.deadline = store::fmt_ms(store::to_ms(self.now() + d));
        input
    }

    fn try_ask(&self, handle: &str, input: AskInput) -> Result<(AskResult, Vec<Effect>), String> {
        let (v, fx) = self.mcp(McpOp::Ask {
            handle: handle.into(),
            ask: input,
        })?;
        Ok((de(v), fx))
    }

    fn ask(&self, handle: &str, input: AskInput) -> AskResult {
        self.try_ask(handle, input).unwrap().0
    }

    fn placed(&self, handle: &str, title: &str) -> String {
        self.ask(handle, self.input(title)).placed
    }

    fn answer(&self, id: &str, choice: &str) -> Result<(Answer, Vec<Effect>), String> {
        let op = DeskOp::Answer {
            ask_id: id.into(),
            choice: Some(choice.into()),
            values: None,
            note: None,
        };
        let (v, fx) = self.desk(op)?;
        Ok((de::<AnswerResult>(v).answer, fx))
    }

    fn token(&self, token: &str) -> Result<Answer, String> {
        let (v, _) = self.desk(DeskOp::AnswerToken {
            token: token.into(),
        })?;
        Ok(de::<AnswerResult>(v).answer)
    }

    fn list(&self) -> ListResult {
        de(self.desk(DeskOp::List).unwrap().0)
    }

    fn open_digest(&self) -> DigestResult {
        de(self.desk(DeskOp::OpenDigest).unwrap().0)
    }

    fn answers(&self, handle: &str, ids: &[&str]) -> AnswersResult {
        let op = McpOp::Answers {
            handle: handle.into(),
            ids: ids.iter().map(|s| s.to_string()).collect(),
            wait_ms: 0,
        };
        de(self.mcp(op).unwrap().0)
    }

    fn cancel(&self, handle: &str, id: &str) -> Result<Vec<Effect>, String> {
        self.mcp(McpOp::Cancel {
            handle: handle.into(),
            id: id.into(),
        })
        .map(|(_, fx)| fx)
    }

    fn set_budget(&self, n: u32) {
        let s = Settings {
            daily_budget: n,
            ..Settings::default()
        };
        self.desk(DeskOp::SetSettings { settings: s }).unwrap();
    }

    fn set_owner(&self, name: &str, binding: &str, codex_interactive: bool, relay: bool) {
        let owner = Owner {
            name: name.into(),
            binding: binding.into(),
            codex_interactive,
            relay,
        };
        self.desk(DeskOp::SetOwner { owner }).unwrap();
    }

    fn status(&self) -> StatusResult {
        de(self.mcp(McpOp::Status).unwrap().0)
    }

    fn ask_status(&self, id: &str) -> AskStatus {
        let conn = self.svc.conn_for_tests();
        store::ask_row(&conn, id).unwrap().unwrap().status
    }
}

fn toasts(fx: &[Effect]) -> Vec<(String, Vec<(String, String)>)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Toast {
                ask_id, buttons, ..
            } => Some((ask_id.clone(), buttons.clone())),
            _ => None,
        })
        .collect()
}

fn inbox_changes(fx: &[Effect]) -> Vec<(u32, u32)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::InboxChanged { open, digest, .. } => Some((*open, *digest)),
            _ => None,
        })
        .collect()
}

// ---- rule 1: budget, digest, fair share, P0 ----

#[test]
fn over_the_budget_asks_go_to_the_digest_and_are_never_refused() {
    let t = T::new();
    t.set_budget(2);
    let a = t.register("owner-a");
    let b = t.register("owner-b");
    assert_eq!(t.placed(&a, "First"), "open");
    assert_eq!(t.placed(&a, "Second"), "open");
    let held = t.ask(&b, t.input("Third"));
    assert_eq!(held.placed, "digest");
    assert!(held.id.starts_with("ask_") && held.existing.is_none());
    let s = t.status();
    assert_eq!((s.open, s.digest), (2, 1));
    let l = t.list();
    assert_eq!(l.inbox.len(), 2);
    assert_eq!(l.digest_count, 1);
    assert_eq!(t.open_digest().digest[0].id, held.id);
}

#[test]
fn the_fair_share_stops_one_owner_taking_the_day() {
    let t = T::new(); // budget 5
    let (a, b, c) = (
        t.register("owner-a"),
        t.register("owner-b"),
        t.register("owner-c"),
    );
    assert_eq!(
        t.placed(&a, "a1"),
        "open",
        "alone, an owner may take the whole budget"
    );
    assert_eq!(t.placed(&b, "b1"), "open");
    assert_eq!(
        t.placed(&a, "a2"),
        "open",
        "two owners: a share of ceil(5 / 2) = 3 each"
    );
    assert_eq!(t.placed(&a, "a3"), "open");
    assert_eq!(
        t.placed(&a, "a4"),
        "cap",
        "a has 3 open asks: the owner cap answers before the share"
    );
    assert_eq!(t.placed(&b, "b2"), "open");
    assert_eq!(t.placed(&c, "c1"), "digest", "the budget of 5 is used");
    assert_eq!(t.list().inbox.len(), 5);
}

#[test]
fn the_fair_share_holds_an_owner_below_its_cap() {
    let t = T::new();
    t.set_budget(4);
    let (a, b, c) = (
        t.register("owner-a"),
        t.register("owner-b"),
        t.register("owner-c"),
    );
    assert_eq!(t.placed(&a, "a1"), "open");
    assert_eq!(t.placed(&b, "b1"), "open");
    assert_eq!(t.placed(&c, "c1"), "open");
    assert_eq!(
        t.placed(&a, "a2"),
        "open",
        "three owners: a share of ceil(4 / 3) = 2 each"
    );
    assert_eq!(
        t.placed(&a, "a3"),
        "digest",
        "a is at its share of 2, below its cap of 3"
    );
    assert_eq!(t.placed(&b, "b2"), "digest", "the budget of 4 is used");
    assert_eq!(t.list().inbox.len(), 4);
}

#[test]
fn p0_skips_the_budget_once_per_owner_per_day() {
    let t = T::new();
    t.set_budget(1);
    let a = t.register("owner-a");
    let b = t.register("owner-b");
    // Deadlines 3 days out, so nothing defaults when the clock moves a day.
    let p0 = |title: &str| AskInput {
        priority: Priority::P0,
        p0_reason: Some("prod is down".into()),
        ..t.due_in(t.input(title), Duration::days(3))
    };

    let mut no_reason = t.input("No reason");
    no_reason.priority = Priority::P0;
    assert!(t.try_ask(&a, no_reason).unwrap_err().contains("p0_reason"));

    let normal = t.ask(&a, t.input("Normal"));
    assert_eq!(normal.placed, "open");
    let first = t.ask(&a, p0("Urgent one"));
    assert_eq!(first.placed, "open", "P0 skips the full budget");
    let second = t.ask(&a, p0("Urgent two"));
    assert_eq!(
        second.placed, "digest",
        "a second P0 that day is a P1 and meets the budget"
    );
    let held = t.open_digest().digest;
    assert_eq!(held[0].input.priority, Priority::P1);
    assert_eq!(
        held[0].input.p0_reason.as_deref(),
        Some("prod is down"),
        "the stated reason is kept"
    );
    assert_eq!(
        t.ask(&b, p0("Owner b's urgent")).placed,
        "open",
        "the skip is per owner"
    );

    // Next day: the skip is back, and the held ask is promoted first.
    t.answer(&normal.id, "Yes").unwrap();
    t.answer(&first.id, "Yes").unwrap();
    t.advance(Duration::days(1));
    t.tick();
    assert_eq!(t.ask_status(&second.id), AskStatus::Open);
    assert_eq!(t.ask(&a, p0("Urgent three")).placed, "open");
}

#[test]
fn digest_asks_are_promoted_by_deadline_as_the_budget_frees() {
    let t = T::new();
    t.set_budget(1);
    let (a, b, c) = (
        t.register("owner-a"),
        t.register("owner-b"),
        t.register("owner-c"),
    );
    t.ask(&a, t.input("a1"));
    let late = t.ask(&b, t.due_in(t.input("b1"), Duration::days(3)));
    let soon = t.ask(&c, t.due_in(t.input("c1"), Duration::days(2)));
    assert_eq!(
        (late.placed.as_str(), soon.placed.as_str()),
        ("digest", "digest")
    );

    assert!(inbox_changes(&t.tick()).len() <= 1);
    assert_eq!(
        t.ask_status(&soon.id),
        AskStatus::Digest,
        "nothing frees within the day"
    );

    t.advance(Duration::days(1));
    let fx = t.tick();
    assert_eq!(
        t.ask_status(&soon.id),
        AskStatus::Open,
        "the earlier deadline goes first"
    );
    assert_eq!(t.ask_status(&late.id), AskStatus::Digest);
    assert!(inbox_changes(&fx).contains(&(2, 1)), "{fx:?}");

    t.advance(Duration::days(1));
    t.tick();
    assert_eq!(t.ask_status(&late.id), AskStatus::Open);
}

#[test]
fn cancelling_an_unseen_inbox_ask_frees_its_slot() {
    let t = T::new();
    t.set_budget(1);
    let (a, b, c) = (
        t.register("owner-a"),
        t.register("owner-b"),
        t.register("owner-c"),
    );
    let first = t.ask(&a, t.input("a1"));
    let held = t.ask(&b, t.input("b1"));
    assert_eq!(held.placed, "digest");
    let fx = t.cancel(&a, &first.id).unwrap();
    assert_eq!(
        t.ask_status(&held.id),
        AskStatus::Open,
        "the unseen slot went to the digest ask"
    );
    assert!(inbox_changes(&fx).contains(&(1, 0)));

    // Once Artem has seen an Inbox ask, its slot stays used even if it is cancelled.
    t.list();
    let third = t.ask(&c, t.input("c1"));
    assert_eq!(third.placed, "digest");
    t.cancel(&b, &held.id).unwrap();
    assert_eq!(t.ask_status(&third.id), AskStatus::Digest);
}

#[test]
fn the_local_day_follows_the_stored_utc_offset() {
    let t = T::new();
    t.set_budget(1);
    t.set(datetime!(2026-10-19 21:30 UTC));
    let (a, b) = (t.register("owner-a"), t.register("owner-b"));
    t.ask(&a, t.input("a1"));
    let held = t.ask(&b, t.input("b1"));
    t.set(datetime!(2026-10-19 22:30 UTC));
    t.tick();
    assert_eq!(
        t.ask_status(&held.id),
        AskStatus::Digest,
        "still 19 October in UTC"
    );
    t.svc.set_utc_offset_minutes(120).unwrap();
    assert_eq!(t.svc.utc_offset_minutes(), 120);
    t.tick();
    assert_eq!(
        t.ask_status(&held.id),
        AskStatus::Open,
        "00:30 on 20 October at UTC+2"
    );
    assert!(t.svc.set_utc_offset_minutes(15 * 60).is_err());
}

// ---- rule 2: caps and dedupe ----

#[test]
fn an_owner_over_its_cap_gets_its_oldest_open_ask_back() {
    let t = T::new();
    let a = t.register("owner-a");
    let ids: Vec<String> = ["one", "two", "three"]
        .iter()
        .map(|n| t.ask(&a, t.input(n)).id)
        .collect();
    let over = t.ask(&a, t.input("four"));
    assert_eq!(over.placed, "cap");
    assert_eq!(over.id, ids[0]);
    assert_eq!(
        over.existing.as_ref().map(|e| e.id.as_str()),
        Some(ids[0].as_str())
    );
    assert_eq!(t.status().open, 3);
    t.answer(&ids[0], "No").unwrap();
    assert_eq!(
        t.placed(&a, "four"),
        "open",
        "an answered ask no longer counts"
    );
}

#[test]
fn duplicates_return_the_existing_ask() {
    let t = T::new();
    let a = t.register("owner-a");
    let b = t.register("owner-b");
    let first = t.ask(&a, t.input("Ship it?"));
    let again = t.ask(&a, t.input("  ship IT "));
    assert_eq!(
        (again.placed.as_str(), again.id.as_str()),
        ("existing", first.id.as_str())
    );

    let keyed = t.ask(
        &a,
        AskInput {
            dedupe_key: Some("deploy-42".into()),
            ..t.input("Deploy now?")
        },
    );
    assert_eq!(keyed.placed, "open");
    let same_key = t.ask(
        &a,
        AskInput {
            dedupe_key: Some("deploy-42".into()),
            ..t.input("Different words")
        },
    );
    assert_eq!(
        (same_key.placed.as_str(), same_key.id.as_str()),
        ("existing", keyed.id.as_str())
    );

    assert_eq!(
        t.ask(&b, t.input("Ship it?")).placed,
        "open",
        "dedupe is per owner"
    );
    t.answer(&first.id, "Yes").unwrap();
    let fresh = t.ask(&a, t.input("Ship it?"));
    assert_ne!(fresh.placed, "existing", "only open and digest asks dedupe");
    assert_ne!(fresh.id, first.id);
}

// ---- rule 3: owners ----

#[test]
fn provisional_owners_are_per_session_even_in_the_same_folder() {
    let t = T::new();
    let cwd = Some("C:\\DEV\\Compass");
    let s1 = t.register_full(None, Some("routine-run-1"), cwd);
    let s2 = t.register_full(None, Some("routine-run-2"), cwd);
    assert_eq!(s1.owner, "session:routine-run-1");
    assert_ne!(s1.owner, s2.owner);
    assert!(s1.provisional && s2.provisional);
    let anon1 = t.register_full(None, None, cwd);
    let anon2 = t.register_full(None, None, cwd);
    assert_ne!(
        anon1.owner, anon2.owner,
        "no session ref and no process key: a fresh random owner each time"
    );
    assert_eq!(
        t.register_full(None, Some("routine-run-1"), None).owner,
        s1.owner,
        "the same session, the same owner"
    );

    // No host session: one provisional owner per MCP process, however often it registers.
    let p1 = t.register_process(Some("mcp-0a1b"));
    assert_eq!(p1.owner, "session:mcp-0a1b");
    assert_eq!(t.register_process(Some("mcp-0a1b")).owner, p1.owner);
    assert_ne!(
        t.register_process(Some("mcp-9f9f")).owner,
        p1.owner,
        "another process, another owner"
    );

    let asked = t.ask(&s1.handle, t.input("Mine"));
    let other = t.answers(&s2.handle, &[&asked.id]);
    assert_eq!(
        other.unknown,
        vec![asked.id.clone()],
        "another session can't see it"
    );
    assert!(other.answers.is_empty() && other.waiting.is_empty());
}

#[test]
fn config_owners_are_bound_and_the_desk_can_approve_provisional_ones() {
    let t = T::new();
    let r = t.register_full(
        Some("compass-storefront"),
        Some("run-9"),
        Some("C:\\DEV\\Compass"),
    );
    assert_eq!(
        (r.owner.as_str(), r.provisional),
        ("compass-storefront", false)
    );
    let owners: OwnersResult = de(t.desk(DeskOp::Owners).unwrap().0);
    let o = owners
        .owners
        .iter()
        .find(|o| o.name == "compass-storefront")
        .unwrap();
    assert_eq!(o.binding, "config");
    assert!(!o.codex_interactive && !o.relay);

    let claim = McpOp::Register {
        owner: Some("session:someone-else".into()),
        client: "x".into(),
        session_ref: None,
        cwd: None,
        doing: None,
        provisional_key: None,
    };
    assert!(t.mcp(claim).unwrap_err().contains("session:"));

    let p = t.register_full(None, Some("artem-chat"), None);
    assert!(p.provisional);
    t.set_owner(&p.owner, "approved", false, false);
    assert!(!t.register_full(None, Some("artem-chat"), None).provisional);
    let agents: AgentsResult = de(t.desk(DeskOp::Agents).unwrap().0);
    assert!(
        agents
            .agents
            .iter()
            .filter(|a| a.owner == p.owner)
            .all(|a| !a.provisional)
    );
    assert!(
        t.desk(DeskOp::SetOwner {
            owner: Owner {
                binding: "boss".into(),
                ..o.clone()
            }
        })
        .is_err()
    );
}

#[test]
fn provisional_owners_get_no_p0_skip_and_share_one_fair_share() {
    let t = T::new();
    t.set_budget(4);
    let bound = t.register("compass-storefront");
    // Five fresh provisional owners, as an agent minting a new one per register would get.
    let fresh: Vec<RegisterResult> = (0..5)
        .map(|i| t.register_full(None, Some(&format!("s{i}")), None))
        .collect();
    assert!(fresh.iter().all(|r| r.provisional));

    let p0 = |title: &str| AskInput {
        priority: Priority::P0,
        p0_reason: Some("urgent".into()),
        ..t.input(title)
    };
    let (r, _) = t.try_ask(&fresh[0].handle, p0("Provisional P0")).unwrap();
    let note = r.note.clone().unwrap_or_default();
    assert!(
        note.contains("P1") && note.contains("provisional"),
        "{note}"
    );
    let conn_priority = |id: &str| {
        let conn = t.svc.conn_for_tests();
        store::ask_row(&conn, id).unwrap().unwrap().input.priority
    };
    assert_eq!(conn_priority(&r.id), Priority::P1, "stored as P1");
    {
        let conn = t.svc.conn_for_tests();
        let skip: bool = conn
            .query_row("SELECT p0_skip FROM asks WHERE id = ?1", [&r.id], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(!skip, "no P0 skip for a provisional owner");
    }
    assert_eq!(
        r.placed, "open",
        "it took a normal slot of the provisional bucket"
    );

    // Two buckets are active (the bound owner and "provisional"): a share of ceil(4 / 2) = 2 each, for all the
    // provisional owners together.
    assert_eq!(t.placed(&bound, "b1"), "open");
    assert_eq!(
        t.placed(&fresh[1].handle, "f1"),
        "open",
        "the provisional bucket's second slot"
    );
    for (i, f) in fresh.iter().enumerate().skip(2) {
        assert_eq!(
            t.placed(&f.handle, &format!("f{i}")),
            "digest",
            "a fresh owner buys no fresh share"
        );
    }
    assert_eq!(
        t.placed(&bound, "b2"),
        "open",
        "the bound owner still has its share"
    );
    assert_eq!(t.list().inbox.len(), 4);

    // The cap stays per owner: three each, provisional or not.
    let one = &fresh[0].handle;
    assert_eq!(t.placed(one, "x2"), "digest");
    assert_eq!(t.placed(one, "x3"), "digest");
    assert_eq!(t.placed(one, "x4"), "cap");

    // Once Artem approves an owner it is out of the bucket and may use its P0 skip.
    t.set_owner(&fresh[4].owner, "approved", false, false);
    let (r, _) = t.try_ask(&fresh[4].handle, p0("Approved P0")).unwrap();
    assert_eq!((r.placed.as_str(), r.note.as_deref()), ("open", None));
    assert_eq!(conn_priority(&r.id), Priority::P0);
}

#[test]
fn the_desk_owner_list_is_bounded_and_idle_provisional_owners_are_pruned() {
    let t = T::new();
    t.register("compass-storefront");
    t.set_owner("approved-one", "approved", false, false);
    let first = t.register_full(None, Some("first-session"), None);
    let asked = t.ask(&first.handle, t.input("Keeps its owner"));
    for i in 0..60 {
        t.advance(Duration::seconds(1));
        t.register_full(None, Some(&format!("run-{i:02}")), None);
    }
    let owners: OwnersResult = de(t.desk(DeskOp::Owners).unwrap().0);
    let names: Vec<&str> = owners.owners.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        &names[..2],
        ["approved-one", "compass-storefront"],
        "every bound or approved owner, by name"
    );
    assert_eq!(names.len(), 2 + 50, "at most 50 provisional owners");
    assert_eq!(names[2], "session:run-59", "the most recent first");
    assert_eq!(owners.more_provisional, 11);

    // A week later, with no activity, the provisional owners without asks are gone; the rest stay.
    t.advance(Duration::days(7) + Duration::minutes(2));
    t.tick();
    let owners: OwnersResult = de(t.desk(DeskOp::Owners).unwrap().0);
    let names: Vec<&str> = owners.owners.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        ["approved-one", "compass-storefront", first.owner.as_str()]
    );
    assert_eq!(owners.more_provisional, 0);
    assert_eq!(t.ask_status(&asked.id), AskStatus::Open);
}

#[test]
fn non_ascii_asks_round_trip() {
    let t = T::new();
    let r = t.register_full(None, Some("sessión-ü-日本"), Some("C:\\DEV\\Résumé"));
    assert_eq!(r.owner, "session:sessión-ü-日本");
    let input = AskInput {
        priority: Priority::P1,
        title: "Résumé prüfen? 日本語".into(),
        body: "Ünïcödé body ".repeat(30),
        evidence: vec![
            Evidence {
                label: "Lebenslauf".into(),
                url: "Notes/Résumé.md".into(),
            },
            Evidence {
                label: "Byte 8 is inside é".into(),
                url: "1234567éx".into(),
            },
            Evidence {
                label: "Pfad".into(),
                url: "C:\\DEV\\Notes\\Résumé.md".into(),
            },
        ],
        options: vec![
            "Ja, bitte 🙂".into(),
            "Nein — später".into(),
            "Vielleicht…".into(),
        ],
        default: json!("Nein — später"),
        ..t.input("x")
    };
    let (asked, fx) = t.try_ask(&r.handle, input).unwrap();
    let toast = fx.iter().find_map(|e| match e {
        Effect::Toast {
            title,
            body,
            buttons,
            ..
        } => Some((title.clone(), body.clone(), buttons.clone())),
        _ => None,
    });
    let (title, body, buttons) = toast.expect("a P1 gets a toast");
    assert_eq!(title, "Résumé prüfen? 日本語");
    assert!(
        body.starts_with("From session:sessión-ü-日本. Ünïcödé"),
        "{body}"
    );
    assert_eq!(
        buttons.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
        ["Ja, bitte 🙂", "Nein — später", "Vielleicht…"]
    );
    let listed = t.list().inbox;
    assert_eq!(listed[0].input.evidence[1].url, "1234567éx");
    assert_eq!(listed[0].asking_session.as_deref(), Some("sessión-ü-日本"));
    let again = t.ask(
        &r.handle,
        AskInput {
            title: "RÉSUMÉ PRÜFEN 日本語".into(),
            ..listed[0].input.clone()
        },
    );
    assert_eq!(
        (again.placed.as_str(), again.id.as_str()),
        ("existing", asked.id.as_str()),
        "dedupe folds case"
    );
    let (answer, _) = t.answer(&asked.id, "Ja, bitte 🙂").unwrap();
    assert_eq!(answer.choice.as_deref(), Some("Ja, bitte 🙂"));
}

// ---- rule 4: validation through the service ----

#[test]
fn invalid_asks_are_refused_with_a_clear_error() {
    let t = T::new();
    let a = t.register("owner-a");
    let bad_link = AskInput {
        evidence: vec![Evidence {
            label: "x".into(),
            url: "search-ms:query=x".into(),
        }],
        ..t.input("Link")
    };
    assert!(t.try_ask(&a, bad_link).unwrap_err().contains("search-ms:"));
    assert!(
        t.try_ask(
            &a,
            AskInput {
                title: String::new(),
                ..t.input("")
            }
        )
        .unwrap_err()
        .contains("title")
    );
    let past = t.due_in(t.input("Late"), Duration::minutes(-1));
    assert!(t.try_ask(&a, past).unwrap_err().contains("future"));
    let far = t.due_in(t.input("Far"), Duration::days(31));
    assert!(t.try_ask(&a, far).unwrap_err().contains("30 days"));
    assert!(
        t.try_ask(
            &a,
            AskInput {
                default: json!("Maybe"),
                ..t.input("Default")
            }
        )
        .is_err()
    );
    assert_eq!(t.status().open, 0, "nothing was stored");
}

// ---- rules 5 and 6: shown and defaults ----

#[test]
fn defaults_apply_only_to_shown_reversible_asks() {
    let t = T::new();
    let a = t.register("owner-a");
    let asked = t.ask(
        &a,
        t.due_in(
            AskInput {
                default: json!("No"),
                ..t.input("Rename it?")
            },
            Duration::hours(1),
        ),
    );
    t.advance(Duration::hours(2));
    t.tick();
    assert_eq!(
        t.ask_status(&asked.id),
        AskStatus::Open,
        "never shown: no default, even overdue"
    );
    let r = t.answers(&a, &[&asked.id]);
    assert!(r.answers.is_empty(), "silence is never an answer");
    assert_eq!(r.waiting, vec![asked.id.clone()]);

    let shown = t.list();
    assert!(
        shown.inbox[0].shown_at.is_some(),
        "listing the Inbox marks it shown"
    );
    t.tick();
    assert_eq!(
        t.ask_status(&asked.id),
        AskStatus::Open,
        "overdue, but just shown: the grace runs first"
    );
    t.advance(Duration::milliseconds(rules::DEFAULT_GRACE_MS));
    t.tick();
    assert_eq!(t.ask_status(&asked.id), AskStatus::Defaulted);
    let got = t.answers(&a, &[&asked.id]).answers;
    let answer = got[0].answer.as_ref().unwrap();
    assert_eq!(answer.via, AnswerVia::Defaulted);
    assert_eq!(answer.choice.as_deref(), Some("No"));
}

#[test]
fn irreversible_asks_never_default() {
    let t = T::new();
    let a = t.register("owner-a");
    let input = AskInput {
        reversible: false,
        ..t.due_in(t.input("Delete the branch?"), Duration::hours(1))
    };
    let asked = t.ask(&a, input);
    t.list();
    t.advance(Duration::days(5));
    t.tick();
    assert_eq!(
        t.ask_status(&asked.id),
        AskStatus::Open,
        "stays open, overdue"
    );
    assert!(t.answers(&a, &[&asked.id]).answers.is_empty());
}

#[test]
fn digest_asks_default_only_after_the_digest_is_opened() {
    let t = T::new();
    t.set_budget(1);
    let (a, b) = (t.register("owner-a"), t.register("owner-b"));
    t.ask(&a, t.input("a1"));
    let held = t.ask(&b, t.due_in(t.input("b1"), Duration::hours(1)));
    assert_eq!(held.placed, "digest");
    t.list();
    t.advance(Duration::hours(2));
    t.tick();
    assert_eq!(
        t.ask_status(&held.id),
        AskStatus::Digest,
        "listing the Inbox doesn't show the digest"
    );
    let d = t.open_digest();
    assert!(d.digest[0].shown_at.is_some());
    t.tick();
    assert_eq!(
        t.ask_status(&held.id),
        AskStatus::Digest,
        "just shown: the grace runs first"
    );
    t.advance(Duration::hours(1));
    t.tick();
    assert_eq!(t.ask_status(&held.id), AskStatus::Defaulted);
}

#[test]
fn a_shown_ask_defaults_at_its_deadline_or_an_hour_after_shown_whichever_is_later() {
    let t = T::new();
    let a = t.register("owner-a");
    // Shown long before its deadline: it defaults at the deadline itself.
    let early = t.ask(&a, t.due_in(t.input("Early"), Duration::hours(3)));
    // Opening the Desk after a day away: overdue the moment it is first shown.
    let late = t.ask(&a, t.due_in(t.input("Late"), Duration::minutes(30)));
    t.advance(Duration::days(1));
    t.tick();
    assert_eq!(t.ask_status(&late.id), AskStatus::Open, "never shown");
    // `early` was due 21 h ago and is shown only now as well, so both wait out the grace.
    t.list();
    let shown_at = t.now();
    for step in [
        Duration::seconds(1),
        Duration::minutes(30),
        Duration::minutes(59),
    ] {
        t.set(shown_at + step);
        t.tick();
        assert_eq!(
            t.ask_status(&late.id),
            AskStatus::Open,
            "{step} after it was shown"
        );
        assert_eq!(
            t.ask_status(&early.id),
            AskStatus::Open,
            "{step} after it was shown"
        );
    }
    t.set(shown_at + Duration::hours(1));
    t.tick();
    assert_eq!(
        t.ask_status(&late.id),
        AskStatus::Defaulted,
        "an hour after it was shown"
    );
    assert_eq!(t.ask_status(&early.id), AskStatus::Defaulted);

    // Shown well ahead: no extra wait past the deadline.
    let ahead = t.ask(&a, t.due_in(t.input("Ahead"), Duration::hours(2)));
    t.list();
    t.advance(Duration::hours(2) - Duration::seconds(1));
    t.tick();
    assert_eq!(t.ask_status(&ahead.id), AskStatus::Open);
    t.advance(Duration::seconds(1));
    t.tick();
    assert_eq!(
        t.ask_status(&ahead.id),
        AskStatus::Defaulted,
        "at its deadline"
    );
}

// ---- rule 7: answers ----

#[test]
fn desk_answers_are_checked_against_the_ask() {
    let t = T::new();
    let a = t.register("owner-a");
    let choose = t.ask(&a, t.input("Pick"));
    assert!(
        t.answer(&choose.id, "Maybe")
            .unwrap_err()
            .contains("not one of the options")
    );
    let (ans, _) = t.answer(&choose.id, "No").unwrap();
    assert_eq!(
        (ans.via, ans.choice.as_deref()),
        (AnswerVia::Desk, Some("No"))
    );
    assert!(
        t.answer(&choose.id, "Yes")
            .unwrap_err()
            .contains("already answered"),
        "only open asks"
    );
    assert!(t.answer("ask_nope", "Yes").is_err());

    let text = AskInput {
        kind: AskKind::Text,
        options: vec![],
        default: json!("none"),
        ..t.input("Name it")
    };
    let text = t.ask(&a, text);
    let op = |values: Value| DeskOp::Answer {
        ask_id: text.id.clone(),
        choice: None,
        values: Some(values),
        note: None,
    };
    assert!(t.desk(op(json!(5))).is_err());
    let r: AnswerResult = de(t.desk(op(json!("Agentbolt"))).unwrap().0);
    assert_eq!(r.answer.choice.as_deref(), Some("Agentbolt"));

    let form = AskInput {
        kind: AskKind::Form,
        options: vec![],
        fields: vec![
            Field {
                id: "count".into(),
                label: "How many".into(),
                field_type: "number".into(),
                required: true,
                options: None,
            },
            Field {
                id: "size".into(),
                label: "Size".into(),
                field_type: "choice".into(),
                required: false,
                options: Some(vec!["S".into(), "L".into()]),
            },
        ],
        default: json!({"count": 1}),
        ..t.input("Order")
    };
    let form = t.ask(&a, form);
    let op = |values: Value| DeskOp::Answer {
        ask_id: form.id.clone(),
        choice: None,
        values: Some(values),
        note: Some("thanks".into()),
    };
    assert!(
        t.desk(op(json!({"size": "S"})))
            .unwrap_err()
            .contains("required")
    );
    assert!(t.desk(op(json!({"count": 2, "size": "XL"}))).is_err());
    assert!(t.desk(op(json!({"count": 2, "colour": "red"}))).is_err());
    let r: AnswerResult = de(t.desk(op(json!({"count": 2, "size": "L"}))).unwrap().0);
    assert_eq!(r.answer.values, Some(json!({"count": 2, "size": "L"})));
    assert_eq!(r.answer.note.as_deref(), Some("thanks"));
}

#[test]
fn reading_answers_never_consumes_them() {
    let t = T::new();
    let a = t.register("owner-a");
    let b = t.register("owner-b");
    let mine = t.ask(&a, t.input("Mine"));
    let theirs = t.ask(&b, t.input("Theirs"));
    let before = t.answers(&a, &[]);
    assert!(before.answers.is_empty());
    assert_eq!(before.waiting, vec![mine.id.clone()]);

    t.answer(&mine.id, "Yes").unwrap();
    for _ in 0..3 {
        let r = t.answers(&a, &[&mine.id]);
        assert_eq!(r.answers.len(), 1, "still there after reading");
        assert_eq!(
            r.answers[0].answer.as_ref().unwrap().choice.as_deref(),
            Some("Yes")
        );
        assert_eq!(t.answers(&a, &[]).answers.len(), 1);
    }
    let r = t.answers(&a, &[&theirs.id, "ask_missing", &mine.id]);
    assert_eq!(
        r.unknown,
        vec![theirs.id.clone(), "ask_missing".to_string()]
    );
    assert_eq!(r.answers.len(), 1);
    let again = t.register_full(Some("owner-a"), Some("a-next-run"), None);
    assert_eq!(
        again.answers.len(),
        1,
        "the next run of the owner gets it from register"
    );
    assert!(again.open_asks.is_empty());

    t.cancel(&b, &theirs.id).unwrap();
    assert_eq!(
        t.answers(&b, &[&theirs.id]).cancelled,
        vec![theirs.id.clone()]
    );
}

// ---- rule 8: toast tokens ----

fn urgent(t: &T, title: &str) -> AskInput {
    AskInput {
        priority: Priority::P1,
        options: vec!["Red".into(), "Green".into(), "Blue".into()],
        default: json!("Red"),
        ..t.due_in(t.input(title), Duration::days(20))
    }
}

#[test]
fn a_toast_button_answers_its_own_option_once() {
    let t = T::new();
    let a = t.register("owner-a");
    let (asked, fx) = t.try_ask(&a, urgent(&t, "Colour?")).unwrap();
    let toasts = toasts(&fx);
    assert_eq!(toasts.len(), 1);
    let (ask_id, buttons) = &toasts[0];
    assert_eq!(ask_id, &asked.id);
    let labels: Vec<&str> = buttons.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(labels, ["Red", "Green", "Blue"]);
    assert!(
        buttons
            .iter()
            .all(|(_, tok)| tok.chars().all(|c| c.is_ascii_hexdigit() || c == '.'))
    );

    let answer = t.token(&buttons[1].1).unwrap();
    assert_eq!(
        (answer.via, answer.choice.as_deref()),
        (AnswerVia::Toast, Some("Green"))
    );
    assert!(t.token(&buttons[1].1).unwrap_err().contains("already used"));
    assert!(
        t.token(&buttons[0].1).is_err(),
        "the ask is answered: every other button is dead"
    );
}

#[test]
fn a_token_can_not_answer_another_option() {
    let t = T::new();
    let a = t.register("owner-a");
    let (asked, fx) = t.try_ask(&a, urgent(&t, "Colour?")).unwrap();
    let token = &toasts(&fx)[0].1[0].1;
    let (toast_id, rest) = token.split_once('.').unwrap();
    let mac = rest.split_once('.').unwrap().1;
    for idx in ["1", "2", "4"] {
        let forged = format!("{toast_id}.{idx}.{mac}");
        assert_eq!(t.token(&forged).unwrap_err(), "invalid answer token");
    }
    assert_eq!(t.ask_status(&asked.id), AskStatus::Open);
    assert_eq!(t.token(token).unwrap().choice.as_deref(), Some("Red"));
}

#[test]
fn tokens_expire_after_seven_days() {
    let t = T::new();
    let a = t.register("owner-a");
    let (asked, fx) = t.try_ask(&a, urgent(&t, "Colour?")).unwrap();
    let token = toasts(&fx)[0].1[2].1.clone();
    t.advance(Duration::days(7) + Duration::seconds(1));
    assert!(t.token(&token).unwrap_err().contains("expired"));
    assert_eq!(t.ask_status(&asked.id), AskStatus::Open);
}

#[test]
fn tokens_die_when_the_ask_is_answered_or_cancelled_another_way() {
    let t = T::new();
    let a = t.register("owner-a");
    let (asked, fx) = t.try_ask(&a, urgent(&t, "First?")).unwrap();
    t.answer(&asked.id, "Blue").unwrap();
    assert!(
        t.token(&toasts(&fx)[0].1[0].1)
            .unwrap_err()
            .contains("already answered")
    );

    let (asked, fx) = t.try_ask(&a, urgent(&t, "Second?")).unwrap();
    t.cancel(&a, &asked.id).unwrap();
    assert!(
        t.token(&toasts(&fx)[0].1[0].1)
            .unwrap_err()
            .contains("cancelled")
    );
    assert!(t.token("not-a-token").is_err());
}

#[test]
fn which_asks_get_toasts_and_buttons() {
    let t = T::new();
    let a = t.register("owner-a");
    let b = t.register("owner-b");
    let c = t.register("owner-c");
    let (_, fx) = t.try_ask(&a, t.input("Quiet P2")).unwrap();
    assert!(toasts(&fx).is_empty(), "P2 not due soon: no toast");
    let (_, fx) = t
        .try_ask(&a, t.due_in(t.input("P2 due soon"), Duration::minutes(90)))
        .unwrap();
    assert_eq!(toasts(&fx)[0].1.len(), 2, "due within 2 hours");
    let (_, fx) = t
        .try_ask(
            &a,
            AskInput {
                reversible: false,
                ..urgent(&t, "Irreversible")
            },
        )
        .unwrap();
    assert!(
        toasts(&fx)[0].1.is_empty(),
        "irreversible: a toast without buttons"
    );
    let text = AskInput {
        kind: AskKind::Text,
        options: vec![],
        default: json!(""),
        ..urgent(&t, "Text")
    };
    let (text_ask, fx) = t.try_ask(&b, text).unwrap();
    assert!(toasts(&fx)[0].1.is_empty(), "text asks open the Desk");
    let six = AskInput {
        kind: AskKind::Form,
        options: vec![],
        fields: vec![Field {
            id: "x".into(),
            label: "X".into(),
            field_type: "boolean".into(),
            required: false,
            options: None,
        }],
        default: json!({}),
        ..urgent(&t, "Form")
    };
    let (_, fx) = t.try_ask(&b, six).unwrap();
    assert!(toasts(&fx)[0].1.is_empty());

    t.desk(DeskOp::SetSettings {
        settings: Settings {
            daily_budget: 10,
            toasts_off: true,
            ntfy_topic_url: None,
        },
    })
    .unwrap();
    let (off, fx) = t.try_ask(&c, urgent(&t, "Toasts off")).unwrap();
    assert!(toasts(&fx).is_empty(), "toasts_off silences them");
    assert_eq!(
        t.svc.mint_toast(&off.id).map(|b| b.len()),
        Some(3),
        "mint_toast still mints for the daemon"
    );
    assert_eq!(t.svc.mint_toast(&text_ask.id), None);
    assert_eq!(t.svc.mint_toast("ask_missing"), None);
}

// ---- rules 9 to 13 ----

#[test]
fn the_event_chain_detects_a_tampered_row() {
    let t = T::new();
    let a = t.register("owner-a");
    let asked = t.ask(&a, t.input("Audit me"));
    t.answer(&asked.id, "Yes").unwrap();
    assert!(t.status().chain_ok);
    let n = t.svc.verify_chain().unwrap();
    assert!(n >= 4, "secret, owner, agent, ask and answer events ({n})");
    {
        let conn = t.svc.conn_for_tests();
        assert!(
            conn.execute("UPDATE events SET kind = 'x' WHERE seq = 1", [])
                .is_err(),
            "append-only"
        );
        conn.execute_batch("DROP TRIGGER events_no_update;")
            .unwrap();
        conn.execute(
            "UPDATE events SET data = replace(data, '\"Yes\"', '\"No\"') WHERE kind = 'ask.answered'",
            [],
        )
        .unwrap();
    }
    assert!(!t.status().chain_ok);
    assert!(
        t.svc
            .verify_chain()
            .unwrap_err()
            .contains("changed after it was written")
    );
}

#[test]
fn status_checks_only_new_rows_between_full_walks_and_still_catches_a_bad_one() {
    let t = T::new();
    let a = t.register("owner-a");
    t.ask(&a, t.input("First"));
    assert!(
        t.status().chain_ok,
        "the first Status walks the whole chain"
    );
    t.ask(&a, t.input("Second"));
    assert!(
        t.status().chain_ok,
        "new, well-formed rows verify incrementally"
    );
    {
        let conn = t.svc.conn_for_tests();
        let next: i64 = conn
            .query_row("SELECT MAX(seq) + 1 FROM events", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO events (seq, at, kind, subject, data, prev_hash, hash) VALUES (?1, 'x', 'forged', 's', '{}', 'bad', 'bad')",
            [next],
        )
        .unwrap();
    }
    assert!(
        !t.status().chain_ok,
        "a forged row after the mark is caught without a full walk"
    );
}

#[test]
fn relay_calls_are_refused_to_every_other_owner() {
    let t = T::new();
    let a = t.register("owner-a");
    let asked = t.ask(&a, t.input("Mine"));
    t.answer(&asked.id, "Yes").unwrap();
    let e = t.mcp(McpOp::RelayList { handle: a.clone() }).unwrap_err();
    assert!(e.contains("relay owner"), "{e}");
    let mark = McpOp::RelayMark {
        handle: a,
        ask_id: asked.id,
        to: "owner-a-session".into(),
        ok: true,
        detail: None,
    };
    assert!(t.mcp(mark).unwrap_err().contains("relay owner"));
}

#[test]
fn the_relay_lists_answers_until_any_delivery_succeeds() {
    let t = T::new();
    t.set_owner("compass-reception", "config", false, true);
    let relay = t.register("compass-reception");
    let a = t.register("compass-storefront");
    let b = t.register("owner-b");
    let one = t.ask(&a, t.input("One"));
    let two = t.ask(&b, t.input("Two"));
    let open = t.ask(&b, t.input("Still open"));
    t.answer(&one.id, "Yes").unwrap();
    t.advance(Duration::seconds(1));
    t.answer(&two.id, "No").unwrap();
    let mine = t.ask(&relay, t.input("Relay's own"));
    t.answer(&mine.id, "Yes").unwrap();

    let list = |t: &T| -> Vec<String> {
        let r: RelayListResult = de(t
            .mcp(McpOp::RelayList {
                handle: relay.clone(),
            })
            .unwrap()
            .0);
        r.answers.into_iter().map(|a| a.id).collect()
    };
    assert_eq!(
        list(&t),
        vec![two.id.clone(), one.id.clone()],
        "answered asks of other owners only, newest first"
    );
    assert!(!list(&t).contains(&open.id));

    let mark = |ok: bool, to: &str| McpOp::RelayMark {
        handle: relay.clone(),
        ask_id: one.id.clone(),
        to: to.into(),
        ok,
        detail: Some("NOTES.md line".into()),
    };
    t.mcp(mark(false, "compass-storefront-session")).unwrap();
    assert!(
        list(&t).contains(&one.id),
        "a failed delivery doesn't count"
    );
    // The relay could only write it to NOTES.md: that is a delivery too, so it leaves the list.
    let (v, _) = t.mcp(mark(true, "notes")).unwrap();
    assert_eq!(de::<AnswerResult>(v).answer.deliveries.len(), 2);
    assert_eq!(list(&t), vec![two.id.clone()]);
    assert_eq!(
        t.answers(&a, &[]).answers.len(),
        1,
        "a notes delivery takes it off the relay's list, not off the asker's own: that agent may not read NOTES.md"
    );
    assert_eq!(
        t.answers(&a, &[&one.id]).answers.len(),
        1,
        "and readable by id"
    );

    // The daemon's own record (codex queue) counts the same way.
    t.svc
        .record_delivery(&two.id, "owner-b-session", true, Some("queued"))
        .unwrap();
    assert!(list(&t).is_empty());
    assert!(
        t.svc
            .record_delivery(&open.id, "owner-b-session", true, None)
            .unwrap_err()
            .contains("no answer")
    );
    assert!(
        t.svc
            .record_delivery("ask_missing", "x", true, None)
            .is_err()
    );
}

#[test]
fn the_relay_list_keeps_a_14_day_window_and_the_newest_100() {
    let t = T::new();
    t.set_budget(50);
    t.set_owner("compass-reception", "config", false, true);
    let relay = t.register("compass-reception");
    let old_owner = t.register("old-owner");
    let old = t.ask(&old_owner, t.input("Old"));
    t.answer(&old.id, "Yes").unwrap();
    t.advance(Duration::days(15));
    // 102 fresh answers from 34 owners (3 each, under the cap), one second apart.
    let mut fresh = Vec::new();
    for o in 0..34 {
        let h = t.register(&format!("owner-{o:02}"));
        for k in 0..3 {
            let id = t
                .ask(
                    &h,
                    t.due_in(t.input(&format!("q{o}-{k}")), Duration::days(20)),
                )
                .id;
            t.advance(Duration::seconds(1));
            t.answer(&id, "Yes").unwrap();
            fresh.push(id);
        }
    }
    let r: RelayListResult = de(t
        .mcp(McpOp::RelayList {
            handle: relay.clone(),
        })
        .unwrap()
        .0);
    let ids: Vec<&str> = r.answers.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids.len(), 100);
    assert_eq!(ids[0], fresh[101], "newest first");
    assert_eq!(
        ids[99], fresh[2],
        "the two oldest fresh ones wait for the next call"
    );
    assert!(
        !ids.contains(&old.id.as_str()),
        "answered 15 days ago: outside the window"
    );
}

#[test]
fn cancel_takes_only_the_owners_own_open_asks() {
    let t = T::new();
    let a = t.register("owner-a");
    let b = t.register("owner-b");
    let asked = t.ask(&a, t.input("Mine"));
    assert!(t.cancel(&b, &asked.id).unwrap_err().contains("no ask"));
    assert_eq!(t.ask_status(&asked.id), AskStatus::Open);
    t.answer(&asked.id, "Yes").unwrap();
    assert!(
        t.cancel(&a, &asked.id)
            .unwrap_err()
            .contains("already answered")
    );
    let other = t.ask(&a, t.input("Other"));
    t.cancel(&a, &other.id).unwrap();
    assert_eq!(t.ask_status(&other.id), AskStatus::Cancelled);
    assert!(
        t.cancel(&a, &other.id)
            .unwrap_err()
            .contains("already cancelled")
    );
    assert!(
        t.answer(&other.id, "Yes").is_err(),
        "a cancelled ask can't be answered"
    );
}

#[test]
fn every_handle_op_needs_a_known_handle() {
    let t = T::new();
    let ops = vec![
        McpOp::Heartbeat {
            handle: "hnope".into(),
            doing: None,
        },
        McpOp::Ask {
            handle: "hnope".into(),
            ask: t.input("x"),
        },
        McpOp::Answers {
            handle: "hnope".into(),
            ids: vec![],
            wait_ms: 0,
        },
        McpOp::Cancel {
            handle: "hnope".into(),
            id: "ask_x".into(),
        },
        McpOp::RelayList {
            handle: "hnope".into(),
        },
        McpOp::RelayMark {
            handle: "hnope".into(),
            ask_id: "x".into(),
            to: "y".into(),
            ok: true,
            detail: None,
        },
    ];
    for op in ops {
        assert_eq!(t.mcp(op).unwrap_err(), UNKNOWN_HANDLE);
    }
    assert!(t.mcp(McpOp::Status).is_ok(), "status needs no handle");
}

#[test]
fn handles_survive_disconnects_and_expire_after_a_week_of_silence() {
    let t = T::new();
    let a = t.register("owner-a");
    t.svc.disconnected(1);
    t.advance(Duration::minutes(5));
    let (v, _) = t
        .mcp(McpOp::Heartbeat {
            handle: a.clone(),
            doing: Some("building\nM1".into()),
        })
        .unwrap();
    let agent: Agent = de(v);
    assert_eq!(agent.doing.as_deref(), Some("building M1"));
    assert_eq!(agent.last_seen, store::fmt_ms(store::to_ms(t.now())));
    assert_eq!(agent.machine, "testbox");
    let agents: AgentsResult = de(t.desk(DeskOp::Agents).unwrap().0);
    assert_eq!(agents.agents.len(), 1);
    assert_eq!(t.status().agents, 1);

    t.advance(Duration::hours(5));
    assert_eq!(t.status().agents, 0, "not alive after 4 hours of silence");
    t.advance(Duration::days(7));
    t.tick();
    let e = t
        .mcp(McpOp::Heartbeat {
            handle: a,
            doing: None,
        })
        .unwrap_err();
    assert_eq!(e, UNKNOWN_HANDLE);
}

#[test]
fn codex_interactive_owners_get_their_answers_queued() {
    let t = T::new();
    t.set_owner("codex-nestbolt", "approved", true, false);
    let h = t
        .register_full(Some("codex-nestbolt"), Some("thread-019"), None)
        .handle;
    let asked = t.ask(&h, t.input("Use the faster solver?"));
    let (_, fx) = t.answer(&asked.id, "Yes").unwrap();
    let queued: Vec<_> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::CodexQueue {
                ask_id,
                thread,
                text,
            } => Some((ask_id.clone(), thread.clone(), text.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(queued.len(), 1);
    assert_eq!(
        (queued[0].0.as_str(), queued[0].1.as_str()),
        (asked.id.as_str(), "thread-019")
    );
    assert!(queued[0].2.contains("\"Yes\"") && queued[0].2.contains("Artem answered"));

    // A default is queued too, saying it carries no authority.
    let later = t.ask(&h, t.due_in(t.input("Second"), Duration::hours(1)));
    t.list();
    t.advance(Duration::hours(2));
    let fx = t.tick();
    let text = fx.iter().find_map(|e| match e {
        Effect::CodexQueue { ask_id, text, .. } if *ask_id == later.id => Some(text.clone()),
        _ => None,
    });
    assert!(text.unwrap().contains("no authority"));

    let plain = t.register("codex-exec");
    let asked = t.ask(&plain, t.input("Exec thread"));
    let (_, fx) = t.answer(&asked.id, "Yes").unwrap();
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::CodexQueue { .. })),
        "not marked interactive"
    );
}

#[test]
fn inbox_changed_is_sent_only_when_the_counts_change() {
    let t = T::new();
    assert_eq!(
        inbox_changes(&t.tick()),
        vec![(0, 0)],
        "the first tick reports the starting count"
    );
    assert!(t.tick().is_empty());
    let a = t.register("owner-a");
    let (asked, fx) = t.try_ask(&a, t.input("Count me")).unwrap();
    let next = fx.iter().find_map(|e| match e {
        Effect::InboxChanged {
            open: 1,
            digest: 0,
            next_deadline,
        } => next_deadline.clone(),
        _ => None,
    });
    assert_eq!(
        next,
        Some(store::fmt_ms(store::to_ms(t.now() + Duration::days(1))))
    );
    let (_, fx) = t
        .mcp(McpOp::Answers {
            handle: a.clone(),
            ids: vec![],
            wait_ms: 30_000,
        })
        .unwrap();
    assert!(fx.is_empty(), "reading changes nothing");
    assert!(t.desk(DeskOp::List).unwrap().1.is_empty());
    let (_, fx) = t.answer(&asked.id, "Yes").unwrap();
    assert_eq!(inbox_changes(&fx), vec![(0, 0)]);
}

#[test]
fn settings_round_trip_and_are_checked() {
    let t = T::new();
    let s: Settings = de(t.desk(DeskOp::GetSettings).unwrap().0);
    assert_eq!(
        s,
        Settings {
            ntfy_topic_url: None,
            daily_budget: 5,
            toasts_off: false
        }
    );
    let set = |s: Settings| t.desk(DeskOp::SetSettings { settings: s });
    assert!(
        set(Settings {
            daily_budget: 0,
            ..s.clone()
        })
        .unwrap_err()
        .contains("daily_budget")
    );
    assert!(
        set(Settings {
            daily_budget: 51,
            ..s.clone()
        })
        .is_err()
    );
    assert!(
        set(Settings {
            ntfy_topic_url: Some("http://ntfy.sh/x".into()),
            ..s.clone()
        })
        .is_err()
    );
    let good = Settings {
        ntfy_topic_url: Some("https://ntfy.sh/secret-topic".into()),
        daily_budget: 3,
        toasts_off: true,
    };
    set(good.clone()).unwrap();
    assert_eq!(de::<Settings>(t.desk(DeskOp::GetSettings).unwrap().0), good);
    // An upper-case scheme is accepted and stored lower-case, which is what the daemon's push checks.
    set(Settings {
        ntfy_topic_url: Some("HTTPS://ntfy.sh/Secret-Topic".into()),
        ..good.clone()
    })
    .unwrap();
    let stored = de::<Settings>(t.desk(DeskOp::GetSettings).unwrap().0).ntfy_topic_url;
    assert_eq!(stored.as_deref(), Some("https://ntfy.sh/Secret-Topic"));
    assert!(
        set(Settings {
            ntfy_topic_url: Some("https://".into()),
            ..good.clone()
        })
        .is_err()
    );
    assert!(
        set(Settings {
            ntfy_topic_url: Some("1234567é/topic".into()),
            ..good.clone()
        })
        .is_err()
    );
    set(Settings {
        ntfy_topic_url: Some("  ".into()),
        ..good
    })
    .unwrap();
    assert_eq!(
        de::<Settings>(t.desk(DeskOp::GetSettings).unwrap().0).ntfy_topic_url,
        None
    );
}

#[test]
fn the_database_file_is_exclusive_and_keeps_its_secret() {
    let dir = std::env::temp_dir().join(format!(
        "agentbolt-core-test-{}",
        crate::tokens::random_hex(6)
    ));
    let path = dir.join("nested").join("agentbolt.db");
    let token = {
        let t = T::with(StoreService::open(&path).unwrap());
        let e = StoreService::open(&path)
            .err()
            .expect("a second open must fail");
        assert!(e.contains("another"), "{e}");
        let a = t.register("owner-a");
        let (_, fx) = t.try_ask(&a, urgent(&t, "Survive a restart?")).unwrap();
        toasts(&fx)[0].1[1].1.clone()
    };
    {
        // Reopened after the daemon stopped: the same secret, so the toast's buttons still work.
        let t = T::with(StoreService::open(&path).unwrap());
        assert_eq!(t.token(&token).unwrap().choice.as_deref(), Some("Green"));
        assert!(t.svc.verify_chain().is_ok());
        let mode: String = t
            .svc
            .conn_for_tests()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_service_is_send_and_sync() {
    fn assert_send_sync<S: Service + Send + Sync + 'static>() {}
    assert_send_sync::<StoreService>();
    let svc: Arc<dyn Service> = Arc::new(StoreService::open_in_memory().unwrap());
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let svc = svc.clone();
            std::thread::spawn(move || {
                let op = McpOp::Register {
                    owner: Some(format!("owner-{i}")),
                    client: "t".into(),
                    session_ref: None,
                    cwd: None,
                    doing: None,
                    provisional_key: None,
                };
                svc.mcp(&ClientInfo::default(), op).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let s: StatusResult = de(svc.mcp(&ClientInfo::default(), McpOp::Status).unwrap().0);
    assert_eq!(s.agents, 4);
    assert!(s.chain_ok);
}
