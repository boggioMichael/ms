-- migrations/0001_research_catalog.sql — the data program's research catalog (P2)
--
-- DESIGNED FOR P2, NOT YET RUN AGAINST A DATABASE of the program. No P2 database exists and nothing
-- is deployed. (Syntax check: see the note at the end of this header.)
--
-- Design:    docs/data-program/DATA_ARCHITECTURE.md §5–§9 (pipeline, layers, zones, storage, deletion).
-- Names:     the owner's directive §7 (the envelope) and src/research/contracts.rs + consent.rs
--            (schema 0.1.0, w46), as read on 2026-10-10. Reconcile with DATA_CONTRACTS.md.
-- Metrics:   docs/data-program/METRICS_CATALOG.md F12 reads these tables (read-only, via DuckDB).
--
-- Rules this schema keeps:
--   * Unknown is NULL. No measured column has a DEFAULT 0, and none is NOT NULL unless it is
--     always known.
--   * Consent receipts, withdrawals, labels and events are append-only. A change is a new row that
--     supersedes; UPDATE is refused by trigger. Deletion (deletion.*) is the only way rows leave,
--     and retention drops whole partitions.
--   * No personal field in research.* or delivery.*: no name, e-mail, account, address, device id or
--     payment detail. The identity zone (accounts, contact, incentive payments, and the only
--     account <-> research_subject_id link) is a SEPARATE DATABASE and is not in this migration.
--   * consent.* has NO foreign key from research.* or delivery.*, so that it can move to its own
--     database without a rewrite. The ingest role checks consent only through the narrow function
--     consent.receipt_allows(), never by reading the tables.
--   * Events are partitioned by game, then by collection date. One file or table per user is never
--     used.
--   * Customer identifiers are per-customer HMACs made at export time. The catalog keeps only a
--     reference to each customer's key (in a key service), never the key itself.
--
-- Syntax check, and nothing more: on 2026-10-10 (w45) this file was applied once to an EMPTY
-- throwaway PostgreSQL 16 cluster (/var/lib/postgresql/w45-ddl-check, socket only, no TCP). The
-- cluster was deleted afterwards. A smoke script with synthetic rows, rolled back at its end, checked:
--   * monthly and default partition routing;
--   * UPDATE refused on events and on consent receipts;
--   * a model inference refused as gold;
--   * a payload tag that differs from event_type refused;
--   * an id that is not a token refused;
--   * a quarantine past 30 days refused;
--   * consent.receipt_allows() true before a withdrawal and false after;
--   * no rights grants for MapleStory or MapleStory Worlds;
--   * deleting a subject leaves no rows.
-- This shows that PostgreSQL 16 accepts the DDL and that those constraints fire. It says nothing
-- about performance, operations or fitness for P2.

BEGIN;

-- ---------------------------------------------------------------------------------------------
-- Schemas and group roles (NOLOGIN: a deployment maps its service accounts onto them)

CREATE SCHEMA IF NOT EXISTS vocab;      -- closed vocabularies, copyable to another database
CREATE SCHEMA IF NOT EXISTS consent;    -- receipts, texts, tallies: the consent zone
CREATE SCHEMA IF NOT EXISTS rights;     -- rights manifests per title and deliverable
CREATE SCHEMA IF NOT EXISTS research;   -- layers 1 and 2: sessions, events, episodes, labels, media
CREATE SCHEMA IF NOT EXISTS delivery;   -- layer 3: datasets, artifacts, exports, lineage, deletion index
CREATE SCHEMA IF NOT EXISTS deletion;   -- deletion requests and their tasks
CREATE SCHEMA IF NOT EXISTS audit;      -- access log (no payloads, no secrets)

DO $$
DECLARE r text;
BEGIN
    FOREACH r IN ARRAY ARRAY['syrup_consent', 'syrup_ingest', 'syrup_analyst', 'syrup_export',
                             'syrup_deletion', 'syrup_rights_reviewer']
    LOOP
        IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = r) THEN
            EXECUTE format('CREATE ROLE %I NOLOGIN', r);
        END IF;
    END LOOP;
END $$;

REVOKE ALL ON SCHEMA consent, rights, research, delivery, deletion, audit FROM PUBLIC;

-- ---------------------------------------------------------------------------------------------
-- Vocabularies (text domains rather than enums, so that a value can be added without a type rewrite)

CREATE DOMAIN vocab.token AS text
    CHECK (VALUE ~ '^[A-Za-z0-9._:+/-]{1,96}$' AND position('//' IN VALUE) = 0);   -- contracts.rs is_token
CREATE DOMAIN vocab.purpose AS text
    CHECK (VALUE IN ('service_operation', 'improve_syrup', 'aggregate_analytics',
                     'external_research_training', 'media_donation'));
CREATE DOMAIN vocab.data_type AS text
    CHECK (VALUE IN ('session_metadata', 'gameplay_state', 'goals', 'help_requests', 'assistant_advice',
                     'player_actions', 'corrections', 'feedback', 'outcomes', 'experiment_assignments',
                     'capture_quality', 'media', 'voice'));
