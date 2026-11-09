-- A3 Sheet: identity, permissions and step rules (spec sections 4, 13, 14).
-- Rules live here so a direct database connection cannot get around them.
--
-- Errors raised for users take the form 'A3_CODE|Plain message'. The app shows the plain message.

-- ---------------------------------------------------------------- identity helpers

CREATE FUNCTION app_uid() RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS
$$ SELECT id FROM app_user WHERE role_name = session_user AND active $$;

CREATE FUNCTION is_admin() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS
$$ SELECT COALESCE((SELECT is_admin FROM app_user WHERE role_name = session_user AND active), false) $$;

CREATE FUNCTION is_project_member(p uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS
$$ SELECT EXISTS (SELECT 1 FROM project_member WHERE project_id = p AND user_id = app_uid()) $$;

CREATE FUNCTION is_project_owner(p uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS
$$ SELECT EXISTS (SELECT 1 FROM project WHERE id = p AND owner_id = app_uid()) $$;

CREATE FUNCTION can_edit_sheet(s uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS
$$ SELECT EXISTS (
     SELECT 1 FROM a3_sheet sh
     JOIN project_member m ON m.project_id = sh.project_id
     WHERE sh.id = s AND m.user_id = app_uid()) $$;

CREATE FUNCTION is_sheet_owner(s uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS
$$ SELECT EXISTS (SELECT 1 FROM a3_sheet WHERE id = s AND owner_id = app_uid()) $$;

CREATE FUNCTION can_edit_attachment(a uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS
$$ SELECT can_edit_sheet((SELECT sheet_id FROM attachment WHERE id = a)) $$;

CREATE FUNCTION is_schema_owner() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS
$$ SELECT session_user = (SELECT tableowner FROM pg_tables WHERE schemaname = 'public' AND tablename = 'schema_meta') $$;

-- ---------------------------------------------------------------- A3 numbers

-- A3-2026-0042: a per-year sequence, so numbers stay unique across concurrent users.
CREATE FUNCTION next_a3_number() RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  y   text := to_char(CURRENT_DATE, 'YYYY');
  seq text := 'a3_number_' || y;
BEGIN
  PERFORM pg_advisory_xact_lock(hashtext(seq));
  EXECUTE format('CREATE SEQUENCE IF NOT EXISTS %I', seq);
  RETURN 'A3-' || y || '-' || lpad(nextval(seq::regclass)::text, 4, '0');
END
$$;

-- ---------------------------------------------------------------- sheet triggers

CREATE FUNCTION a3_sheet_before_insert() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
BEGIN
  NEW.number := next_a3_number();
  NEW.registered_on := CURRENT_DATE;
  NEW.created_at := now();
  NEW.updated_at := now();
  RETURN NEW;
END
$$;
CREATE TRIGGER a3_sheet_before_insert BEFORE INSERT ON a3_sheet
  FOR EACH ROW EXECUTE FUNCTION a3_sheet_before_insert();

-- Exactly four step rows per sheet.
CREATE FUNCTION a3_sheet_after_insert() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
BEGIN
  INSERT INTO a3_step (sheet_id, letter)
  SELECT NEW.id, l FROM unnest(ARRAY['A', 'B', 'C', 'D']) AS l;
  RETURN NULL;
END
$$;
CREATE TRIGGER a3_sheet_after_insert AFTER INSERT ON a3_sheet
  FOR EACH ROW EXECUTE FUNCTION a3_sheet_after_insert();

CREATE FUNCTION a3_sheet_before_update() RETURNS trigger
LANGUAGE plpgsql AS
$$
BEGIN
  IF NEW.owner_id IS DISTINCT FROM OLD.owner_id THEN
    IF NOT (is_sheet_owner(OLD.id) OR is_project_owner(OLD.project_id)) THEN
      RAISE EXCEPTION 'A3_NOT_OWNER|Only the A3 owner can transfer ownership.';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM project_member WHERE project_id = NEW.project_id AND user_id = NEW.owner_id) THEN
      RAISE EXCEPTION 'A3_OWNER_NOT_MEMBER|The new owner must be a member of the project.';
    END IF;
  END IF;
  IF NEW.withdrawn_at IS DISTINCT FROM OLD.withdrawn_at AND NOT is_sheet_owner(OLD.id) THEN
    RAISE EXCEPTION 'A3_NOT_OWNER|Only the A3 owner can withdraw it.';
  END IF;
  NEW.updated_at := now();
  RETURN NEW;
END
$$;
CREATE TRIGGER a3_sheet_before_update BEFORE UPDATE ON a3_sheet
  FOR EACH ROW EXECUTE FUNCTION a3_sheet_before_update();

CREATE FUNCTION touch_sheet(s uuid) RETURNS void
LANGUAGE sql SECURITY DEFINER SET search_path = public, pg_temp AS
$$ UPDATE a3_sheet SET updated_at = now() WHERE id = s AND updated_at < now() $$;

-- ---------------------------------------------------------------- step rules (FR-ST-01 to 08)

-- A closed step is read-only. Another user's live lock blocks a save. Every content change bumps the version.
CREATE FUNCTION a3_step_before_update() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  me uuid := app_uid();
BEGIN
  IF NEW.content IS DISTINCT FROM OLD.content OR NEW.plain_text IS DISTINCT FROM OLD.plain_text THEN
    IF OLD.state = 'closed' THEN
      RAISE EXCEPTION 'A3_STEP_CLOSED|Step % is closed. Reopen it to edit.', OLD.letter;
    END IF;
    IF OLD.locked_by IS NOT NULL AND OLD.locked_by IS DISTINCT FROM me
       AND OLD.locked_at > now() - interval '2 minutes' THEN
      RAISE EXCEPTION 'A3_STEP_LOCKED|Step % is being edited by someone else.', OLD.letter;
    END IF;
    NEW.version := OLD.version + 1;
    NEW.updated_at := now();
    NEW.updated_by := me;
    PERFORM touch_sheet(OLD.sheet_id);
  END IF;
  RETURN NEW;
END
$$;
CREATE TRIGGER a3_step_before_update BEFORE UPDATE ON a3_step
  FOR EACH ROW EXECUTE FUNCTION a3_step_before_update();

CREATE FUNCTION close_step(p_step uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  st a3_step;
  sh a3_sheet;
  me uuid := app_uid();
BEGIN
  SELECT * INTO st FROM a3_step WHERE id = p_step;
  IF NOT FOUND THEN RAISE EXCEPTION 'A3_NOT_FOUND|Step not found.'; END IF;
  SELECT * INTO sh FROM a3_sheet WHERE id = st.sheet_id FOR UPDATE;
  IF me IS NULL OR sh.owner_id <> me THEN
    RAISE EXCEPTION 'A3_NOT_OWNER|Only the A3 owner can close a step.';
  END IF;
  IF sh.withdrawn_at IS NOT NULL THEN
    RAISE EXCEPTION 'A3_WITHDRAWN|This A3 has been withdrawn.';
  END IF;
  SELECT * INTO st FROM a3_step WHERE id = p_step;
  IF st.state = 'closed' THEN
    RAISE EXCEPTION 'A3_ALREADY_CLOSED|Step % is already closed.', st.letter;
  END IF;
  IF btrim(st.plain_text) = '' THEN
    RAISE EXCEPTION 'A3_STEP_EMPTY|Step % has no text yet, so it cannot close.', st.letter;
  END IF;
  IF EXISTS (SELECT 1 FROM a3_step WHERE sheet_id = st.sheet_id AND letter < st.letter AND state = 'open') THEN
    RAISE EXCEPTION 'A3_STEP_ORDER|Steps close in order A, B, C, D. Close the earlier steps first.';
  END IF;
  UPDATE a3_step
     SET state = 'closed', closed_at = now(), closed_by = me, locked_by = NULL, locked_at = NULL
   WHERE id = p_step;
  INSERT INTO a3_step_snapshot (step_id, sheet_id, letter, content, plain_text, version, author_id, reason)
  VALUES (st.id, st.sheet_id, st.letter, st.content, st.plain_text, st.version, me, 'close');
  INSERT INTO audit_event (actor_id, object_type, object_id, sheet_id, action, detail)
  VALUES (me, 'a3_step', st.id, st.sheet_id, 'step_closed', jsonb_build_object('letter', st.letter));
  PERFORM touch_sheet(st.sheet_id);
END
$$;

CREATE FUNCTION reopen_step(p_step uuid, p_reason text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  st a3_step;
  sh a3_sheet;
  me uuid := app_uid();
BEGIN
  IF p_reason NOT IN ('New evidence', 'Result not sustained', 'Correction', 'Other') THEN
    RAISE EXCEPTION 'A3_BAD_REASON|Choose a reason for reopening.';
  END IF;
  SELECT * INTO st FROM a3_step WHERE id = p_step;
  IF NOT FOUND THEN RAISE EXCEPTION 'A3_NOT_FOUND|Step not found.'; END IF;
  SELECT * INTO sh FROM a3_sheet WHERE id = st.sheet_id FOR UPDATE;
  IF me IS NULL OR sh.owner_id <> me THEN
    RAISE EXCEPTION 'A3_NOT_OWNER|Only the A3 owner can reopen a step.';
  END IF;
  SELECT * INTO st FROM a3_step WHERE id = p_step;
  IF st.state = 'open' THEN
    RAISE EXCEPTION 'A3_ALREADY_OPEN|Step % is already open.', st.letter;
  END IF;
  UPDATE a3_step
     SET state = 'open', closed_at = NULL, closed_by = NULL, reopen_count = reopen_count + 1
   WHERE id = p_step;
  INSERT INTO a3_step_snapshot (step_id, sheet_id, letter, content, plain_text, version, author_id, reason)
  VALUES (st.id, st.sheet_id, st.letter, st.content, st.plain_text, st.version, me, 'reopen');
  INSERT INTO audit_event (actor_id, object_type, object_id, sheet_id, action, reason, detail)
  VALUES (me, 'a3_step', st.id, st.sheet_id, 'step_reopened', p_reason, jsonb_build_object('letter', st.letter));
  PERFORM touch_sheet(st.sheet_id);
END
$$;

-- FR-A3-10: the owner restores a snapshot, which creates a new snapshot.
CREATE FUNCTION restore_step_snapshot(p_snapshot uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  sn a3_step_snapshot;
  v  integer;
  me uuid := app_uid();
BEGIN
  SELECT * INTO sn FROM a3_step_snapshot WHERE id = p_snapshot;
  IF NOT FOUND THEN RAISE EXCEPTION 'A3_NOT_FOUND|Snapshot not found.'; END IF;
  IF NOT is_sheet_owner(sn.sheet_id) THEN
    RAISE EXCEPTION 'A3_NOT_OWNER|Only the A3 owner can restore a snapshot.';
  END IF;
  UPDATE a3_step SET content = sn.content, plain_text = sn.plain_text
   WHERE id = sn.step_id RETURNING version INTO v;
  INSERT INTO a3_step_snapshot (step_id, sheet_id, letter, content, plain_text, version, author_id, reason)
  VALUES (sn.step_id, sn.sheet_id, sn.letter, sn.content, sn.plain_text, v, me, 'restore');
  INSERT INTO audit_event (actor_id, object_type, object_id, sheet_id, action, detail)
  VALUES (me, 'a3_step', sn.step_id, sn.sheet_id, 'snapshot_restored', jsonb_build_object('letter', sn.letter, 'from', sn.created_at));
END
$$;

-- FR-A3-07: one editor per step. Returns the current holder's id: the caller's when acquired.
CREATE FUNCTION acquire_step_lock(p_step uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  st a3_step;
  me uuid := app_uid();
BEGIN
  SELECT * INTO st FROM a3_step WHERE id = p_step FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'A3_NOT_FOUND|Step not found.'; END IF;
  IF NOT can_edit_sheet(st.sheet_id) THEN
    RAISE EXCEPTION 'A3_NOT_MEMBER|Only project members can edit this A3.';
  END IF;
  IF st.state = 'closed' THEN
    RAISE EXCEPTION 'A3_STEP_CLOSED|Step % is closed. Reopen it to edit.', st.letter;
  END IF;
  IF st.locked_by IS NOT NULL AND st.locked_by <> me AND st.locked_at > now() - interval '2 minutes' THEN
    RETURN st.locked_by;
  END IF;
  UPDATE a3_step SET locked_by = me, locked_at = now() WHERE id = p_step;
  RETURN me;
END
$$;

CREATE FUNCTION release_step_lock(p_step uuid) RETURNS void
LANGUAGE sql SECURITY DEFINER SET search_path = public, pg_temp AS
$$ UPDATE a3_step SET locked_by = NULL, locked_at = NULL WHERE id = p_step AND locked_by = app_uid() $$;

-- ---------------------------------------------------------------- derived status (section 4)

CREATE VIEW v_a3_status AS
WITH agg AS (
  SELECT sheet_id,
         bool_and(state = 'closed')                        AS all_closed,
         min(letter) FILTER (WHERE state = 'open')         AS current_step,
         max(closed_at)                                    AS last_close,
         bool_or(state = 'open' AND reopen_count > 0)      AS reopened,
         string_agg(CASE WHEN state = 'closed' THEN 'C' ELSE 'O' END, '' ORDER BY letter) AS step_strip
  FROM a3_step
  GROUP BY sheet_id
)
SELECT sh.id AS sheet_id,
       CASE WHEN agg.all_closed THEN 'Closed' ELSE 'Open' END AS status,
       agg.current_step,
       CASE WHEN agg.all_closed THEN agg.last_close END AS closed_at,
       agg.step_strip,
       sh.withdrawn_at IS NOT NULL AS withdrawn,
       (NOT agg.all_closed AND sh.withdrawn_at IS NULL
        AND sh.target_close_date IS NOT NULL AND sh.target_close_date < CURRENT_DATE) AS overdue,
       (NOT agg.all_closed AND sh.withdrawn_at IS NULL
        AND sh.updated_at < now() - make_interval(days => (SELECT value::int FROM app_setting WHERE key = 'stalled_days'))) AS stalled,
       (agg.reopened AND sh.withdrawn_at IS NULL) AS reopened
FROM a3_sheet sh
JOIN agg ON agg.sheet_id = sh.id;

-- ---------------------------------------------------------------- attachment rules

-- FR-ATT-05: deleting needs the owner and is reversible for 30 days.
CREATE FUNCTION attachment_before_update() RETURNS trigger
LANGUAGE plpgsql AS
$$
BEGIN
  IF NEW.deleted_at IS DISTINCT FROM OLD.deleted_at AND NOT is_sheet_owner(OLD.sheet_id) THEN
    RAISE EXCEPTION 'A3_NOT_OWNER|Only the A3 owner can delete or restore an attachment.';
  END IF;
  RETURN NEW;
END
$$;
CREATE TRIGGER attachment_before_update BEFORE UPDATE ON attachment
  FOR EACH ROW EXECUTE FUNCTION attachment_before_update();

CREATE FUNCTION attachment_after_insert() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
BEGIN
  PERFORM touch_sheet(NEW.sheet_id);
  RETURN NULL;
END
$$;
CREATE TRIGGER attachment_after_insert AFTER INSERT ON attachment
  FOR EACH ROW EXECUTE FUNCTION attachment_after_insert();

-- Purges attachments past the 30-day restore window, then blobs nothing references.
CREATE FUNCTION purge_deleted_attachments() RETURNS integer
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  n integer;
BEGIN
  DELETE FROM attachment WHERE deleted_at < now() - interval '30 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  DELETE FROM attachment_blob b
   WHERE created_at < now() - interval '1 day'
     AND NOT EXISTS (SELECT 1 FROM attachment a WHERE a.blob_hash = b.hash);
  RETURN n;
END
$$;

-- ---------------------------------------------------------------- warning rules

-- FR-EW-03: an update returns the card to Unread for everyone who had read it, tagged Updated.
CREATE FUNCTION early_warning_update_after_insert() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
BEGIN
  UPDATE early_warning SET last_update_at = now() WHERE id = NEW.warning_id;
  UPDATE warning_read_state SET unread = true, updated = true WHERE warning_id = NEW.warning_id;
  RETURN NULL;
END
$$;
CREATE TRIGGER early_warning_update_after_insert AFTER INSERT ON early_warning_update
  FOR EACH ROW EXECUTE FUNCTION early_warning_update_after_insert();

-- ---------------------------------------------------------------- user administration

CREATE FUNCTION assert_scram(p_verifier text) RETURNS void
LANGUAGE plpgsql IMMUTABLE AS
$$
BEGIN
  IF p_verifier !~ '^SCRAM-SHA-256\$[0-9]+:[A-Za-z0-9+/=]+\$[A-Za-z0-9+/=]+:[A-Za-z0-9+/=]+$' THEN
    RAISE EXCEPTION 'A3_BAD_VERIFIER|Passwords must reach the server as a SCRAM verifier.';
  END IF;
END
$$;

CREATE FUNCTION admin_create_user(p_role text, p_name text, p_email text, p_verifier text, p_is_admin boolean)
RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  new_id uuid;
BEGIN
  IF NOT (is_admin() OR is_schema_owner()) THEN
    RAISE EXCEPTION 'A3_NOT_ADMIN|Only an admin can create users.';
  END IF;
  IF p_role !~ '^[a-z][a-z0-9_.-]{1,62}$' OR p_role LIKE 'pg\_%' OR p_role LIKE 'a3\_%' OR p_role = 'postgres' THEN
    RAISE EXCEPTION 'A3_BAD_LOGIN|Login names use lower-case letters, digits, dots, hyphens and underscores.';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = p_role) THEN
    RAISE EXCEPTION 'A3_LOGIN_TAKEN|That login name is already in use.';
  END IF;
  PERFORM assert_scram(p_verifier);
  EXECUTE format('CREATE ROLE %I LOGIN PASSWORD %L IN ROLE a3_user', p_role, p_verifier);
  INSERT INTO app_user (role_name, display_name, email, is_admin)
  VALUES (p_role, p_name, p_email, p_is_admin) RETURNING id INTO new_id;
  INSERT INTO audit_event (actor_id, object_type, object_id, action, detail)
  VALUES (app_uid(), 'app_user', new_id, 'user_created', jsonb_build_object('role', p_role, 'is_admin', p_is_admin));
  RETURN new_id;
END
$$;

CREATE FUNCTION admin_set_user_active(p_user uuid, p_active boolean) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  r text;
BEGIN
  IF NOT is_admin() THEN RAISE EXCEPTION 'A3_NOT_ADMIN|Only an admin can change users.'; END IF;
  IF p_user = app_uid() AND NOT p_active THEN
    RAISE EXCEPTION 'A3_SELF|You cannot deactivate your own account.';
  END IF;
  UPDATE app_user SET active = p_active WHERE id = p_user RETURNING role_name INTO r;
  IF r IS NULL THEN RAISE EXCEPTION 'A3_NOT_FOUND|User not found.'; END IF;
  EXECUTE format('ALTER ROLE %I %s', r, CASE WHEN p_active THEN 'LOGIN' ELSE 'NOLOGIN' END);
  INSERT INTO audit_event (actor_id, object_type, object_id, action)
  VALUES (app_uid(), 'app_user', p_user, CASE WHEN p_active THEN 'user_activated' ELSE 'user_deactivated' END);
END
$$;

CREATE FUNCTION admin_set_user_admin(p_user uuid, p_is_admin boolean) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
BEGIN
  IF NOT is_admin() THEN RAISE EXCEPTION 'A3_NOT_ADMIN|Only an admin can change users.'; END IF;
  IF p_user = app_uid() AND NOT p_is_admin THEN
    RAISE EXCEPTION 'A3_SELF|You cannot remove your own admin rights.';
  END IF;
  UPDATE app_user SET is_admin = p_is_admin WHERE id = p_user;
  INSERT INTO audit_event (actor_id, object_type, object_id, action, detail)
  VALUES (app_uid(), 'app_user', p_user, 'user_admin_changed', jsonb_build_object('is_admin', p_is_admin));
END
$$;

CREATE FUNCTION admin_reset_password(p_user uuid, p_verifier text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
DECLARE
  r text;
BEGIN
  IF NOT (is_admin() OR is_schema_owner()) THEN RAISE EXCEPTION 'A3_NOT_ADMIN|Only an admin can reset passwords.'; END IF;
  PERFORM assert_scram(p_verifier);
  SELECT role_name INTO r FROM app_user WHERE id = p_user;
  IF r IS NULL THEN RAISE EXCEPTION 'A3_NOT_FOUND|User not found.'; END IF;
  EXECUTE format('ALTER ROLE %I PASSWORD %L', r, p_verifier);
  INSERT INTO audit_event (actor_id, object_type, object_id, action)
  VALUES (app_uid(), 'app_user', p_user, 'password_reset');
END
$$;

CREATE FUNCTION change_my_password(p_verifier text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
BEGIN
  IF app_uid() IS NULL THEN RAISE EXCEPTION 'A3_NOT_USER|No account for this login.'; END IF;
  PERFORM assert_scram(p_verifier);
  EXECUTE format('ALTER ROLE %I PASSWORD %L', session_user, p_verifier);
  INSERT INTO audit_event (actor_id, object_type, object_id, action)
  VALUES (app_uid(), 'app_user', app_uid(), 'password_changed');
END
$$;

CREATE FUNCTION project_after_insert() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS
$$
BEGIN
  INSERT INTO project_member (project_id, user_id) VALUES (NEW.id, NEW.owner_id);
  RETURN NULL;
END
$$;
CREATE TRIGGER project_after_insert AFTER INSERT ON project
  FOR EACH ROW EXECUTE FUNCTION project_after_insert();

-- ---------------------------------------------------------------- privileges

REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO a3_user;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA public TO a3_user;
-- Internal helpers: a user calling touch_sheet directly could hide a stalled A3.
REVOKE EXECUTE ON FUNCTION touch_sheet(uuid), next_a3_number(), assert_scram(text) FROM a3_user;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO a3_user;

GRANT SELECT ON ALL TABLES IN SCHEMA public TO a3_user;

-- Column lists keep derived and automatic fields out of reach: a user cannot set a3_step.state
-- directly, so close_step and reopen_step are the only way to change it.
GRANT UPDATE (display_name, email, default_project_id, last_project_id) ON app_user TO a3_user;
GRANT INSERT, UPDATE ON project TO a3_user;
GRANT INSERT, DELETE ON project_member TO a3_user;
GRANT INSERT, DELETE ON pick_list_value TO a3_user;
GRANT UPDATE ON app_setting TO a3_user;
GRANT INSERT (id, title, project_id, owner_id, team_member_ids, problem_date, target_close_date,
              category, severity, source, area, product, origin_warning_id, origin_task_id) ON a3_sheet TO a3_user;
GRANT UPDATE (title, project_id, owner_id, team_member_ids, problem_date, target_close_date,
              category, severity, source, area, product, withdrawn_at, withdrawn_by, withdrawn_reason) ON a3_sheet TO a3_user;
GRANT UPDATE (content, plain_text, locked_by, locked_at) ON a3_step TO a3_user;
GRANT INSERT ON a3_step_snapshot TO a3_user;
GRANT INSERT ON attachment_blob, attachment_chunk TO a3_user;
GRANT UPDATE (complete) ON attachment_blob TO a3_user;
GRANT INSERT ON attachment TO a3_user;
GRANT UPDATE (read_state, read_error, ai_state, ai_error, ai_note, summary, excluded_sheets, confirms_count, deleted_at, deleted_by) ON attachment TO a3_user;
GRANT INSERT, UPDATE, DELETE ON source_segment TO a3_user;
GRANT INSERT ON ai_run TO a3_user;
GRANT INSERT, UPDATE ON a3_fact, ai_proposal, ai_finding TO a3_user;
GRANT INSERT ON ai_message TO a3_user;
GRANT INSERT ON early_warning TO a3_user;
GRANT UPDATE (withdrawn_at, withdrawn_reason) ON early_warning TO a3_user;
GRANT INSERT ON early_warning_update TO a3_user;
GRANT INSERT, UPDATE ON warning_read_state TO a3_user;
GRANT INSERT, UPDATE, DELETE ON follow_up_task TO a3_user;
GRANT INSERT ON audit_event TO a3_user;
GRANT USAGE ON SEQUENCE audit_event_id_seq TO a3_user;

-- ---------------------------------------------------------------- row-level security (D3)
-- All signed-in users can read everything. Writes are limited below.

ALTER TABLE app_user ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON app_user FOR SELECT USING (app_uid() IS NOT NULL OR role_name = session_user);
CREATE POLICY update_self ON app_user FOR UPDATE USING (id = app_uid()) WITH CHECK (id = app_uid());

ALTER TABLE app_setting ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON app_setting FOR SELECT USING (true);
CREATE POLICY admin_write ON app_setting FOR UPDATE USING (is_admin()) WITH CHECK (is_admin());

ALTER TABLE project ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON project FOR SELECT USING (true);
CREATE POLICY create_own ON project FOR INSERT WITH CHECK (owner_id = app_uid());
CREATE POLICY owner_update ON project FOR UPDATE USING (owner_id = app_uid()) WITH CHECK (owner_id = app_uid());

ALTER TABLE project_member ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON project_member FOR SELECT USING (true);
CREATE POLICY owner_add ON project_member FOR INSERT WITH CHECK (is_project_owner(project_id));
CREATE POLICY owner_remove ON project_member FOR DELETE USING (is_project_owner(project_id));

ALTER TABLE pick_list_value ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON pick_list_value FOR SELECT USING (true);
CREATE POLICY manage_insert ON pick_list_value FOR INSERT
  WITH CHECK (CASE WHEN project_id IS NULL THEN is_admin() ELSE is_project_owner(project_id) END);
CREATE POLICY manage_delete ON pick_list_value FOR DELETE
  USING (CASE WHEN project_id IS NULL THEN is_admin() ELSE is_project_owner(project_id) END);

ALTER TABLE a3_sheet ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON a3_sheet FOR SELECT USING (true);
CREATE POLICY member_insert ON a3_sheet FOR INSERT
  WITH CHECK (is_project_member(project_id) AND owner_id = app_uid());
CREATE POLICY member_update ON a3_sheet FOR UPDATE
  USING (is_project_member(project_id)) WITH CHECK (is_project_member(project_id));

ALTER TABLE a3_step ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON a3_step FOR SELECT USING (true);
CREATE POLICY member_update ON a3_step FOR UPDATE
  USING (can_edit_sheet(sheet_id)) WITH CHECK (can_edit_sheet(sheet_id));

ALTER TABLE a3_step_snapshot ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON a3_step_snapshot FOR SELECT USING (true);
CREATE POLICY member_insert ON a3_step_snapshot FOR INSERT
  WITH CHECK (can_edit_sheet(sheet_id) AND author_id = app_uid());

ALTER TABLE attachment_blob ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON attachment_blob FOR SELECT USING (true);
CREATE POLICY user_insert ON attachment_blob FOR INSERT WITH CHECK (app_uid() IS NOT NULL);
CREATE POLICY finish_upload ON attachment_blob FOR UPDATE
  USING (NOT complete AND app_uid() IS NOT NULL) WITH CHECK (app_uid() IS NOT NULL);

ALTER TABLE attachment_chunk ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON attachment_chunk FOR SELECT USING (true);
CREATE POLICY user_insert ON attachment_chunk FOR INSERT WITH CHECK (
  app_uid() IS NOT NULL
  AND EXISTS (SELECT 1 FROM attachment_blob b WHERE b.hash = attachment_chunk.hash AND NOT b.complete));

ALTER TABLE attachment ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON attachment FOR SELECT USING (true);
CREATE POLICY member_insert ON attachment FOR INSERT
  WITH CHECK (can_edit_sheet(sheet_id) AND added_by = app_uid());
CREATE POLICY member_update ON attachment FOR UPDATE
  USING (can_edit_sheet(sheet_id)) WITH CHECK (can_edit_sheet(sheet_id));

ALTER TABLE source_segment ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON source_segment FOR SELECT USING (true);
CREATE POLICY member_insert ON source_segment FOR INSERT WITH CHECK (can_edit_attachment(attachment_id));
CREATE POLICY member_update ON source_segment FOR UPDATE USING (can_edit_attachment(attachment_id));
CREATE POLICY member_delete ON source_segment FOR DELETE USING (can_edit_attachment(attachment_id));

ALTER TABLE ai_run ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON ai_run FOR SELECT USING (true);
CREATE POLICY own_insert ON ai_run FOR INSERT WITH CHECK (user_id = app_uid());

ALTER TABLE a3_fact ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON a3_fact FOR SELECT USING (true);
CREATE POLICY member_insert ON a3_fact FOR INSERT WITH CHECK (can_edit_sheet(sheet_id));
CREATE POLICY member_update ON a3_fact FOR UPDATE USING (can_edit_sheet(sheet_id));

ALTER TABLE ai_proposal ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON ai_proposal FOR SELECT USING (true);
CREATE POLICY member_insert ON ai_proposal FOR INSERT WITH CHECK (can_edit_sheet(sheet_id));
CREATE POLICY member_update ON ai_proposal FOR UPDATE USING (can_edit_sheet(sheet_id));

ALTER TABLE ai_finding ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON ai_finding FOR SELECT USING (true);
CREATE POLICY member_insert ON ai_finding FOR INSERT WITH CHECK (can_edit_sheet(sheet_id));
CREATE POLICY member_update ON ai_finding FOR UPDATE USING (can_edit_sheet(sheet_id));

-- Any reader may ask about an A3. A question changes nothing on the sheet.
ALTER TABLE ai_message ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON ai_message FOR SELECT USING (true);
CREATE POLICY own_insert ON ai_message FOR INSERT WITH CHECK (user_id = app_uid());

ALTER TABLE early_warning ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON early_warning FOR SELECT USING (true);
CREATE POLICY member_insert ON early_warning FOR INSERT
  WITH CHECK (can_edit_sheet(sheet_id) AND author_id = app_uid());
CREATE POLICY author_update ON early_warning FOR UPDATE
  USING (author_id = app_uid()) WITH CHECK (author_id = app_uid());

ALTER TABLE early_warning_update ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON early_warning_update FOR SELECT USING (true);
CREATE POLICY author_insert ON early_warning_update FOR INSERT WITH CHECK (
  author_id = app_uid()
  AND EXISTS (SELECT 1 FROM early_warning w WHERE w.id = warning_id AND w.author_id = app_uid()));

ALTER TABLE warning_read_state ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON warning_read_state FOR SELECT USING (true);
CREATE POLICY own_insert ON warning_read_state FOR INSERT WITH CHECK (user_id = app_uid());
CREATE POLICY own_update ON warning_read_state FOR UPDATE USING (user_id = app_uid()) WITH CHECK (user_id = app_uid());

ALTER TABLE follow_up_task ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON follow_up_task FOR SELECT USING (true);
CREATE POLICY own_insert ON follow_up_task FOR INSERT WITH CHECK (created_by = app_uid());
CREATE POLICY party_update ON follow_up_task FOR UPDATE
  USING (assignee_id = app_uid() OR created_by = app_uid());
CREATE POLICY party_delete ON follow_up_task FOR DELETE
  USING (assignee_id = app_uid() OR created_by = app_uid());

ALTER TABLE audit_event ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_all ON audit_event FOR SELECT USING (true);
CREATE POLICY own_insert ON audit_event FOR INSERT WITH CHECK (actor_id = app_uid());
