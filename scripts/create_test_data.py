#!/usr/bin/env python3
"""Create test users and random solves for scoreboard testing."""

import random
import string
import subprocess
import sys

NUM_USERS = 1000
CTF_NAME = "playground"
NAMESPACE = "ctf"

# Probability of each user solving each flag (adjust for variety)
SOLVE_PROBABILITY = 0.15  # 15% chance per flag

def run_sql(sql):
    """Run SQL via podman exec."""
    result = subprocess.run(
        ["podman", "exec", "-i", "app-postgres-1", "psql", "-U", "ctf", "-d", "ctf_app", "-t", "-c", sql],
        capture_output=True,
        text=True
    )
    # Clean up output - get just the first line with numbers
    out = result.stdout.strip()
    lines = [l.strip() for l in out.split('\n') if l.strip() and not l.startswith('INSERT') and not l.startswith('(')]
    return lines[0] if lines else out

def get_challenges():
    """Get list of challenges with their flags."""
    sql = """
    SELECT name, flags_json::text 
    FROM app_challenges 
    WHERE ctf_name = 'playground';
    """
    result = run_sql(sql)
    challenges = []
    for line in result.split('\n'):
        line = line.strip()
        if '|' in line:
            parts = line.split('|')
            name = parts[0].strip()
            flags_json = parts[1].strip()
            # Parse the JSON manually (simple cases)
            import json
            try:
                flags = json.loads(flags_json)
                flag_names = [f.get('name', 'flag') for f in flags]
                challenges.append((name, flag_names))
            except:
                challenges.append((name, ['flag']))
    return challenges

def main():
    print("Getting challenges...")
    challenges = get_challenges()
    print(f"Found {len(challenges)} challenges")
    
    if not challenges:
        print("No challenges found! Make sure the app is synced.")
        sys.exit(1)
    
    # Create users
    print(f"\nCreating {NUM_USERS} users...")
    user_ids = []
    
    for i in range(1, NUM_USERS + 1):
        username = f"player{i:04d}"
        email = f"{username}@ctf.local"
        sub = f"test-{username}"
        name = f"Player {i}"
        
        sql = f"""
        INSERT INTO app_users (sub, email, name, roles_json) 
        VALUES ('{sub}', '{email}', '{name}', '[]'::jsonb)
        ON CONFLICT (sub) DO UPDATE SET name = '{name}'
        RETURNING id;
        """
        result = run_sql(sql)
        try:
            user_id = int(result.strip())
            user_ids.append(user_id)
        except:
            # Try to get existing user
            sql = f"SELECT id FROM app_users WHERE sub = '{sub}';"
            result = run_sql(sql)
            try:
                user_id = int(result.strip())
                user_ids.append(user_id)
            except:
                pass
        
        if i % 100 == 0:
            print(f"  Created {i} users...")
    
    print(f"Created {len(user_ids)} users")
    
    # Generate random solves
    print("\nGenerating random solves...")
    total_solves = 0
    
    for challenge_name, flag_names in challenges:
        for flag_name in flag_names:
            # Determine how many users solve this flag
            num_solvers = int(len(user_ids) * SOLVE_PROBABILITY * random.uniform(0.3, 2.0))
            num_solvers = min(num_solvers, len(user_ids))
            
            solvers = random.sample(user_ids, num_solvers)
            
            for user_id in solvers:
                # Random timestamp in last 7 days
                hours_ago = random.randint(0, 168)
                
                sql = f"""
                INSERT INTO app_flag_submissions 
                (user_id, challenge_namespace, challenge_name, ctf_name, flag_name, submitted_flag, accepted, created_at)
                VALUES ({user_id}, '{NAMESPACE}', '{challenge_name}', '{CTF_NAME}', '{flag_name}', 'SCC{{solved}}', true, now() - interval '{hours_ago} hours')
                ON CONFLICT DO NOTHING;
                """
                run_sql(sql)
                total_solves += 1
            
            print(f"  {challenge_name}/{flag_name}: {num_solvers} solves")
    
    # Mark challenges as solved for users who solved all flags
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
    
    print(f"\nDone! Created ~{total_solves} flag submissions")
    
    # Quick stats
    print("\n--- Stats ---")
    sql = "SELECT count(*) FROM app_users WHERE sub LIKE 'test-%';"
    print(f"Test users: {run_sql(sql)}")
    
    sql = "SELECT count(*) FROM app_flag_submissions WHERE accepted = true;"
    print(f"Total accepted submissions: {run_sql(sql)}")
    
    sql = "SELECT count(*) FROM app_solved_challenges;"
    print(f"Fully solved challenges: {run_sql(sql)}")

if __name__ == "__main__":
    main()
