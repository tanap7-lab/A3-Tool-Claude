//! Section 17 acceptance criteria for permissions, status and records, against real PostgreSQL.

mod common;

use a3_core::model::*;
use a3_core::repo::*;
use a3_core::store::{AttachmentStore, PgAttachmentStore};
use common::*;

#[tokio::test]
async fn status_matches_the_table_for_every_step_combination() {
    let t = team().await;
    let letters = ["A", "B", "C", "D"];
    // All 16 combinations of open and closed. Later steps can stay closed while an earlier one reopens.
    for mask in 0..16u32 {
        let d = new_sheet(&t.owner, t.project.id).await;
        let id = d.sheet.id;
        for l in letters {
            fill_step(&t.owner, id, l, "Text").await;
        }
        for l in letters {
            t.owner.close_step(step_of(&d, l).id).await.unwrap();
        }
        for (i, l) in letters.iter().enumerate() {
            if mask & (1 << i) != 0 {
                t.owner.reopen_step(step_of(&d, l).id, "Correction").await.unwrap();
            }
        }
        let got = t.owner.sheet_detail(id).await.unwrap().sheet;
        let earliest_open = letters.iter().enumerate().find(|(i, _)| mask & (1 << i) != 0).map(|(_, l)| l.to_string());
        assert_eq!(got.current_step, earliest_open, "mask {mask}");
        if mask == 0 {
            assert_eq!(got.status, "Closed", "mask {mask}");
            assert!(got.closed_at.is_some());
        } else {
            assert_eq!(got.status, "Open", "mask {mask}");
            assert!(got.closed_at.is_none());
            assert!(got.reopened, "a reopened step raises the flag");
        }
    }
}

#[tokio::test]
async fn four_steps_exist_and_start_open() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    assert_eq!(d.steps.len(), 4);
    assert!(d.steps.iter().all(|s| s.state == "open"));
    assert_eq!(d.sheet.status, "Open");
    assert_eq!(d.sheet.current_step.as_deref(), Some("A"));
    let re = regex::Regex::new(r"^A3-\d{4}-\d{4,}$").unwrap();
    assert!(re.is_match(&d.sheet.number), "{}", d.sheet.number);
}

#[tokio::test]
async fn steps_close_in_order_with_text_by_the_owner_only() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let id = d.sheet.id;
    fill_step(&t.owner, id, "A", "Problem").await;
    fill_step(&t.owner, id, "B", "Cause").await;

    let e = t.owner.close_step(step_of(&d, "B").id).await.unwrap_err();
    assert_eq!(e.code, "A3_STEP_ORDER");
    let e = t.member.close_step(step_of(&d, "A").id).await.unwrap_err();
    assert_eq!(e.code, "A3_NOT_OWNER");
    t.owner.close_step(step_of(&d, "A").id).await.unwrap();
    let e = t.owner.close_step(step_of(&d, "C").id).await.unwrap_err();
    assert_eq!(e.code, "A3_STEP_ORDER");
    t.owner.close_step(step_of(&d, "B").id).await.unwrap();
    let e = t.owner.close_step(step_of(&d, "C").id).await.unwrap_err();
    assert_eq!(e.code, "A3_STEP_EMPTY");
}

#[tokio::test]
async fn closed_steps_are_read_only_and_reopening_is_logged() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let id = d.sheet.id;
    for l in ["A", "B"] {
        fill_step(&t.owner, id, l, "Text").await;
    }
    t.owner.close_step(step_of(&d, "A").id).await.unwrap();
    t.owner.close_step(step_of(&d, "B").id).await.unwrap();

    let now = t.owner.sheet_detail(id).await.unwrap();
    let a = step_of(&now, "A");
    let e = t.owner.save_step(a.id, a.version, &doc("New")).await.unwrap_err();
    assert_eq!(e.code, "A3_STEP_CLOSED");

    let e = t.owner.reopen_step(a.id, "Because I said so").await.unwrap_err();
    assert_eq!(e.code, "A3_BAD_REASON");
    t.owner.reopen_step(a.id, "New evidence").await.unwrap();

    // FR-ST-08: later steps stay closed and the A3 returns to the earliest open step.
    let after = t.owner.sheet_detail(id).await.unwrap();
    assert_eq!(step_of(&after, "B").state, "closed");
    assert_eq!(after.sheet.current_step.as_deref(), Some("A"));

    let hist = t.owner.history(id).await.unwrap();
    assert!(hist.iter().any(|h| h.action == "step_reopened" && h.reason.as_deref() == Some("New evidence")));
    assert!(hist.iter().any(|h| h.action == "step_closed"));
    let snaps = t.owner.snapshots(id).await.unwrap();
    assert!(snaps.iter().any(|s| s.reason == "close") && snaps.iter().any(|s| s.reason == "reopen"));
}

