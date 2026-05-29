#!/usr/bin/env bash
# Seed the local CTF database with fake playground submissions for scoreboard testing.
#
# Usage: ./scripts/dev/seed-submissions.sh

set -euo pipefail

PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-ctf}"
PGPASSWORD="${PGPASSWORD:-ctf}"
PGDATABASE="${PGDATABASE:-ctf_app}"
CTF_NAME="playground"

export PGHOST PGPORT PGUSER PGPASSWORD PGDATABASE

if ! command -v psql >/dev/null 2>&1; then
    echo "ERROR: psql is required but was not found in PATH." >&2
    exit 1
fi

echo "=== Seeding ${CTF_NAME} submissions ==="
echo "Database: postgresql://${PGUSER}@${PGHOST}:${PGPORT}/${PGDATABASE}"

psql -X -v ON_ERROR_STOP=1 -qAt -c 'SELECT 1' >/dev/null

summary=$(psql -X -v ON_ERROR_STOP=1 -qAt -F '|' <<'SQL'
BEGIN;

CREATE UNIQUE INDEX IF NOT EXISTS idx_flag_submissions_seed_unique
ON flag_submissions (user_id, challenge_name, ctf_name, flag_name, accepted, submitted_flag)
WHERE ip_address = 'seed-submissions';

-- Ensure test users exist; prefer existing real users over seeded ones.
WITH seeded_users(sub, name, email, team_id, team_name) AS (
    VALUES
        ('keycloak-sub-player', 'player', 'player@example.test', 'team-alpha', 'Team Alpha'),
        ('keycloak-sub-test2', 'test2', 'test2@example.test', 'team-alpha', 'Team Alpha'),
        ('keycloak-sub-test3', 'test3', 'test3@example.test', 'team-alpha', 'Team Alpha'),
        ('keycloak-sub-test4', 'test4', 'test4@example.test', 'team-beta', 'Team Beta'),
        ('keycloak-sub-test5', 'test5', 'test5@example.test', 'team-beta', 'Team Beta'),
        ('keycloak-sub-test6', 'test6', 'test6@example.test', 'team-beta', 'Team Beta')
),
upserted_users AS (
    INSERT INTO users (sub, name, email, team_id, team_name)
    SELECT sub, name, email, team_id, team_name
    FROM seeded_users su
    WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.name = su.name)
    ON CONFLICT (sub) DO UPDATE SET
        name = EXCLUDED.name,
        email = EXCLUDED.email,
        team_id = EXCLUDED.team_id,
        team_name = EXCLUDED.team_name
    RETURNING id
),
-- Map usernames to user IDs: prefer the real (lowest id) user if duplicates exist
user_map AS (
    SELECT DISTINCT ON (name) id, name, team_id
    FROM users
    WHERE name IN ('player','test2','test3','test4','test5','test6')
    ORDER BY name, id
),
seeded_submissions(username, challenge_name, flag_name, accepted, minutes_ago) AS (
    VALUES
        ('player', 'misc-sanity', 'flag', true, 235),
        ('test4', 'foren-pcap', 'flag', true, 232),
        ('player', 'crypto-caesar', 'flag', true, 228),
        ('test4', 'crypto-rsa', 'flag', true, 225),
        ('player', 'web-hello', 'flag1', true, 222),
        ('player', 'misc-trivia', 'flag', true, 215),
        ('test4', 'rev-strings', 'flag', true, 210),
        ('player', 'web-xss', 'flag', true, 205),
        ('player', 'foren-pcap', 'flag', true, 190),
        ('player', 'rev-strings', 'flag', true, 186),
        ('player', 'crypto-rsa', 'flag', true, 183),
        ('test4', 'misc-sanity', 'flag', true, 180),
        ('test4', 'crypto-caesar', 'flag', true, 178),
        ('test4', 'web-hello', 'flag1', true, 176),
        ('test4', 'pwn-overflow', 'flag', true, 174),
        ('test5', 'misc-trivia', 'flag', true, 172),
        ('test4', 'web-xss', 'flag', true, 168),
        ('player', 'pwn-overflow', 'flag', true, 160),
        ('test6', 'crypto-aes', 'flag', true, 162),
        ('test3', 'web-hello', 'flag1', true, 164),
        ('test2', 'web-hello', 'flag2', true, 156),
        ('test5', 'foren-stego', 'flag', true, 154),
        ('test5', 'misc-sanity', 'flag', true, 150),
        ('test3', 'web-hello', 'flag2', true, 147),
        ('test6', 'misc-jail', 'flag', true, 145),
        ('test2', 'rev-xor', 'flag', true, 142),
        ('test5', 'rev-xor', 'flag', true, 140),
        ('test6', 'crypto-caesar', 'flag', true, 138),
        ('test4', 'foren-stego', 'flag', true, 136),
        ('test6', 'rev-strings', 'flag', true, 134),
        ('test3', 'web-hello', 'flag3', true, 130),
        ('test2', 'foren-stego', 'flag', true, 128),
        ('test5', 'web-xss', 'flag', true, 126),
        ('test6', 'foren-pcap', 'flag', true, 123),
        ('test4', 'web-sqli', 'flag', true, 121),
        ('test6', 'pwn-format', 'flag', true, 118),
        ('test5', 'crypto-rsa', 'flag', true, 116),
        ('test4', 'rev-xor', 'flag', true, 114),
        ('test2', 'pwn-format', 'flag', true, 112),
        ('test6', 'foren-disk', 'flag', true, 109),
        ('test3', 'web-ssrf', 'flag', true, 105),
        ('test3', 'crypto-ecc', 'flag', true, 101),
        ('test5', 'rev-packed', 'flag', true, 96),
        ('test4', 'pwn-heap', 'flag', true, 92),
        ('test6', 'foren-memory', 'flag', true, 88),
        ('test5', 'crypto-lattice', 'flag', true, 82),
        ('test3', 'misc-quantum', 'flag', true, 76),
        ('player', 'misc-jail', 'flag', true, 70),
        ('test6', 'web-sqli', 'flag', true, 68),
        ('test5', 'pwn-overflow', 'flag', true, 64),
        ('test4', 'misc-jail', 'flag', true, 60),
        ('test6', 'pwn-overflow', 'flag', true, 58),
        ('test5', 'crypto-aes', 'flag', true, 54),
        ('player', 'crypto-aes', 'flag', true, 50),
        ('player', 'web-sqli', 'flag', false, 166),
        ('test5', 'web-hello', 'flag2', false, 149),
        ('test4', 'crypto-rsa', 'flag', false, 133),
        ('test6', 'misc-sanity', 'flag', false, 129),
        ('test5', 'crypto-ecc', 'flag', false, 120),
        ('test4', 'pwn-kernel', 'flag', false, 84)
),
resolved_submissions AS (
    SELECT
        u.id AS user_id,
        u.team_id,
        s.challenge_name,
        'playground'::text AS ctf_name,
        s.flag_name,
        CASE
            WHEN s.accepted THEN format('seed{%s:%s}', s.challenge_name, s.flag_name)
            ELSE format('wrong{%s:%s:%s}', s.username, s.challenge_name, s.flag_name)
        END AS submitted_flag,
        s.accepted,
        now() - make_interval(mins => s.minutes_ago) AS created_at
    FROM seeded_submissions s
    JOIN user_map u ON u.name = s.username
),
inserted_submissions AS (
    INSERT INTO flag_submissions (
        user_id,
        challenge_name,
        ctf_name,
        flag_name,
        submitted_flag,
        accepted,
        cheated,
        admin_submission,
        ip_address,
        created_at,
        team_id
    )
    SELECT
        user_id,
        challenge_name,
        ctf_name,
        flag_name,
        submitted_flag,
        accepted,
        false,
        false,
        'seed-submissions',
        created_at,
        team_id
    FROM resolved_submissions
    ON CONFLICT (user_id, challenge_name, ctf_name, flag_name, accepted, submitted_flag)
        WHERE ip_address = 'seed-submissions'
        DO NOTHING
    RETURNING accepted
),
accepted_seed AS (
    SELECT user_id, challenge_name, flag_name, created_at, team_id
    FROM flag_submissions
    WHERE ctf_name = 'playground'
      AND ip_address = 'seed-submissions'
      AND accepted = true
),
required_flags AS (
    SELECT
        challenge_name,
        CASE WHEN challenge_name = 'web-hello' THEN 3 ELSE 1 END AS required_count
    FROM (
        SELECT DISTINCT challenge_name
        FROM accepted_seed
    ) challenges
),
entity_progress AS (
    SELECT
        CASE
            WHEN team_id IS NOT NULL THEN 'team:' || team_id
            ELSE 'user:' || user_id::text
        END AS entity_key,
        challenge_name,
        count(DISTINCT flag_name) AS solved_flags,
        max(created_at) AS completed_at
    FROM accepted_seed
    GROUP BY 1, 2
),
complete_entities AS (
    SELECT ep.entity_key, ep.challenge_name, ep.completed_at
    FROM entity_progress ep
    JOIN required_flags rf USING (challenge_name)
    WHERE ep.solved_flags = rf.required_count
),
challenge_contributors AS (
    SELECT DISTINCT
        s.user_id,
        s.challenge_name,
        ce.completed_at
    FROM accepted_seed s
    JOIN complete_entities ce
      ON ce.challenge_name = s.challenge_name
     AND ce.entity_key = CASE
            WHEN s.team_id IS NOT NULL THEN 'team:' || s.team_id
            ELSE 'user:' || s.user_id::text
        END
),
inserted_solved AS (
    INSERT INTO solved_challenges (user_id, challenge_name, ctf_name, solved_at)
    SELECT user_id, challenge_name, 'playground', completed_at
    FROM challenge_contributors
    ON CONFLICT (user_id, challenge_name, ctf_name) DO NOTHING
    RETURNING 1
)
SELECT
    (SELECT count(*) FROM upserted_users),
    (SELECT count(*) FROM inserted_submissions WHERE accepted),
    (SELECT count(*) FROM inserted_submissions WHERE NOT accepted),
    (SELECT count(*) FROM inserted_solved);

