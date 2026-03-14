#!/usr/bin/env python3
"""Add random solves for existing users - batch insert version."""

import random
import subprocess
import json

CTF_NAME = "playground"
NAMESPACE = "ctf"
SOLVE_PROBABILITY = 0.15

def run_sql(sql):
    """Run SQL via podman exec."""
    result = subprocess.run(
        ["podman", "exec", "-i", "app-postgres-1", "psql", "-U", "ctf", "-d", "ctf_app", "-t", "-c", sql],
        capture_output=True,
        text=True
    )
    return result.stdout.strip()

def main():
    # Get challenges
    print("Getting challenges...")
    sql = "SELECT name, flags_json::text FROM app_challenges WHERE ctf_name = 'playground';"
    result = run_sql(sql)
    
    challenges = []
    for line in result.split('\n'):
        line = line.strip()
        if '|' in line:
            parts = line.split('|', 1)
            name = parts[0].strip()
            flags_json = parts[1].strip()
            try:
                flags = json.loads(flags_json)
                flag_names = [f.get('name', 'flag') for f in flags]
                challenges.append((name, flag_names))
            except:
                challenges.append((name, ['flag']))
    
    print(f"Found {len(challenges)} challenges")
    
    # Get user IDs
    print("Getting user IDs...")
    sql = "SELECT id FROM app_users ORDER BY id;"
    result = run_sql(sql)
    user_ids = [int(line.strip()) for line in result.split('\n') if line.strip().isdigit()]
    print(f"Found {len(user_ids)} users")
    
    # Generate solves using batch insert
    print("\nGenerating solves...")
    
    for challenge_name, flag_names in challenges:
        for flag_name in flag_names:
            # Random number of solvers
            num_solvers = int(len(user_ids) * SOLVE_PROBABILITY * random.uniform(0.3, 2.0))
            num_solvers = min(num_solvers, len(user_ids))
            
            if num_solvers == 0:
                continue
            
            solvers = random.sample(user_ids, num_solvers)
            
            # Build batch VALUES
            values = []
            for user_id in solvers:
                hours_ago = random.randint(0, 168)
                values.append(f"({user_id}, '{NAMESPACE}', '{challenge_name}', '{CTF_NAME}', '{flag_name}', 'SCC{{solved}}', true, now() - interval '{hours_ago} hours')")
            
            # Batch insert
            sql = f"""
            INSERT INTO app_flag_submissions 
            (user_id, challenge_namespace, challenge_name, ctf_name, flag_name, submitted_flag, accepted, created_at)
            VALUES {','.join(values)}
            ON CONFLICT DO NOTHING;
            """
            run_sql(sql)
            print(f"  {challenge_name}/{flag_name}: {num_solvers} solves")
    
    # Mark complete solves
    print("\nMarking complete solves...")
    sql = """
    INSERT INTO app_solved_challenges (user_id, challenge_namespace, challenge_name, ctf_name, solved_at)
    SELECT DISTINCT s.user_id, s.challenge_namespace, s.challenge_name, s.ctf_name, MAX(s.created_at)
    FROM app_flag_submissions s
    JOIN app_challenges c ON c.name = s.challenge_name AND c.ctf_name = s.ctf_name
    WHERE s.accepted = true
    GROUP BY s.user_id, s.challenge_namespace, s.challenge_name, s.ctf_name, c.flags_json
    HAVING COUNT(DISTINCT s.flag_name) >= jsonb_array_length(c.flags_json)
    ON CONFLICT DO NOTHING;
    """
    run_sql(sql)
    
    # Stats
    print("\n--- Stats ---")
    sql = "SELECT count(*) FROM app_users;"
    print(f"Total users: {run_sql(sql)}")
    
    sql = "SELECT count(*) FROM app_flag_submissions WHERE accepted = true;"
    print(f"Total accepted submissions: {run_sql(sql)}")
    
    sql = "SELECT count(*) FROM app_solved_challenges;"
    print(f"Fully solved challenges: {run_sql(sql)}")

if __name__ == "__main__":
    main()