#[tokio::test]
async fn an_outsider_cannot_change_an_a3_even_with_direct_sql() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let id = d.sheet.id;
    let step = step_of(&d, "A").id;
    let pool = t.outsider.pool();

    // Every signed-in user can read every A3 (D3).
    assert_eq!(t.outsider.sheet_detail(id).await.unwrap().sheet.number, d.sheet.number);

    // Row-level security hides the row from writes: zero rows change.
    let r = sqlx::query("UPDATE a3_step SET content = '{}', plain_text = 'hacked' WHERE id = $1").bind(step).execute(pool).await.unwrap();
    assert_eq!(r.rows_affected(), 0);
    let r = sqlx::query("UPDATE a3_sheet SET title = 'hacked' WHERE id = $1").bind(id).execute(pool).await.unwrap();
    assert_eq!(r.rows_affected(), 0);

    // Column privileges and missing grants refuse the rest outright.
    assert!(sqlx::query("UPDATE a3_step SET state = 'closed' WHERE id = $1").bind(step).execute(pool).await.is_err());
    assert!(sqlx::query("DELETE FROM a3_sheet WHERE id = $1").bind(id).execute(pool).await.is_err());
    assert!(sqlx::query("DELETE FROM a3_step WHERE id = $1").bind(step).execute(pool).await.is_err());
    assert!(sqlx::query("UPDATE app_user SET is_admin = true").execute(pool).await.is_err());
    assert!(sqlx::query("DROP TABLE a3_step").execute(pool).await.is_err());
    assert!(sqlx::query("DELETE FROM audit_event").execute(pool).await.is_err());
    assert!(sqlx::query("SELECT touch_sheet($1)").bind(id).execute(pool).await.is_err());

    assert!(sqlx::query("SELECT close_step($1)").bind(step).execute(pool).await.is_err());
    let e = t.outsider.save_step(step, 1, &doc("x")).await.unwrap_err();
    assert_eq!(e.code, "NO_PERMISSION");
    let e = t
        .outsider
        .create_sheet(&NewSheet { title: "x".into(), project_id: t.project.id, ..Default::default() })
        .await
        .unwrap_err();
    assert_eq!(e.code, "NO_PERMISSION");

    let after = t.owner.sheet_detail(id).await.unwrap();
    assert_eq!(after.sheet.title, "Line 4 scrap");
    assert_eq!(step_of(&after, "A").plain_text, "");
}

#[tokio::test]
async fn a_member_cannot_set_step_state_directly() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let r = sqlx::query("UPDATE a3_step SET state = 'closed', closed_at = now() WHERE id = $1").bind(step_of(&d, "A").id).execute(t.member.pool()).await;
    assert!(r.is_err(), "state changes only through close_step and reopen_step");
}

#[tokio::test]
async fn stale_saves_are_refused_and_versions_climb() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let a = step_of(&d, "A");
    let saved = t.owner.save_step(a.id, a.version, &doc("First")).await.unwrap();
    assert_eq!(saved.version, a.version + 1);
    assert_eq!(saved.plain_text, "First");
    let e = t.member.save_step(a.id, a.version, &doc("Stale")).await.unwrap_err();
    assert_eq!(e.code, "STALE");
    assert_eq!(t.owner.sheet_detail(d.sheet.id).await.unwrap().steps[0].plain_text, "First");
}