COMMIT;
SQL
)

IFS='|' read -r users_upserted accepted_inserted rejected_inserted solved_inserted <<<"${summary}"

echo ""
echo "=== Seed summary ==="
printf 'Users upserted:            %s\n' "${users_upserted}"
printf 'Accepted submissions added: %s\n' "${accepted_inserted}"
printf 'Rejected submissions added: %s\n' "${rejected_inserted}"
printf 'Solved challenge rows added: %s\n' "${solved_inserted}"
echo ""
echo "Scoreboard-shaped totals (seeded rows only):"

psql -X -v ON_ERROR_STOP=1 -qAt -F '|' <<'SQL' | while IFS='|' read -r entity accepted rejected; do
WITH seeded AS (
    SELECT
        CASE
            WHEN u.team_id IS NOT NULL THEN u.team_name
            ELSE u.name
        END AS entity_name,
        CASE
            WHEN u.team_id IS NOT NULL THEN 'team:' || u.team_id
            ELSE 'user:' || u.id::text
        END AS entity_key,
        count(*) FILTER (WHERE fs.accepted) AS accepted_count,
        count(*) FILTER (WHERE NOT fs.accepted) AS rejected_count
    FROM flag_submissions fs
    JOIN users u ON u.id = fs.user_id
    WHERE fs.ctf_name = 'playground'
      AND fs.ip_address = 'seed-submissions'
    GROUP BY 1, 2
)
SELECT entity_name, accepted_count, rejected_count
FROM seeded
ORDER BY accepted_count DESC, rejected_count DESC, entity_name;
SQL
    printf '  %-12s accepted=%-2s rejected=%s\n' "${entity}" "${accepted}" "${rejected}"
done

echo ""
echo "Done. Re-running this script is safe."