CREATE DOMAIN vocab.recipient_class AS text
    CHECK (VALUE IN ('this_device', 'syrup_team', 'external_researcher', 'licensed_buyer'));
CREATE DOMAIN vocab.game_id AS text
    CHECK (VALUE IN ('maplestory', 'maplestory_worlds', 'synthetic', 'other', 'unknown'));
CREATE DOMAIN vocab.event_type AS text
    CHECK (VALUE IN ('session', 'observation', 'task', 'help', 'assistant', 'player_action',
                     'correction', 'feedback', 'outcome', 'experiment', 'capture_quality'));
CREATE DOMAIN vocab.coverage AS text
    CHECK (VALUE IN ('full', 'partial', 'not_visible', 'unknown'));
CREATE DOMAIN vocab.source_type AS text
    CHECK (VALUE IN ('publisher_ground_truth', 'direct_observation', 'human_asserted', 'human_reviewed',
                     'model_inferred', 'synthetic', 'unknown'));
CREATE DOMAIN vocab.annotator_type AS text
    CHECK (VALUE IN ('publisher', 'detector', 'player', 'human_reviewer', 'model', 'rule', 'generator',
                     'unknown'));
CREATE DOMAIN vocab.verification_status AS text
    CHECK (VALUE IN ('unverified', 'reviewer_verified', 'gold', 'rejected'));
CREATE DOMAIN vocab.outcome_kind AS text
    CHECK (VALUE IN ('success', 'failure', 'aborted', 'unobserved', 'censored'));
CREATE DOMAIN vocab.sampling_policy AS text
    CHECK (VALUE IN ('census', 'base_random', 'event_triggered'));      -- proposed closed vocabulary
CREATE DOMAIN vocab.probability AS double precision
    CHECK (VALUE > 0 AND VALUE <= 1);
CREATE DOMAIN vocab.rights_status AS text
    CHECK (VALUE IN ('approved', 'requires_title_specific_review', 'denied'));
CREATE DOMAIN vocab.rights_use AS text
    CHECK (VALUE IN ('capture', 'store', 'annotate', 'train_internal', 'train_external', 'evaluate',
                     'transfer'));
CREATE DOMAIN vocab.deliverable AS text
    CHECK (VALUE IN ('event_metadata', 'media'));

-- Refuse UPDATE on append-only tables (a change is a new, superseding row).
CREATE FUNCTION vocab.forbid_update() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '%.% is append-only: insert a superseding row instead of updating',
        TG_TABLE_SCHEMA, TG_TABLE_NAME;
END $$;

-- ---------------------------------------------------------------------------------------------
-- consent: the consent zone (in P2, proposed to be its own database with its own credentials)

CREATE TABLE consent.consent_texts (
    text_id         vocab.token NOT NULL,
    version         vocab.token NOT NULL,
    language        text        NOT NULL CHECK (language ~ '^[a-z]{2,3}$'),
    sha256          text        NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),   -- of the exact text shown
    body_ref        text,                      -- where the full text is archived (never edited)
    counsel_review  text,                      -- reference of the legal review, NULL while a draft
    published_at    timestamptz NOT NULL,
    retired_at      timestamptz,
    PRIMARY KEY (text_id, version, language)
);

-- One receipt per grant (directive §10: text version, time, source, scope; with epoch).
CREATE TABLE consent.consent_receipts (
    receipt_id           vocab.token NOT NULL PRIMARY KEY,
    research_subject_id  vocab.token NOT NULL,
    text_id              vocab.token NOT NULL,
    text_version         vocab.token NOT NULL,
    text_language        text        NOT NULL,
    text_sha256          text        NOT NULL CHECK (text_sha256 ~ '^[0-9a-f]{64}$'),
    given_at             timestamptz NOT NULL,
    source               text        NOT NULL,     -- UI surface and client version that showed the text
    epoch                integer     NOT NULL CHECK (epoch >= 1),
    age_assurance        text        NOT NULL CHECK (age_assurance IN ('self_declared_adult', 'not_assured')),
    recorded_at          timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (text_id, text_version, text_language)
        REFERENCES consent.consent_texts (text_id, version, language),
    UNIQUE (research_subject_id, epoch)
);
CREATE TRIGGER consent_receipts_append_only BEFORE UPDATE ON consent.consent_receipts
    FOR EACH ROW EXECUTE FUNCTION vocab.forbid_update();

-- The scope: purposes x data types x recipient classes (never one boolean).
CREATE TABLE consent.receipt_scope (
    receipt_id       vocab.token           NOT NULL REFERENCES consent.consent_receipts,
    purpose          vocab.purpose         NOT NULL,
    data_type        vocab.data_type       NOT NULL,
    recipient_class  vocab.recipient_class NOT NULL,
    PRIMARY KEY (receipt_id, purpose, data_type, recipient_class)
);
CREATE TRIGGER receipt_scope_append_only BEFORE UPDATE ON consent.receipt_scope
    FOR EACH ROW EXECUTE FUNCTION vocab.forbid_update();