#[tokio::test]
async fn one_editor_per_step_and_the_lock_expires() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let a = step_of(&d, "A");
    let held = t.owner.acquire_lock(a.id).await.unwrap();
    assert_eq!(held.locked_by, Some(t.owner.me().id));
    let seen = t.member.acquire_lock(a.id).await.unwrap();
    assert_eq!(seen.locked_by, Some(t.owner.me().id), "the member sees who holds it");
    assert!(seen.locked_by_name.is_some());
    let e = t.member.save_step(a.id, held.version, &doc("Mine")).await.unwrap_err();
    assert_eq!(e.code, "A3_STEP_LOCKED");

    // Two idle minutes later the lock has lapsed.
    let owner = owner_pool().await;
    sqlx::query("UPDATE a3_step SET locked_at = now() - interval '3 minutes' WHERE id = $1").bind(a.id).execute(&owner).await.unwrap();
    let taken = t.member.acquire_lock(a.id).await.unwrap();
    assert_eq!(taken.locked_by, Some(t.member.me().id));
    t.member.save_step(a.id, held.version, &doc("Mine")).await.unwrap();
}

#[tokio::test]
async fn health_flags_are_computed() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let id = d.sheet.id;
    let mut patch = SheetPatch {
        title: d.sheet.title.clone(),
        project_id: d.sheet.project_id,
        owner_id: d.sheet.owner_id,
        team_member_ids: vec![],
        problem_date: None,
        target_close_date: chrono::NaiveDate::from_ymd_opt(2020, 1, 1),
        category: None,
        severity: None,
        source: None,
        area: None,
        product: None,
    };
    t.owner.update_sheet(id, &patch).await.unwrap();
    let s = t.owner.sheet_detail(id).await.unwrap().sheet;
    assert!(s.overdue);
    assert!(!s.stalled);
    patch.target_close_date = None;
    t.owner.update_sheet(id, &patch).await.unwrap();

    // Inside one transaction the table lock hides the disabled trigger from every other session,
    // so tests running alongside never see a3_sheet without its rules.
    let owner = owner_pool().await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("ALTER TABLE a3_sheet DISABLE TRIGGER a3_sheet_before_update").execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE a3_sheet SET updated_at = now() - interval '15 days' WHERE id = $1").bind(id).execute(&mut *tx).await.unwrap();
    sqlx::query("ALTER TABLE a3_sheet ENABLE TRIGGER a3_sheet_before_update").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let s = t.owner.sheet_detail(id).await.unwrap().sheet;
    assert!(s.stalled, "no change for 14 days");
}

#[tokio::test]
async fn withdrawn_a3s_leave_counts_but_stay_retrievable() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let id = d.sheet.id;
    let e = t.member.withdraw_sheet(id, "Duplicate").await.unwrap_err();
    assert_eq!(e.code, "A3_NOT_OWNER");
    t.owner.withdraw_sheet(id, "Duplicate").await.unwrap();
    let f = SheetFilter { project_id: Some(t.project.id), ..Default::default() };
    assert_eq!(t.owner.list_sheets(&f).await.unwrap().total, 0);
    let f = SheetFilter { project_id: Some(t.project.id), quick: Some("withdrawn".into()), ..Default::default() };
    assert_eq!(t.owner.list_sheets(&f).await.unwrap().total, 1);
    assert_eq!(t.owner.sheet_detail(id).await.unwrap().sheet.withdrawn_reason.as_deref(), Some("Duplicate"));
}

