-- A3 Sheet: core schema (spec section 14).
-- Run by the admin migration tool as the schema owner. Users have no schema rights.

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'a3_user') THEN
    CREATE ROLE a3_user NOLOGIN;
  END IF;
END
$$;

CREATE TABLE schema_meta (
  version     integer PRIMARY KEY,
  applied_at  timestamptz NOT NULL DEFAULT now()
);

-- Company-wide settings set by an admin: stalled threshold and link base address.
-- Section 13 needs them and section 14 lists no home for them, so this is the one table added.
CREATE TABLE app_setting (
  key    text PRIMARY KEY,
  value  text NOT NULL
);
INSERT INTO app_setting (key, value) VALUES ('stalled_days', '14'), ('link_base', '');

CREATE TABLE app_user (
  id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  role_name           text NOT NULL UNIQUE,
  display_name        text NOT NULL,
  email               text NOT NULL DEFAULT '',
  is_admin            boolean NOT NULL DEFAULT false,
  active              boolean NOT NULL DEFAULT true,
  default_project_id  uuid,
  last_project_id     uuid,
  created_at          timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE project (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  name         text NOT NULL,
  code         text NOT NULL UNIQUE,
  description  text NOT NULL DEFAULT '',
  owner_id     uuid NOT NULL REFERENCES app_user (id),
  ai_enabled   boolean NOT NULL DEFAULT true,
  archived_at  timestamptz,
  created_at   timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE app_user
  ADD FOREIGN KEY (default_project_id) REFERENCES project (id) ON DELETE SET NULL,
  ADD FOREIGN KEY (last_project_id) REFERENCES project (id) ON DELETE SET NULL;

CREATE TABLE project_member (
  project_id  uuid NOT NULL REFERENCES project (id) ON DELETE CASCADE,
  user_id     uuid NOT NULL REFERENCES app_user (id),
  added_at    timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (project_id, user_id)
);

CREATE TABLE pick_list_value (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  list_name   text NOT NULL CHECK (list_name IN ('Category', 'Source', 'Area', 'Product')),
  value       text NOT NULL CHECK (btrim(value) <> ''),
  project_id  uuid REFERENCES project (id) ON DELETE CASCADE,
  sort_order  integer NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX pick_list_value_uq
  ON pick_list_value (list_name, value, COALESCE(project_id, '00000000-0000-0000-0000-000000000000'));

INSERT INTO pick_list_value (list_name, value, sort_order) VALUES
  ('Category', 'Quality', 1), ('Category', 'Safety', 2), ('Category', 'Delivery', 3),
  ('Category', 'Cost', 4), ('Category', 'Process', 5), ('Category', 'Other', 6),
  ('Source', 'Customer', 1), ('Source', 'Internal', 2), ('Source', 'Supplier', 3),
  ('Source', 'Audit', 4), ('Source', 'Field', 5);

CREATE TABLE a3_sheet (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  number             text NOT NULL UNIQUE,
  title              text NOT NULL DEFAULT '' CHECK (char_length(title) <= 90),
  project_id         uuid NOT NULL REFERENCES project (id),
  owner_id           uuid NOT NULL REFERENCES app_user (id),
  team_member_ids    uuid[] NOT NULL DEFAULT '{}',
  registered_on      date NOT NULL DEFAULT CURRENT_DATE,
  problem_date       date,
  target_close_date  date,
  category           text,
  severity           text CHECK (severity IN ('High', 'Medium', 'Low')),
  source             text,
  area               text,
  product            text,
  origin_warning_id  uuid,
  origin_task_id     uuid,
  withdrawn_at       timestamptz,
  withdrawn_by       uuid REFERENCES app_user (id),
  withdrawn_reason   text,
  created_at         timestamptz NOT NULL DEFAULT now(),
  updated_at         timestamptz NOT NULL DEFAULT now(),
  search             tsvector GENERATED ALWAYS AS (to_tsvector('english', number || ' ' || title)) STORED
);
CREATE INDEX a3_sheet_project_idx ON a3_sheet (project_id);
CREATE INDEX a3_sheet_owner_idx ON a3_sheet (owner_id);
CREATE INDEX a3_sheet_search_idx ON a3_sheet USING gin (search);

CREATE TABLE a3_step (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  sheet_id     uuid NOT NULL REFERENCES a3_sheet (id) ON DELETE CASCADE,
  letter       text NOT NULL CHECK (letter IN ('A', 'B', 'C', 'D')),
  content      jsonb NOT NULL DEFAULT '{"type":"doc","content":[{"type":"paragraph"}]}',
  plain_text   text NOT NULL DEFAULT '',
  state        text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'closed')),
  closed_at    timestamptz,
  closed_by    uuid REFERENCES app_user (id),
  reopen_count integer NOT NULL DEFAULT 0,
  version      integer NOT NULL DEFAULT 1,
  locked_by    uuid REFERENCES app_user (id),
  locked_at    timestamptz,
  updated_at   timestamptz NOT NULL DEFAULT now(),
  updated_by   uuid REFERENCES app_user (id),
  search       tsvector GENERATED ALWAYS AS (to_tsvector('english', plain_text)) STORED,
  UNIQUE (sheet_id, letter)
);
CREATE INDEX a3_step_search_idx ON a3_step USING gin (search);

CREATE TABLE a3_step_snapshot (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  step_id     uuid NOT NULL REFERENCES a3_step (id) ON DELETE CASCADE,
  sheet_id    uuid NOT NULL REFERENCES a3_sheet (id) ON DELETE CASCADE,
  letter      text NOT NULL,
  content     jsonb NOT NULL,
  plain_text  text NOT NULL,
  version     integer NOT NULL,
  author_id   uuid REFERENCES app_user (id),
  reason      text NOT NULL CHECK (reason IN ('accept', 'close', 'reopen', 'autosave', 'restore')),
  created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX a3_step_snapshot_step_idx ON a3_step_snapshot (step_id, created_at DESC);
CREATE INDEX a3_step_snapshot_sheet_idx ON a3_step_snapshot (sheet_id, created_at DESC);

CREATE TABLE attachment_blob (
  hash        text PRIMARY KEY CHECK (hash ~ '^[0-9a-f]{64}$'),
  size_bytes  bigint NOT NULL CHECK (size_bytes >= 0),
  complete    boolean NOT NULL DEFAULT false,
  created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE attachment_chunk (
  hash  text NOT NULL REFERENCES attachment_blob (hash) ON DELETE CASCADE,
  seq   integer NOT NULL,
  data  bytea NOT NULL,
  PRIMARY KEY (hash, seq)
);

CREATE TABLE attachment (
  id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  sheet_id         uuid NOT NULL REFERENCES a3_sheet (id) ON DELETE CASCADE,
  file_name        text NOT NULL,
  version          integer NOT NULL DEFAULT 1,
  blob_hash        text NOT NULL REFERENCES attachment_blob (hash),
  file_type        text NOT NULL,
  read_state       text NOT NULL DEFAULT 'pending'
                   CHECK (read_state IN ('pending', 'reading', 'read', 'unreadable', 'stored_only', 'needs_modern_format')),
  read_error       text,
  ai_state         text NOT NULL DEFAULT 'none'
                   CHECK (ai_state IN ('none', 'pending', 'running', 'done', 'failed')),
  ai_error         text,
  ai_note          text,
  summary          text,
  excluded_sheets  text[] NOT NULL DEFAULT '{}',
  confirms_count   integer NOT NULL DEFAULT 0,
  added_by         uuid NOT NULL REFERENCES app_user (id),
  added_at         timestamptz NOT NULL DEFAULT now(),
  deleted_at       timestamptz,
  deleted_by       uuid REFERENCES app_user (id),
  UNIQUE (sheet_id, file_name, version)
);
CREATE INDEX attachment_sheet_idx ON attachment (sheet_id);

CREATE TABLE source_segment (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  attachment_id  uuid NOT NULL REFERENCES attachment (id) ON DELETE CASCADE,
  seq            integer NOT NULL,
  label          text NOT NULL,
  location       jsonb NOT NULL DEFAULT '{}',
  text           text NOT NULL,
  from_image     boolean NOT NULL DEFAULT false,
  needs_image    boolean NOT NULL DEFAULT false,
  ai_visible     boolean NOT NULL DEFAULT true,
  group_name     text,
  search         tsvector GENERATED ALWAYS AS (to_tsvector('english', text)) STORED,
  UNIQUE (attachment_id, seq)
);
CREATE INDEX source_segment_search_idx ON source_segment USING gin (search);

CREATE TABLE ai_run (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  sheet_id        uuid REFERENCES a3_sheet (id) ON DELETE SET NULL,
  job             text NOT NULL,
  provider        text NOT NULL,
  model           text NOT NULL,
  user_id         uuid NOT NULL REFERENCES app_user (id),
  sources         jsonb NOT NULL DEFAULT '[]',
  outcome         text NOT NULL,
  input_tokens    integer NOT NULL DEFAULT 0,
  output_tokens   integer NOT NULL DEFAULT 0,
  discarded_count integer NOT NULL DEFAULT 0,
  discard_reasons jsonb NOT NULL DEFAULT '[]',
  created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE a3_fact (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  sheet_id           uuid NOT NULL REFERENCES a3_sheet (id) ON DELETE CASCADE,
  letter             text NOT NULL CHECK (letter IN ('A', 'B', 'C', 'D')),
  text               text NOT NULL,
  source_segment_id  uuid REFERENCES source_segment (id) ON DELETE SET NULL,
  state              text NOT NULL DEFAULT 'accepted' CHECK (state IN ('accepted', 'rejected', 'superseded')),
  created_by         uuid REFERENCES app_user (id),
  created_at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX a3_fact_sheet_idx ON a3_fact (sheet_id);

CREATE TABLE ai_proposal (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  sheet_id           uuid NOT NULL REFERENCES a3_sheet (id) ON DELETE CASCADE,
  attachment_id      uuid REFERENCES attachment (id) ON DELETE SET NULL,
  letter             text CHECK (letter IN ('A', 'B', 'C', 'D')),
  kind               text NOT NULL CHECK (kind IN ('add', 'update', 'conflict', 'metadata')),
  field              text,
  proposed_text      text NOT NULL,
  current_text       text,
  replaces_fact_id   uuid REFERENCES a3_fact (id) ON DELETE SET NULL,
  source_segment_id  uuid REFERENCES source_segment (id) ON DELETE SET NULL,
  source_label       text,
  quote              text,
  from_image         boolean NOT NULL DEFAULT false,
  why                text,
  norm_key           text NOT NULL,
  source_blob_hash   text,
  state              text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'accepted', 'rejected')),
  decided_by         uuid REFERENCES app_user (id),
  decided_at         timestamptz,
  run_id             uuid REFERENCES ai_run (id) ON DELETE SET NULL,
  created_at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ai_proposal_sheet_idx ON ai_proposal (sheet_id, state);

CREATE TABLE ai_finding (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  sheet_id           uuid NOT NULL REFERENCES a3_sheet (id) ON DELETE CASCADE,
  check_code         text NOT NULL CHECK (check_code ~ '^V([1-9]|10)$'),
  letter             text NOT NULL CHECK (letter IN ('A', 'B', 'C', 'D')),
  rank               integer NOT NULL,
  text               text NOT NULL,
  why                text,
  source_segment_id  uuid REFERENCES source_segment (id) ON DELETE SET NULL,
  source_label       text,
  quote              text,
  step_text_hash     text NOT NULL,
  state              text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'dismissed', 'resolved')),
  decided_by         uuid REFERENCES app_user (id),
  run_id             uuid REFERENCES ai_run (id) ON DELETE SET NULL,
  created_at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ai_finding_sheet_idx ON ai_finding (sheet_id, state);

CREATE TABLE ai_message (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  sheet_id    uuid NOT NULL REFERENCES a3_sheet (id) ON DELETE CASCADE,
  role        text NOT NULL CHECK (role IN ('user', 'assistant', 'summary')),
  text        text NOT NULL,
  sources     jsonb NOT NULL DEFAULT '[]',
  user_id     uuid REFERENCES app_user (id),
  created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ai_message_sheet_idx ON ai_message (sheet_id, created_at);

CREATE TABLE early_warning (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  sheet_id          uuid NOT NULL REFERENCES a3_sheet (id),
  headline          text NOT NULL CHECK (char_length(headline) BETWEEN 1 AND 80),
  what_happened     text NOT NULL,
  who_should_check  text[] NOT NULL DEFAULT '{}',
  quick_check       text NOT NULL DEFAULT '',
  severity          text NOT NULL CHECK (severity IN ('High', 'Medium', 'Low')),
  cause_status      text NOT NULL CHECK (cause_status IN ('Cause not confirmed', 'Root cause confirmed', 'Solution verified')),
  author_id         uuid NOT NULL REFERENCES app_user (id),
  posted_at         timestamptz NOT NULL DEFAULT now(),
  last_update_at    timestamptz NOT NULL DEFAULT now(),
  withdrawn_at      timestamptz,
  withdrawn_reason  text
);
CREATE INDEX early_warning_sheet_idx ON early_warning (sheet_id);
CREATE INDEX early_warning_recent_idx ON early_warning (last_update_at DESC);

CREATE TABLE early_warning_update (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  warning_id    uuid NOT NULL REFERENCES early_warning (id) ON DELETE CASCADE,
  text          text NOT NULL CHECK (btrim(text) <> ''),
  cause_status  text NOT NULL,
  author_id     uuid NOT NULL REFERENCES app_user (id),
  created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE warning_read_state (
  warning_id  uuid NOT NULL REFERENCES early_warning (id) ON DELETE CASCADE,
  user_id     uuid NOT NULL REFERENCES app_user (id),
  read_at     timestamptz,
  unread      boolean NOT NULL DEFAULT false,
  updated     boolean NOT NULL DEFAULT false,
  PRIMARY KEY (warning_id, user_id)
);

CREATE TABLE follow_up_task (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  warning_id   uuid NOT NULL REFERENCES early_warning (id) ON DELETE CASCADE,
  description  text NOT NULL CHECK (btrim(description) <> ''),
  assignee_id  uuid NOT NULL REFERENCES app_user (id),
  target_area  text NOT NULL DEFAULT '',
  due_on       date,
  result       text CHECK (result IN ('Not affected', 'Affected', 'Not applicable')),
  closed_at    timestamptz,
  linked_a3_id uuid REFERENCES a3_sheet (id) ON DELETE SET NULL,
  created_by   uuid NOT NULL REFERENCES app_user (id),
  created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX follow_up_task_assignee_idx ON follow_up_task (assignee_id) WHERE closed_at IS NULL;
CREATE INDEX follow_up_task_warning_idx ON follow_up_task (warning_id);

ALTER TABLE a3_sheet
  ADD FOREIGN KEY (origin_warning_id) REFERENCES early_warning (id) ON DELETE SET NULL,
  ADD FOREIGN KEY (origin_task_id) REFERENCES follow_up_task (id) ON DELETE SET NULL;

CREATE TABLE audit_event (
  id           bigserial PRIMARY KEY,
  actor_id     uuid REFERENCES app_user (id),
  object_type  text NOT NULL,
  object_id    uuid,
  sheet_id     uuid,
  action       text NOT NULL,
  reason       text,
  detail       jsonb NOT NULL DEFAULT '{}',
  at           timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_event_sheet_idx ON audit_event (sheet_id, at DESC);
CREATE INDEX audit_event_action_idx ON audit_event (action, at DESC);

INSERT INTO schema_meta (version) VALUES (1);
