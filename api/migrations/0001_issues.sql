-- Homeowner issue tickets (replaces the Survey123 "Report an Issue" layer).
-- Lives in schema `app` so the Martin tile server (which auto-publishes only
-- the `public` schema) never exposes reporter names/emails/addresses.
CREATE EXTENSION IF NOT EXISTS postgis;
CREATE SCHEMA IF NOT EXISTS app;

CREATE TABLE IF NOT EXISTS app.issues (
    id                 serial PRIMARY KEY,
    ticket             text NOT NULL UNIQUE,
    name               text,
    street_address     text,
    email              text,
    hoa_account_number text,
    issue_category     text NOT NULL,
    issue_type         text,
    describe_the_issue text,
    additional_notes   text,
    status             text NOT NULL DEFAULT 'New' CHECK (status IN ('New', 'In Progress', 'Closed')),
    assigned_to        text,
    staff_notes        text,
    geom               geometry(Point, 4326) NOT NULL,
    source             text NOT NULL DEFAULT 'web',
    legacy_globalid    text UNIQUE,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS issues_geom_idx ON app.issues USING gist (geom);
CREATE INDEX IF NOT EXISTS issues_status_idx ON app.issues (status);
CREATE INDEX IF NOT EXISTS issues_category_idx ON app.issues (issue_category);

CREATE TABLE IF NOT EXISTS app.issue_photos (
    id            serial PRIMARY KEY,
    issue_id      integer NOT NULL REFERENCES app.issues (id) ON DELETE CASCADE,
    storage       text NOT NULL CHECK (storage IN ('upload', 'seed')),
    rel_path      text NOT NULL,
    content_type  text,
    size          bigint,
    original_name text,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS issue_photos_issue_idx ON app.issue_photos (issue_id);

-- Default assignee per category (same table as the web map's Arcade "assignedto").
CREATE OR REPLACE FUNCTION app.assignee_for(category text) RETURNS text
LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE category
    WHEN 'Streets and pavement'       THEN 'Maintenance lead (placeholder)'
    WHEN 'Signs and striping'         THEN 'Maintenance lead (placeholder)'
    WHEN 'Drainage and storm drains'  THEN 'Maintenance lead (placeholder)'
    WHEN 'Lighting'                   THEN 'Maintenance lead (placeholder)'
    WHEN 'Irrigation and water leaks' THEN 'Landscape contractor (placeholder)'
    WHEN 'Landscaping and trees'      THEN 'Landscape contractor (placeholder)'
    WHEN 'Common areas and amenities' THEN 'Property manager (placeholder)'
    WHEN 'Neighbor and CC&R concerns' THEN 'Property manager (placeholder)'
    WHEN 'Safety hazards'             THEN 'Property manager and HOA director (placeholder)'
    WHEN 'Golf course boundary'       THEN 'Club contact (placeholder)'
    ELSE 'Property manager general POC and HOA director or delegate (placeholder)'
  END
$$;

-- Ticket number: CO-YYMMDD-NNNN (Pacific date, id zero-padded to 4).
CREATE OR REPLACE FUNCTION app.ticket_for(id integer, ts timestamptz) RETURNS text
LANGUAGE sql IMMUTABLE AS $$
  SELECT 'CO-' || to_char(ts AT TIME ZONE 'America/Los_Angeles', 'YYMMDD') || '-' ||
         CASE WHEN id < 10000 THEN lpad(id::text, 4, '0') ELSE id::text END
$$;

-- Idempotent import of the Survey123 public-view reports (public.issue_reports,
-- loaded by scripts/seed.sh). Keeps their original objectid-based ticket numbers.
CREATE OR REPLACE FUNCTION app.import_legacy_issues() RETURNS integer
LANGUAGE plpgsql AS $$
DECLARE n integer := 0;
BEGIN
  IF to_regclass('public.issue_reports') IS NULL THEN RETURN 0; END IF;
  WITH ins AS (
    INSERT INTO app.issues (id, ticket, issue_category, issue_type, describe_the_issue, additional_notes,
                            status, assigned_to, geom, source, legacy_globalid, created_at, updated_at)
    SELECT r.objectid,
           app.ticket_for(r.objectid, r.creationdate),
           r.issue_category,
           NULLIF(COALESCE(r.issue_type, r.field_7, r.field_8, r.field_9, r.field_10, r.field_11,
                           r.field_12, r.field_13, r.field_14, r.field_15), ''),
           r.describe_the_issue, r.additional_notes,
           CASE WHEN r.status IN ('New', 'In Progress', 'Closed') THEN r.status ELSE 'New' END,
           COALESCE(NULLIF(r.assigned_to, ''), app.assignee_for(r.issue_category)),
           ST_Force2D(r.geom), 'survey123', r.globalid::text,
           COALESCE(r.creationdate, now()), COALESCE(r.editdate, r.creationdate, now())
    FROM public.issue_reports r
    WHERE r.geom IS NOT NULL
    ON CONFLICT DO NOTHING
    RETURNING id, legacy_globalid
  )
  SELECT count(*) INTO n FROM ins;

  IF to_regclass('public.attachments') IS NOT NULL THEN
    INSERT INTO app.issue_photos (issue_id, storage, rel_path, content_type, size, original_name)
    SELECT i.id, 'seed', a.file_path, a.content_type, a.size, a.name
    FROM public.attachments a
    JOIN app.issues i ON i.id = a.objectid AND i.source = 'survey123'
    WHERE a.table_name = 'issue_reports'
      AND NOT EXISTS (SELECT 1 FROM app.issue_photos p WHERE p.issue_id = i.id AND p.rel_path = a.file_path);
  END IF;

  PERFORM setval(pg_get_serial_sequence('app.issues', 'id'),
                 GREATEST((SELECT COALESCE(max(id), 0) FROM app.issues), 1),
                 (SELECT count(*) > 0 FROM app.issues));
  RETURN n;
END $$;