#[tokio::test]
async fn list_filters_search_and_sort() {
    let t = team().await;
    let code = t.project.id;
    let a = new_sheet(&t.owner, code).await;
    let b = new_sheet(&t.member, code).await;
    fill_step(&t.owner, a.sheet.id, "A", "The zygomorphic bracket cracked").await;

    let f = |s: SheetFilter| SheetFilter { project_id: Some(code), ..s };
    assert_eq!(t.owner.list_sheets(&f(Default::default())).await.unwrap().total, 2);
    let mine = t.owner.list_sheets(&f(SheetFilter { mine: true, ..Default::default() })).await.unwrap();
    assert_eq!(mine.total, 1);
    assert_eq!(mine.rows[0].id, a.sheet.id);
    let found = t.owner.list_sheets(&f(SheetFilter { search: Some("zygomorphic".into()), ..Default::default() })).await.unwrap();
    assert_eq!(found.total, 1);
    assert_eq!(found.rows[0].id, a.sheet.id);
    let by_number = t.owner.list_sheets(&SheetFilter { search: Some(b.sheet.number.clone()), ..Default::default() }).await.unwrap();
    assert_eq!(by_number.total, 1);
    for col in ["number", "title", "project", "owner", "step", "status", "health", "severity", "target", "updated", "'; DROP TABLE a3_step; --"] {
        let r = t.owner.list_sheets(&f(SheetFilter { sort: Some(col.into()), descending: Some(false), ..Default::default() })).await;
        assert!(r.is_ok(), "sort {col}");
    }
    // Attachment text is searchable too.
    add_text_file(t.owner.clone(), b.sheet.id, "note.txt", "The quixotic gasket failed").await;
    let found = t.owner.list_sheets(&f(SheetFilter { search: Some("quixotic".into()), ..Default::default() })).await.unwrap();
    assert_eq!(found.rows[0].id, b.sheet.id);
}

#[tokio::test]
async fn attachments_version_dedupe_and_delete_rules() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let id = d.sheet.id;
    let store = PgAttachmentStore::new(t.owner.pool().clone());

    let one = store.put_bytes(b"version one").await.unwrap();
    let first = t.owner.add_attachment(id, "report.txt", &one.hash, "txt").await.unwrap();
    assert!(!first.duplicate);
    assert_eq!(first.attachment.version, 1);

    // FR-ATT-04: identical content is not stored twice, and new content makes a new version.
    let again = t.member.add_attachment(id, "copy.txt", &one.hash, "txt").await.unwrap();
    assert!(again.duplicate);
    assert_eq!(again.attachment.id, first.attachment.id);
    let two = store.put_bytes(b"version two").await.unwrap();
    let second = t.owner.add_attachment(id, "report.txt", &two.hash, "txt").await.unwrap();
    assert_eq!(second.attachment.version, 2);

    let three = store.put_bytes(b"an outsider's file").await.unwrap();
    assert_eq!(t.outsider.add_attachment(id, "x.txt", &three.hash, "txt").await.map(|_| ()).unwrap_err().code, "NO_PERMISSION");

    // A large file survives the chunked round trip, checked against its hash.
    let big: Vec<u8> = (0..3_500_000u32).map(|i| (i % 251) as u8).collect();
    let blob = store.put_bytes(&big).await.unwrap();
    let dir = std::env::temp_dir().join(format!("a3-store-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join("out.bin");
    store.get_to_file(&blob.hash, &dest).await.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), big);
    let from_file = store.put_file(&dest).await.unwrap();
    assert_eq!(from_file.hash, blob.hash, "the same content hashes the same from either path");
    std::fs::remove_dir_all(dir).unwrap();

    // FR-ATT-05: owner only, logged, reversible.
    let aid = second.attachment.id;
    assert_eq!(t.member.delete_attachment(aid).await.unwrap_err().code, "A3_NOT_OWNER");
    t.owner.delete_attachment(aid).await.unwrap();
    assert!(t.owner.attachment(aid).await.unwrap().deleted_at.is_some());
    assert!(t.owner.history(id).await.unwrap().iter().any(|h| h.action == "attachment_deleted"));
    t.owner.restore_attachment(aid).await.unwrap();
    assert!(t.owner.attachment(aid).await.unwrap().deleted_at.is_none());
}