CREATE TABLE consent.withdrawals (
    receipt_id           vocab.token NOT NULL PRIMARY KEY REFERENCES consent.consent_receipts,
    research_subject_id  vocab.token NOT NULL,
    withdrawn_at         timestamptz NOT NULL,
    epoch                integer     NOT NULL CHECK (epoch >= 1),
    source               text        NOT NULL
);
CREATE TRIGGER withdrawals_append_only BEFORE UPDATE ON consent.withdrawals
    FOR EACH ROW EXECUTE FUNCTION vocab.forbid_update();

-- Offer tallies: counts only, no subject id (for the consent rate, METRICS_CATALOG M12.1).
CREATE TABLE consent.offer_tallies (
    text_id        vocab.token   NOT NULL,
    text_version   vocab.token   NOT NULL,
    text_language  text          NOT NULL,
    purpose        vocab.purpose NOT NULL,
    day            date          NOT NULL,
    shown          integer       NOT NULL CHECK (shown >= 0),
    granted        integer       NOT NULL CHECK (granted >= 0),
    declined       integer       NOT NULL CHECK (declined >= 0),
    CHECK (granted + declined <= shown),
    PRIMARY KEY (text_id, text_version, text_language, purpose, day),
    FOREIGN KEY (text_id, text_version, text_language)
        REFERENCES consent.consent_texts (text_id, version, language)
);

-- What is in force: the latest epoch of each subject, if not withdrawn.
CREATE VIEW consent.current_grants AS
SELECT r.research_subject_id, r.receipt_id, r.epoch, s.purpose, s.data_type, s.recipient_class
FROM consent.consent_receipts r
JOIN consent.receipt_scope s USING (receipt_id)
WHERE NOT EXISTS (SELECT 1 FROM consent.withdrawals w WHERE w.receipt_id = r.receipt_id)
  AND r.epoch = (SELECT max(r2.epoch) FROM consent.consent_receipts r2
                 WHERE r2.research_subject_id = r.research_subject_id);

-- The narrow interface the ingest and export services use (DATA_ARCHITECTURE.md §5 stage 2, §6).
CREATE FUNCTION consent.receipt_allows(p_receipt_id text, p_epoch integer, p_purpose text,
                                       p_data_type text, p_recipient_class text)
RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path = consent, pg_temp AS $$
    SELECT EXISTS (
        SELECT 1 FROM consent.current_grants g
        WHERE g.receipt_id = p_receipt_id AND g.epoch = p_epoch AND g.purpose = p_purpose
          AND g.data_type = p_data_type AND g.recipient_class = p_recipient_class)
$$;

-- ---------------------------------------------------------------------------------------------
-- rights: one manifest per title and deliverable (directive §10; consent.rs RightsManifest)

CREATE TABLE rights.rights_policies (
    rights_policy_id  vocab.token         NOT NULL PRIMARY KEY,
    game_id           vocab.game_id       NOT NULL,
    deliverable       vocab.deliverable   NOT NULL,
    rights_holder     text                NOT NULL,
    status            vocab.rights_status NOT NULL,
    basis             text,               -- evidence of permission or of an approved analysis
    applies_to        text[]              NOT NULL DEFAULT '{}',   -- builds, worlds, variants covered
    territory         text[]              NOT NULL DEFAULT '{}',   -- ISO codes; empty = none granted
    valid_from        timestamptz         NOT NULL,
    valid_until       timestamptz         NOT NULL,
    revoked_at        timestamptz,
    reviewed_by_role  text,
    reviewed_at       timestamptz,
    notes             text                NOT NULL DEFAULT '',
    CHECK (valid_until > valid_from),
    CHECK (status <> 'approved' OR (basis IS NOT NULL AND reviewed_by_role IS NOT NULL))
);

-- What an approved policy grants: per purpose, recipient class and use. No row = not granted.
CREATE TABLE rights.rights_grants (
    rights_policy_id  vocab.token           NOT NULL REFERENCES rights.rights_policies,
    purpose           vocab.purpose         NOT NULL,
    recipient_class   vocab.recipient_class NOT NULL,
    use               vocab.rights_use      NOT NULL,
    PRIMARY KEY (rights_policy_id, purpose, recipient_class, use)
);

-- ---------------------------------------------------------------------------------------------
-- research: layers 1 and 2

CREATE TABLE research.games (
    game_id    vocab.game_id NOT NULL PRIMARY KEY,
    title      text          NOT NULL,
    publisher  text,
    notes      text          NOT NULL DEFAULT ''
);