#[tokio::test]
async fn warnings_read_state_updates_and_roll_up() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let id = d.sheet.id;
    let wid = t
        .owner
        .post_warning(&NewWarning {
            sheet_id: id,
            headline: "Cracks on bracket".into(),
            what_happened: "Scrap rose to 3.5% on line 4.".into(),
            who_should_check: vec!["Line 4".into()],
            quick_check: "Check die 4.".into(),
            severity: "High".into(),
        })
        .await
        .unwrap();

    // Posted while B is open, so the card says so.
    let w = t.member.warning(wid).await.unwrap();
    assert_eq!(w.cause_status, "Cause not confirmed");
    assert!(w.unread, "unread until marked");

    // Read state is per user. Opening a card does not mark it read.
    t.member.set_warning_read(wid, true).await.unwrap();
    assert!(!t.member.warning(wid).await.unwrap().unread);
    assert!(t.outsider.warning(wid).await.unwrap().unread);

    // An update returns the card to Unread, tagged Updated. Only the author can update.
    assert!(t.member.add_warning_update(wid, "Not mine").await.is_err());
    t.owner.add_warning_update(wid, "Root cause found").await.unwrap();
    let w = t.member.warning(wid).await.unwrap();
    assert!(w.unread);
    assert!(w.updated);
    assert_eq!(t.member.warning_updates(wid).await.unwrap().len(), 1);

    // The roll-up, with any reader creating tasks.
    let mk = |desc: &'static str| NewTask { warning_id: wid, description: desc.into(), assignee_id: None, target_area: "Line 7".into(), due_on: None };
    let t1 = t.outsider.create_task(&mk("Check die")).await.unwrap();
    let t2 = t.member.create_task(&mk("Check die too")).await.unwrap();
    t.outsider.close_task(t1, "Not affected").await.unwrap();
    assert_eq!(t.member.close_task(t1, "Affected").await.unwrap_err().code, "NO_PERMISSION", "only the assignee or creator");
    t.member.close_task(t2, "Affected").await.unwrap();
    let w = t.owner.warning(wid).await.unwrap();
    assert_eq!((w.checks_total, w.not_affected, w.affected, w.checks_open), (2, 1, 1, 0));

    // Affected creates a linked A3, pre-filled and linked both ways, once.
    let other = t.member.create_project("Other", &format!("O{}", &uuid::Uuid::new_v4().simple().to_string()[..8]), "").await.unwrap();
    let linked = t.member.create_linked_a3(t2, other.id).await.unwrap();
    let ld = t.member.sheet_detail(linked).await.unwrap();
    assert_eq!(ld.sheet.title, "Cracks on bracket");
    assert_eq!(ld.sheet.origin_warning_id, Some(wid));
    assert!(step_of(&ld, "A").plain_text.contains("Scrap rose to 3.5%"));
    assert_eq!(t.member.create_linked_a3(t2, other.id).await.unwrap(), linked);
    let tasks = t.owner.tasks_for_warning(wid).await.unwrap();
    assert_eq!(tasks.iter().find(|x| x.id == t2).unwrap().linked_a3_id, Some(linked));

    // Withdrawn cards stay visible, labelled.
    assert!(t.member.withdraw_warning(wid, "x").await.is_err());
    t.owner.withdraw_warning(wid, "Duplicate").await.unwrap();
    assert!(t.member.warning(wid).await.unwrap().withdrawn_at.is_some());

    // Later A3 edits never change what people were told.
    fill_step(&t.owner, id, "A", "Different text").await;
    assert_eq!(t.member.warning(wid).await.unwrap().what_happened, "Scrap rose to 3.5% on line 4.");
}

#[tokio::test]
async fn warning_field_limits_are_enforced() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let base = NewWarning {
        sheet_id: d.sheet.id,
        headline: "h".repeat(81),
        what_happened: "ok".into(),
        who_should_check: vec![],
        quick_check: "".into(),
        severity: "Low".into(),
    };
    assert!(t.owner.post_warning(&base).await.is_err());
    let long = NewWarning { headline: "ok".into(), what_happened: vec!["w"; 41].join(" "), ..base.clone() };
    assert!(t.owner.post_warning(&long).await.is_err());
    let lines = NewWarning { headline: "ok".into(), what_happened: "ok".into(), quick_check: "a\nb\nc".into(), ..base };
    assert!(t.owner.post_warning(&lines).await.is_err());
}

#[tokio::test]
async fn projects_and_settings_respect_roles() {
    let t = team().await;
    assert!(t.member.set_project_ai(t.project.id, false).await.is_err());
    t.owner.set_project_ai(t.project.id, false).await.unwrap();
    assert!(t.outsider.add_project_member(t.project.id, t.outsider.me().id).await.is_err());
    assert!(t.member.remove_project_member(t.project.id, t.owner.me().id).await.is_err());
    t.owner.add_pick_value("Area", "Line 4", Some(t.project.id)).await.unwrap();
    assert!(t.member.add_pick_value("Area", "Line 9", Some(t.project.id)).await.is_err());
    assert!(t.member.add_pick_value("Category", "Hacked", None).await.is_err());
    assert!(t.owner.pick_values(Some(t.project.id)).await.unwrap().iter().any(|p| p.value == "Line 4"));

    assert!(t.member.set_settings(&Settings { stalled_days: 3, link_base: "".into() }).await.is_err());
    assert!(t.member.admin_create_user("sneaky", "S", "", "password1", true).await.is_err());
    let admin = new_user(true).await;
    let cur = admin.get_settings().await.unwrap();
    assert!(admin.set_settings(&Settings { stalled_days: 14, link_base: "http://not-https".into() }).await.is_err());
    admin.set_settings(&cur).await.unwrap();

    // An admin creates, resets and deactivates a real login.
    let login = format!("u{}", uuid::Uuid::new_v4().simple());
    let uid = admin.admin_create_user(&login, "New person", "n@example.com", "first-password", false).await.unwrap();
    let pool = a3_core::connect::sign_in(&profile(), &login, "first-password").await.unwrap();
    pool.close().await;
    admin.admin_reset_password(uid, "second-password").await.unwrap();
    assert_eq!(a3_core::connect::sign_in(&profile(), &login, "first-password").await.unwrap_err().code, "BAD_LOGIN");
    let pool = a3_core::connect::sign_in(&profile(), &login, "second-password").await.unwrap();
    let user = PgRepository::open(pool).await.unwrap();
    user.change_my_password("third-password").await.unwrap();
    a3_core::connect::sign_in(&profile(), &login, "third-password").await.unwrap().close().await;
    admin.admin_set_active(uid, false).await.unwrap();
    assert!(a3_core::connect::sign_in(&profile(), &login, "third-password").await.is_err());

    // Names that could hijack a system role are refused.
    for bad in ["postgres", "pg_read_all_data", "a3_owner", "Bad Name", "x"] {
        assert!(admin.admin_create_user(bad, "X", "", "password12", false).await.is_err(), "{bad}");
    }
}

#[tokio::test]
async fn dashboard_and_management_numbers_follow_the_data() {
    let t = team().await;
    let d = new_sheet(&t.owner, t.project.id).await;
    let id = d.sheet.id;
    fill_step(&t.owner, id, "A", "Text").await;
    t.owner.close_step(step_of(&d, "A").id).await.unwrap();

    let wid = t.owner
        .post_warning(&NewWarning { sheet_id: id, headline: "H".into(), what_happened: "W".into(), who_should_check: vec![], quick_check: "".into(), severity: "Low".into() })
        .await
        .unwrap();
    t.member.create_task(&NewTask { warning_id: wid, description: "Check".into(), assignee_id: None, target_area: "".into(), due_on: None }).await.unwrap();

    let work = t.owner.my_work().await.unwrap();
    assert!(work.my_sheets.iter().any(|s| s.id == id));
    assert!(work.unread_warnings.iter().any(|w| w.id == wid));
    assert!(work.unread_warnings.len() <= 5);
    let member_work = t.member.my_work().await.unwrap();
    assert_eq!(member_work.my_tasks.len(), 1);

    let f = MgmtFilter { project_id: Some(t.project.id), ..Default::default() };
    let m = t.owner.management(&f).await.unwrap();
    let count = |l: &str| m.by_step.iter().find(|x| x.label == l).unwrap().count;
    assert_eq!((count("A"), count("B"), count("C"), count("D")), (0, 1, 0, 0));
    assert_eq!(m.by_age.iter().find(|x| x.label == "0-30").unwrap().count, 1);
    assert_eq!(m.closed_count, 0);
    t.owner.reopen_step(step_of(&d, "A").id, "Correction").await.unwrap();
    let m = t.owner.management(&f).await.unwrap();
    assert_eq!(m.reopened_count, 1);
    assert_eq!(m.reopened_by_reason[0].label, "Correction");
    let reach = m.warning_reach.iter().find(|r| r.id == wid).unwrap();
    assert_eq!(reach.checks_open, 1);

}