CREATE TABLE research.research_subjects (
    research_subject_id  vocab.token NOT NULL PRIMARY KEY,   -- a pseudonym; the identity link lives elsewhere
    enrolled_on          date        NOT NULL,               -- a day, not a time
    status               text        NOT NULL DEFAULT 'active'
                         CHECK (status IN ('active', 'withdrawn', 'deleting')),   -- deleted = row gone
    status_changed_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE research.sessions (
    session_id           vocab.token     NOT NULL PRIMARY KEY,
    research_subject_id  vocab.token     NOT NULL REFERENCES research.research_subjects ON DELETE CASCADE,
    game_id              vocab.game_id   NOT NULL REFERENCES research.games,
    game_variant         text,
    world_id             text,
    game_build           text,
    platform             text,
    locale               text,
    client_version       text            NOT NULL,
    collection_date      date            NOT NULL,           -- partition key of the session's events
    first_ingested_at    timestamptz     NOT NULL,
    observed_ms          bigint          CHECK (observed_ms >= 0),   -- in-view time; NULL = unknown
    consent_receipt_id   vocab.token     NOT NULL,           -- no FK: consent is its own zone
    consent_epoch        integer         NOT NULL,
    collection_purpose   vocab.purpose   NOT NULL,
    rights_policy_id     vocab.token     NOT NULL REFERENCES rights.rights_policies
);
CREATE INDEX ON research.sessions (research_subject_id);

-- Each upload batch, once (idempotency: a retried batch gets its stored result back).
CREATE TABLE research.ingest_batches (
    batch_id             vocab.token NOT NULL PRIMARY KEY,
    research_subject_id  vocab.token,                        -- NULL once the subject is deleted
    received_at          timestamptz NOT NULL DEFAULT now(),
    client_sent_at       timestamptz,
    clock_skew_flag      boolean,                            -- |sent - received| beyond the threshold
    schema_version       text        NOT NULL,
    events_received      integer     NOT NULL CHECK (events_received >= 0),
    events_accepted      integer     NOT NULL CHECK (events_accepted >= 0),
    events_duplicate     integer     NOT NULL CHECK (events_duplicate >= 0),
    events_quarantined   integer     NOT NULL CHECK (events_quarantined >= 0),
    events_rejected      integer     NOT NULL CHECK (events_rejected >= 0),
    CHECK (events_accepted + events_duplicate + events_quarantined + events_rejected = events_received),
    result               jsonb       NOT NULL                -- per-event statuses returned to the client
);

-- The event-id ledger: global uniqueness that a partitioned table cannot enforce by itself.
CREATE TABLE research.event_ids (
    event_id         vocab.token   NOT NULL PRIMARY KEY,
    game_id          vocab.game_id NOT NULL,
    collection_date  date          NOT NULL,
    session_id       vocab.token   NOT NULL,
    batch_id         vocab.token   NOT NULL REFERENCES research.ingest_batches,
    accepted_at      timestamptz   NOT NULL DEFAULT now()
);
CREATE INDEX ON research.event_ids (session_id);

-- Events: the envelope of directive §7 plus the family payload. Partitioned by game, then by date.
CREATE TABLE research.events (
    event_id              vocab.token          NOT NULL,
    schema_version        text                 NOT NULL,
    session_id            vocab.token          NOT NULL,
    episode_id            vocab.token,
    research_subject_id   vocab.token          NOT NULL,
    event_type            vocab.event_type     NOT NULL,
    game_id               vocab.game_id        NOT NULL,
    game_variant          text,
    world_id              text,
    game_build            text,
    platform              text,
    locale                text,
    client_version        text                 NOT NULL,
    detector_version      text,
    model_version         text,
    sequence_no           bigint               NOT NULL CHECK (sequence_no >= 0),
    monotonic_timestamp   bigint               NOT NULL CHECK (monotonic_timestamp >= 0),   -- ms in session
    ingested_at           timestamptz          NOT NULL,
    observation_coverage  vocab.coverage       NOT NULL,
    consent_receipt_id    vocab.token          NOT NULL,
    consent_epoch         integer              NOT NULL,
    collection_purpose    vocab.purpose        NOT NULL,
    rights_policy_id      vocab.token          NOT NULL,
    sampling_policy       vocab.sampling_policy NOT NULL,
    sampling_probability  vocab.probability,                -- NULL = not known (never 1 by default)
    payload               jsonb                NOT NULL,     -- {"<family>": {…}} as in contracts.rs
    collection_date       date                 NOT NULL,     -- from the session (DATA_ARCHITECTURE §8)
    batch_id              vocab.token          NOT NULL,
    received_at           timestamptz          NOT NULL DEFAULT now(),
    PRIMARY KEY (game_id, collection_date, event_id),
    CHECK (payload ? event_type)                             -- the payload's tag is the event type
) PARTITION BY LIST (game_id);

CREATE INDEX ON research.events (session_id, sequence_no);
CREATE INDEX ON research.events (research_subject_id);
CREATE INDEX ON research.events (episode_id) WHERE episode_id IS NOT NULL;

CREATE TABLE research.events_maplestory PARTITION OF research.events
    FOR VALUES IN ('maplestory') PARTITION BY RANGE (collection_date);
CREATE TABLE research.events_maplestory_worlds PARTITION OF research.events
    FOR VALUES IN ('maplestory_worlds') PARTITION BY RANGE (collection_date);
CREATE TABLE research.events_synthetic PARTITION OF research.events
    FOR VALUES IN ('synthetic') PARTITION BY RANGE (collection_date);
CREATE TABLE research.events_other PARTITION OF research.events DEFAULT;   -- 'other'; 'unknown' never passes the gate

-- A safety net per game. The job below creates monthly partitions AHEAD of time: a month's partition
-- cannot be attached once its rows sit in the default.
CREATE TABLE research.events_maplestory_default PARTITION OF research.events_maplestory DEFAULT;
CREATE TABLE research.events_maplestory_worlds_default PARTITION OF research.events_maplestory_worlds DEFAULT;
CREATE TABLE research.events_synthetic_default PARTITION OF research.events_synthetic DEFAULT;

-- Creates <game partition>_<yyyy_mm> for the month that holds p_month (run monthly, ahead).
CREATE FUNCTION research.ensure_month_partition(p_game text, p_month date) RETURNS text
LANGUAGE plpgsql AS $$
DECLARE
    parent text := 'events_' || p_game;
    child  text := parent || '_' || to_char(p_month, 'YYYY_MM');
    lo     date := date_trunc('month', p_month)::date;
    hi     date := (date_trunc('month', p_month) + interval '1 month')::date;
BEGIN
    IF p_game NOT IN ('maplestory', 'maplestory_worlds', 'synthetic') THEN
        RAISE EXCEPTION 'no month partitions for game %', p_game;
    END IF;
    EXECUTE format('CREATE TABLE IF NOT EXISTS research.%I PARTITION OF research.%I '
                   'FOR VALUES FROM (%L) TO (%L)', child, parent, lo, hi);
    RETURN child;
END $$;

CREATE TRIGGER events_append_only BEFORE UPDATE ON research.events
    FOR EACH ROW EXECUTE FUNCTION vocab.forbid_update();

-- Episodes (layer 2): the builder's record, plus the columns the catalog filters on.
CREATE TABLE research.episodes (
    episode_id           vocab.token        NOT NULL PRIMARY KEY,
    research_subject_id  vocab.token        NOT NULL REFERENCES research.research_subjects ON DELETE CASCADE,
    schema_version       text               NOT NULL,
    builder_version      text               NOT NULL,
    game_id              vocab.game_id      NOT NULL REFERENCES research.games,
    game_variant         text,
    game_build           text,
    collection_date      date               NOT NULL,
    goal_kind            text,
    goal_origin          text               CHECK (goal_origin IN ('explicit', 'inferred')),
    constraints          text[]             NOT NULL DEFAULT '{}',
    outcome              vocab.outcome_kind NOT NULL,
    outcome_determined_by text              NOT NULL,      -- a source_type, or 'rule' (the builder's)
    observable_until_ms  bigint,
    started_ms           bigint,
    ended_ms             bigint,
    observed_ms          bigint             CHECK (observed_ms >= 0),
    assisted             boolean,                          -- NULL = not known whether advice was shown
    qa_status            text               NOT NULL DEFAULT 'pending'
                         CHECK (qa_status IN ('pending', 'accepted', 'quarantined', 'rejected')),
    qa_checked_at        timestamptz,
    record               jsonb              NOT NULL,       -- the full Episode (episode.rs), sanitized
    built_at             timestamptz        NOT NULL DEFAULT now()
);
CREATE INDEX ON research.episodes (research_subject_id);
CREATE INDEX ON research.episodes (game_id, game_build, collection_date);

CREATE TABLE research.episode_sessions (
    episode_id  vocab.token NOT NULL REFERENCES research.episodes ON DELETE CASCADE,
    session_id  vocab.token NOT NULL REFERENCES research.sessions ON DELETE CASCADE,
    PRIMARY KEY (episode_id, session_id)
);

-- Labels: automatic, user correction and reviewed gold, kept apart (directive §11). Never updated:
-- a later label supersedes an earlier one, and both are kept with their lineage.
CREATE TABLE research.labels (
    label_id             vocab.token              NOT NULL PRIMARY KEY,
    research_subject_id  vocab.token              NOT NULL REFERENCES research.research_subjects ON DELETE CASCADE,
    target_event_id      vocab.token,
    target_episode_id    vocab.token              REFERENCES research.episodes ON DELETE CASCADE,
    label_source         text                     NOT NULL
                         CHECK (label_source IN ('automatic', 'user_correction', 'reviewed_gold')),
    value                jsonb,                   -- NULL = the labeller could not tell
    annotator_type       vocab.annotator_type     NOT NULL,
    source_type          vocab.source_type        NOT NULL,
    verification_status  vocab.verification_status NOT NULL,
    producer_version     text                     NOT NULL,
    split                text                     CHECK (split IN ('train', 'validation', 'test')),
    supersedes_label_id  vocab.token              REFERENCES research.labels,
    created_at           timestamptz              NOT NULL DEFAULT now(),
    CHECK (target_event_id IS NOT NULL OR target_episode_id IS NOT NULL),
    -- Only a reviewer's or the publisher's label may be gold (contracts.rs Provenance::check).
    CHECK (verification_status NOT IN ('reviewer_verified', 'gold')
           OR source_type IN ('human_reviewed', 'publisher_ground_truth')),
    CHECK (label_source <> 'reviewed_gold' OR verification_status = 'gold')
);
CREATE INDEX ON research.labels (target_event_id);
CREATE TRIGGER labels_append_only BEFORE UPDATE ON research.labels
    FOR EACH ROW EXECUTE FUNCTION vocab.forbid_update();

-- Quarantine: bounded in time (it is never a way to keep data).
CREATE TABLE research.quarantine (
    item_id         vocab.token NOT NULL PRIMARY KEY,
    batch_id        vocab.token REFERENCES research.ingest_batches,
    reason          text        NOT NULL,
    item            jsonb       NOT NULL,         -- the sanitized event as received
    quarantined_at  timestamptz NOT NULL DEFAULT now(),
    expires_at      timestamptz NOT NULL,
    resolved_at     timestamptz,
    resolution      text        CHECK (resolution IN ('released', 'deleted')),
    CHECK (expires_at <= quarantined_at + interval '30 days')
);

-- Media objects (the separate, approved media path only; DATA_ARCHITECTURE.md §3.3).
CREATE TABLE research.media_objects (
    media_id               vocab.token   NOT NULL PRIMARY KEY,
    research_subject_id    vocab.token   NOT NULL REFERENCES research.research_subjects ON DELETE CASCADE,
    session_id             vocab.token   NOT NULL REFERENCES research.sessions ON DELETE CASCADE,
    episode_id             vocab.token   REFERENCES research.episodes ON DELETE SET NULL,
    kind                   text          NOT NULL CHECK (kind IN ('clip', 'frame')),  -- no audio in the MVP
    object_key             text          NOT NULL UNIQUE,
    sha256                 text          NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    bytes                  bigint        NOT NULL CHECK (bytes > 0),
    duration_ms            bigint,
    width                  integer,
    height                 integer,
    fps                    real,
    codec                  text,
    encryption_key_ref     text          NOT NULL,           -- a wrapped data key's reference, never a key
    sanitization_status    text          NOT NULL CHECK (sanitization_status IN ('pending', 'verified_clean', 'rejected')),
    inclusion_reason       text          NOT NULL CHECK (inclusion_reason IN ('event_triggered', 'base_random')),
    inclusion_probability  vocab.probability,
    consent_receipt_id     vocab.token   NOT NULL,
    consent_epoch          integer       NOT NULL,
    rights_policy_id       vocab.token   NOT NULL REFERENCES rights.rights_policies,
    review_until           timestamptz,                      -- the participant's review window
    retain_until           timestamptz   NOT NULL,
    created_at             timestamptz   NOT NULL DEFAULT now(),
    CHECK (retain_until <= created_at + interval '30 days')   -- proposal for discussion, not a legal duty
);

-- ---------------------------------------------------------------------------------------------
-- delivery: layer 3, lineage and the deletion index

CREATE TABLE delivery.customers (
    customer_id        vocab.token NOT NULL PRIMARY KEY,
    display_name       text        NOT NULL,
    pseudonym_key_ref  text        NOT NULL,   -- the customer's HMAC key, in a key service
    contract_ref       text,
    created_at         timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE delivery.datasets (
    dataset_id         vocab.token           NOT NULL PRIMARY KEY,
    name               text                  NOT NULL,
    version            text                  NOT NULL,
    layer              text                  NOT NULL CHECK (layer IN ('L1', 'L2', 'L3')),
    purpose            vocab.purpose         NOT NULL,
    recipient_class    vocab.recipient_class NOT NULL,
    schema_version     text                  NOT NULL,
    builder_version    text                  NOT NULL,
    as_of              timestamptz           NOT NULL,
    status             text                  NOT NULL DEFAULT 'building'
                       CHECK (status IN ('building', 'ready', 'withdrawn')),
    approved_episodes  integer,
    approved_hours     double precision,
    built_at           timestamptz           NOT NULL DEFAULT now(),
    UNIQUE (name, version)
);

CREATE TABLE delivery.artifacts (
    artifact_id              vocab.token NOT NULL PRIMARY KEY,
    dataset_id               vocab.token REFERENCES delivery.datasets,
    artifact_kind            text        NOT NULL CHECK (artifact_kind IN
                             ('parquet', 'jsonl', 'media_bundle', 'manifest', 'data_card', 'report')),
    object_key               text        NOT NULL UNIQUE,
    sha256                   text        NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    bytes                    bigint      NOT NULL CHECK (bytes >= 0),
    rows                     bigint,
    game_id                  vocab.game_id,
    game_build               text,
    collection_date          date,
    replaced_by_artifact_id  vocab.token REFERENCES delivery.artifacts,   -- set by a deletion rewrite
    created_at               timestamptz NOT NULL DEFAULT now()
);

-- The deletion index: which artifacts hold which subject's rows (maintained by every writer).
CREATE TABLE delivery.subject_presence (
    research_subject_id  vocab.token NOT NULL,
    artifact_id          vocab.token NOT NULL REFERENCES delivery.artifacts,
    rows                 bigint      NOT NULL CHECK (rows > 0),
    PRIMARY KEY (research_subject_id, artifact_id)
);
CREATE INDEX ON delivery.subject_presence (artifact_id);

-- Lineage: a plain edge table (no graph database). Kinds: event, episode, label, media, dataset,
-- artifact, export, training.
CREATE TABLE delivery.lineage_edges (
    parent_kind      text        NOT NULL,
    parent_id        vocab.token NOT NULL,
    child_kind       text        NOT NULL,
    child_id         vocab.token NOT NULL,
    process          text        NOT NULL,
    process_version  text        NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (parent_kind, parent_id, child_kind, child_id)
);
CREATE INDEX ON delivery.lineage_edges (child_kind, child_id);

CREATE TABLE delivery.exports (
    export_id              vocab.token           NOT NULL PRIMARY KEY,
    customer_id            vocab.token           NOT NULL REFERENCES delivery.customers,
    dataset_id             vocab.token           NOT NULL REFERENCES delivery.datasets,
    purpose                vocab.purpose         NOT NULL,
    recipient_class        vocab.recipient_class NOT NULL,
    rights_policy_ids      text[]                NOT NULL,
    consent_checked_at     timestamptz           NOT NULL,   -- the gate ran at export time
    manifest_sha256        text                  NOT NULL CHECK (manifest_sha256 ~ '^[0-9a-f]{64}$'),
    signature_ref          text,                             -- detached signature, export-only key
    data_card_artifact_id  vocab.token           REFERENCES delivery.artifacts,
    contract_ref           text                  NOT NULL,
    delivered_at           timestamptz,
    url_expires_at         timestamptz,
    retention_until        timestamptz,
    revoked_at             timestamptz,
    status                 text                  NOT NULL DEFAULT 'prepared'
                           CHECK (status IN ('prepared', 'delivered', 'revoked', 'deletion_notified')),
    CHECK (url_expires_at IS NULL OR delivered_at IS NULL OR url_expires_at <= delivered_at + interval '24 hours')
);

-- Which training runs consumed which snapshots (directive §10: no promise of unlearning).
CREATE TABLE delivery.training_consumption (
    training_ref  text        NOT NULL,
    dataset_id    vocab.token NOT NULL REFERENCES delivery.datasets,
    export_id     vocab.token REFERENCES delivery.exports,
    consumer      text        NOT NULL CHECK (consumer IN ('internal', 'customer')),
    consumed_at   timestamptz NOT NULL,
    notes         text        NOT NULL DEFAULT '',
    PRIMARY KEY (training_ref, dataset_id)
);

-- ---------------------------------------------------------------------------------------------
-- deletion: requests and the tasks that prove each target was reached

CREATE TABLE deletion.deletion_requests (
    request_id           vocab.token NOT NULL PRIMARY KEY,
    research_subject_id  vocab.token NOT NULL,     -- kept (pseudonymous) as proof of handling
    requested_at         timestamptz NOT NULL,
    source               text        NOT NULL CHECK (source IN ('participant', 'withdrawal', 'retention', 'operator')),
    scope                text        NOT NULL DEFAULT 'all',   -- 'all', or one purpose
    status               text        NOT NULL DEFAULT 'open'
                         CHECK (status IN ('open', 'in_progress', 'completed', 'failed')),
    completed_at         timestamptz,
    CHECK ((status = 'completed') = (completed_at IS NOT NULL))
);

CREATE TABLE deletion.deletion_tasks (
    task_id       vocab.token NOT NULL PRIMARY KEY,
    request_id    vocab.token NOT NULL REFERENCES deletion.deletion_requests,
    target_kind   text        NOT NULL CHECK (target_kind IN
                  ('hot_events', 'event_ids', 'sessions', 'episodes', 'labels', 'media', 'parquet_file',
                   'derived', 'customer_notice', 'backup_cycle', 'training_record', 'identity_link')),
    target_ref    text        NOT NULL,           -- an id or object key, never content
    status        text        NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'done', 'failed')),
    done_at       timestamptz,
    proof_sha256  text        CHECK (proof_sha256 ~ '^[0-9a-f]{64}$'),   -- e.g. the rewritten file's checksum
    notes         text        NOT NULL DEFAULT '',
    CHECK ((status = 'done') = (done_at IS NOT NULL))
);

-- ---------------------------------------------------------------------------------------------
-- audit: who touched what, for which purpose (no payloads, no keys)

CREATE TABLE audit.access_log (
    at           timestamptz NOT NULL DEFAULT now(),
    actor_role   text        NOT NULL,
    action       text        NOT NULL,
    object_kind  text        NOT NULL,
    object_id    text,
    purpose      vocab.purpose
);
CREATE INDEX ON audit.access_log (at);

-- ---------------------------------------------------------------------------------------------
-- Seeds required by the directive: titles, and the default-deny rights policies

INSERT INTO research.games (game_id, title, publisher, notes) VALUES
    ('maplestory',        'MapleStory',         'Nexon', 'regular client, any world type; requires title-specific review'),
    ('maplestory_worlds', 'MapleStory Worlds',  'Nexon', 'a different product with its own terms; requires title-specific review'),
    ('synthetic',         'Synthetic data',     NULL,    'made by research::synthetic; no game, no player'),
    ('other',             'Other title',        NULL,    'a known title without its own id yet'),
    ('unknown',           'Not identified',     NULL,    'never recordable: no rights policy can cover it');

INSERT INTO rights.rights_policies
    (rights_policy_id, game_id, deliverable, rights_holder, status, valid_from, valid_until, notes) VALUES
    ('maplestory-events-review',    'maplestory',        'event_metadata', 'Nexon',
     'requires_title_specific_review', '2026-10-10', '2126-10-10', 'directive §10: not approved; no grants'),
    ('maplestory-media-review',     'maplestory',        'media',          'Nexon',
     'requires_title_specific_review', '2026-10-10', '2126-10-10', 'Nexon restricts commercial use of gameplay footage'),
    ('mapleworlds-events-review',   'maplestory_worlds', 'event_metadata', 'Nexon',
     'requires_title_specific_review', '2026-10-10', '2126-10-10', 'directive §10: not approved; no grants'),
    ('mapleworlds-media-review',    'maplestory_worlds', 'media',          'Nexon',
     'requires_title_specific_review', '2026-10-10', '2126-10-10', 'MapleStory Worlds terms to be reviewed');
-- (No rights_grants rows for these four: nothing is granted.)

INSERT INTO rights.rights_policies
    (rights_policy_id, game_id, deliverable, rights_holder, status, basis, valid_from, valid_until,
     reviewed_by_role, reviewed_at, notes) VALUES
    ('synthetic-local-demo', 'synthetic', 'event_metadata', 'Syrup (generated data)', 'approved',
     'generated by research::synthetic; contains no game content and no player', '2026-10-10', '2027-10-10',
     'Data Engineering', '2026-10-10', 'local demonstration only (this_device)');
INSERT INTO rights.rights_grants (rights_policy_id, purpose, recipient_class, use)
SELECT 'synthetic-local-demo', p, 'this_device', u
FROM unnest(ARRAY['improve_syrup', 'aggregate_analytics', 'external_research_training']) AS p,
     unnest(ARRAY['capture', 'store', 'annotate', 'evaluate']) AS u;

-- ---------------------------------------------------------------------------------------------
-- Least privilege (DATA_ARCHITECTURE.md §6)

GRANT USAGE ON SCHEMA consent TO syrup_consent;
GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA consent TO syrup_consent;
GRANT USAGE ON SCHEMA consent TO syrup_ingest, syrup_export;
REVOKE ALL ON ALL TABLES IN SCHEMA consent FROM syrup_ingest, syrup_export, syrup_analyst;
REVOKE EXECUTE ON FUNCTION consent.receipt_allows(text, integer, text, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION consent.receipt_allows(text, integer, text, text, text) TO syrup_ingest, syrup_export;

GRANT USAGE ON SCHEMA rights TO syrup_ingest, syrup_export, syrup_analyst, syrup_rights_reviewer;
GRANT SELECT ON ALL TABLES IN SCHEMA rights TO syrup_ingest, syrup_export, syrup_analyst;
GRANT SELECT, INSERT, UPDATE ON ALL TABLES IN SCHEMA rights TO syrup_rights_reviewer;

GRANT USAGE ON SCHEMA research TO syrup_ingest, syrup_export, syrup_analyst, syrup_deletion;
GRANT SELECT, INSERT ON research.research_subjects, research.sessions, research.ingest_batches,
    research.event_ids, research.events, research.episodes, research.episode_sessions,
    research.labels, research.quarantine, research.media_objects TO syrup_ingest;
GRANT UPDATE (status, status_changed_at) ON research.research_subjects TO syrup_ingest;
GRANT UPDATE (qa_status, qa_checked_at) ON research.episodes TO syrup_ingest;
GRANT UPDATE (resolved_at, resolution) ON research.quarantine TO syrup_ingest;
GRANT SELECT ON ALL TABLES IN SCHEMA research TO syrup_analyst, syrup_export;
GRANT SELECT, DELETE ON ALL TABLES IN SCHEMA research TO syrup_deletion;
GRANT UPDATE (status, status_changed_at) ON research.research_subjects TO syrup_deletion;
GRANT UPDATE (research_subject_id) ON research.ingest_batches TO syrup_deletion;

GRANT USAGE ON SCHEMA delivery TO syrup_export, syrup_analyst, syrup_deletion;
GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA delivery TO syrup_export;
GRANT UPDATE (delivered_at, url_expires_at, revoked_at, status) ON delivery.exports TO syrup_export;
GRANT UPDATE (status) ON delivery.datasets TO syrup_export;
GRANT SELECT ON ALL TABLES IN SCHEMA delivery TO syrup_analyst;
GRANT SELECT, INSERT, DELETE ON ALL TABLES IN SCHEMA delivery TO syrup_deletion;
GRANT UPDATE (replaced_by_artifact_id) ON delivery.artifacts TO syrup_deletion;
GRANT UPDATE (status) ON delivery.exports TO syrup_deletion;

GRANT USAGE ON SCHEMA deletion TO syrup_deletion, syrup_analyst;
GRANT SELECT, INSERT, UPDATE ON ALL TABLES IN SCHEMA deletion TO syrup_deletion;
GRANT SELECT ON ALL TABLES IN SCHEMA deletion TO syrup_analyst;

GRANT USAGE ON SCHEMA audit TO syrup_ingest, syrup_export, syrup_deletion, syrup_consent;
GRANT INSERT ON audit.access_log TO syrup_ingest, syrup_export, syrup_deletion, syrup_consent;

COMMIT;
