#!/usr/bin/env python3
"""
Team-sharing end-to-end tests using Playwright (Python).

Verifies that actions by one teammate are visible to another:
  1. Flag submission by user A → teammate B sees challenge as solved
  2. Instance spawn by user A → teammate B sees the instance
  3. Instance stop by user A → teammate B sees it gone

Requires:
  - python3 -m playwright install chromium
  - App running on localhost:8090 with APP_MODE=team
  - Keycloak running on localhost:8081
  - Operator running with correct config

Uses team-beta (test4 + test5) on an unsolved challenge to avoid
conflicts with existing seed data.
"""

import subprocess
import sys
import time
from playwright.sync_api import sync_playwright, expect

BASE_URL = "http://localhost:8090"
KEYCLOAK_URL = "http://localhost:8081"

# Two teammates on team-beta (password = username)
USER_A = "test4"
USER_B = "test5"

# Challenge that team-beta hasn't solved and has a single static-body flag
TEST_CHALLENGE = "web-rce"


def log(msg: str):
    print(f"  ✓ {msg}")


def log_section(msg: str):
    print(f"\n{'='*60}")
    print(f"  {msg}")
    print(f"{'='*60}")


def keycloak_login(page, username, password):
    """Navigate to app and complete Keycloak OAuth login."""
    page.goto(BASE_URL)
    page.wait_for_load_state("domcontentloaded")

    login_btn = page.locator(
        "a:has-text('Login'), button:has-text('Login'), a:has-text('Sign in')"
    )
    if login_btn.count() > 0:
        login_btn.first.click()
        page.wait_for_load_state("domcontentloaded")

    if "keycloak" in page.url or "realms" in page.url:
        page.fill("#username", username)
        page.fill("#password", password)
        page.click("#kc-login")
        page.wait_for_load_state("domcontentloaded")

    page.wait_for_url("**/localhost:8090/**", timeout=10000)
    body = page.text_content("body")
    assert "Logout" in body, f"Login failed for {username}: {body[:200]}"


def get_flag_body_from_instance(instance_ns: str, flag_name: str) -> str:
    """Read the flag body from the Flag CRD in the instance namespace."""
    result = subprocess.run(
        [
            "kubectl",
            "get",
            "flag",
            "-n",
            instance_ns,
            "-o",
            "jsonpath={.items[*].metadata.name}|{.items[*].spec.body}",
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return ""
    parts = result.stdout.split("|")
    if len(parts) != 2:
        return ""
    names = parts[0].split()
    bodies = parts[1].split()
    for n, b in zip(names, bodies):
        if flag_name in n:
            return b
    return ""


def get_flag_value_from_instance(instance_ns: str, flag_name: str) -> str:
    """Read the full rendered flag value (with prefix+random) from Flag CRD status."""
    result = subprocess.run(
        [
            "kubectl",
            "get",
            "flag",
            "-n",
            instance_ns,
            "--no-headers",
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return ""
    # Output format: NAME  VALUE
    for line in result.stdout.strip().split("\n"):
        cols = line.split()
        if len(cols) >= 2 and flag_name in cols[0]:
            return cols[1]
    return ""


def cleanup_test_data():
    """Remove any flag_submissions and solved_challenges for the test challenge + team."""
    subprocess.run(
        [
            "psql",
            "postgres://ctf:ctf@localhost:5432/ctf_app",
            "-c",
            f"""
            DELETE FROM solved_challenges
            WHERE challenge_name = '{TEST_CHALLENGE}'
              AND user_id IN (SELECT id FROM users WHERE team_id = 'team-beta');
            DELETE FROM flag_submissions
            WHERE challenge_name = '{TEST_CHALLENGE}'
              AND user_id IN (SELECT id FROM users WHERE team_id = 'team-beta');
            """,
        ],
        capture_output=True,
    )


def cleanup_test_instance():
    """Delete any team-beta instance for the test challenge."""
    result = subprocess.run(
        ["kubectl", "get", "instance", "-A", "--no-headers"],
        capture_output=True,
        text=True,
    )
    for line in result.stdout.strip().split("\n"):
        if not line:
            continue
        cols = line.split()
        if len(cols) >= 2 and TEST_CHALLENGE in cols[1] and "team-beta" in cols[1]:
            ns = cols[0]
            name = cols[1]
            subprocess.run(
                ["kubectl", "delete", "instance", name, "-n", ns, "--wait=false"],
                capture_output=True,
            )
            # Wait for deletion
            for _ in range(30):
                check = subprocess.run(
                    ["kubectl", "get", "instance", name, "-n", ns],
                    capture_output=True,
                )
                if check.returncode != 0:
                    break
                time.sleep(1)


def navigate_to_challenge(page, context, username, challenge):
    """Navigate to challenge page, re-authenticating if needed."""
    page.goto(f"{BASE_URL}/challenge/{challenge}/")
    page.wait_for_load_state("domcontentloaded")
    time.sleep(0.5)
    body = page.text_content("body")
    if "Login required" in body or ("Login" in body and "Logout" not in body):
        keycloak_login(page, username, username)
        log(f"Re-authenticated {username}")
        page.goto(f"{BASE_URL}/challenge/{challenge}/")
        page.wait_for_load_state("domcontentloaded")
        time.sleep(0.5)
    return page.text_content("body")


def test_flag_submission_sharing(context_a, context_b):
    """Test: User A submits flag → User B sees challenge as solved."""
    log_section("TEST: Flag Submission Sharing")

    page_a = context_a.pages[0]
    page_b = context_b.pages[0]

    # User A navigates to challenge and spawns if needed
    body = navigate_to_challenge(page_a, context_a, USER_A, TEST_CHALLENGE)

    spawn_btn = page_a.locator("button:has-text('Spawn instance')")
    if spawn_btn.count() > 0:
        spawn_btn.first.click()
        page_a.wait_for_load_state("domcontentloaded")
        log(f"{USER_A} clicked spawn on {TEST_CHALLENGE}")

        # Wait for instance to become Ready
        for tick in range(60):
            time.sleep(2)
            page_a.reload()
            page_a.wait_for_load_state("domcontentloaded")
            body = page_a.text_content("body")
            if "Stop instance" in body:
                log(f"Instance ready after {(tick+1)*2}s")
                break
        else:
            raise AssertionError("Instance did not become Ready within 120s")
    elif "Stop instance" in body:
        log("Instance already running")
    else:
        raise AssertionError(f"No spawn button and no running instance: {body[:200]}")

    # User B should also see the instance (team sharing)
    body_b = navigate_to_challenge(page_b, context_b, USER_B, TEST_CHALLENGE)
    assert "Stop instance" in body_b, \
        f"{USER_B} should see team instance but got: {body_b[:200]}"
    log(f"{USER_B} sees the team instance (Stop button visible)")

    # Get the actual flag value from the instance
    instance_ns_prefix = "ctf-inst-playground"
    instance_name = f"{instance_ns_prefix}-{TEST_CHALLENGE}-tteam-beta"
    flag_value = get_flag_value_from_instance(instance_name, "flag")
    assert flag_value, f"Could not read flag value from instance {instance_name}"
    log(f"Flag value retrieved from instance: {flag_value[:20]}...")

    # User A submits the flag — fresh navigation to get enabled inputs
    navigate_to_challenge(page_a, context_a, USER_A, TEST_CHALLENGE)

    flag_form = page_a.locator("form[action='/api/submit_flag']")
    assert flag_form.count() > 0, \
        f"No flag form found. Body: {page_a.text_content('body')[:300]}"
    flag_input = flag_form.locator("input[name='submitted_flag']:not([disabled])")
    assert flag_input.count() > 0, \
        f"No enabled flag input found. Body: {page_a.text_content('body')[:300]}"
    flag_input.first.fill(flag_value)
    # Submit via the flag form specifically (not the logout form)
    flag_form.locator("button[type='submit']").first.click()
    page_a.wait_for_load_state("domcontentloaded")
    time.sleep(1)

    # After submit, check for accept message — may need to re-auth if session died
    body_a = page_a.text_content("body")
    if "Login required" in body_a or ("Login" in body_a and "Logout" not in body_a):
        # Session died during POST. Re-login and check if submission went through anyway
        keycloak_login(page_a, USER_A, USER_A)
        page_a.goto(f"{BASE_URL}/challenge/{TEST_CHALLENGE}/")
        page_a.wait_for_load_state("domcontentloaded")
        time.sleep(0.5)
        body_a = page_a.text_content("body")
        # The challenge should now show as solved if submission was persisted
        is_solved = (
            "solved" in body_a.lower()
            or page_a.locator(".solved-badge, .tile.solved").count() > 0
        )
        assert is_solved, f"Flag submission lost during session refresh: {body_a[:300]}"
        log(f"{USER_A} flag was accepted (verified after re-auth)")
    else:
        assert "accepted" in body_a.lower() or "✓" in body_a, \
            f"Flag not accepted for {USER_A}: {body_a[:300]}"
        log(f"{USER_A} submitted flag successfully")

    # User B reloads and should see the challenge as solved
    body_b = navigate_to_challenge(page_b, context_b, USER_B, TEST_CHALLENGE)
    flag_input_b = page_b.locator("input[name='submitted_flag']:not([disabled])")
    solved_indicator = (
        "solved" in body_b.lower()
        or page_b.locator(".solved-badge, .tile.solved").count() > 0
        or flag_input_b.count() == 0
    )
    assert solved_indicator, \
        f"{USER_B} should see challenge as solved but got: {body_b[:300]}"
    log(f"{USER_B} sees the challenge as solved (team sharing works)")


def test_instance_spawn_sharing(context_a, context_b):
    """Test: User A spawns instance → User B sees it."""
    log_section("TEST: Instance Spawn Sharing")

    page_a = context_a.pages[0]
    page_b = context_b.pages[0]

    # User B checks — should see no instance initially
    page_b.goto(f"{BASE_URL}/challenge/{TEST_CHALLENGE}/")
    page_b.wait_for_load_state("domcontentloaded")
    time.sleep(1)
    body_b = page_b.text_content("body")
    has_spawn = "Spawn instance" in body_b
    assert has_spawn, f"{USER_B} should see Spawn button, got: {body_b[:200]}"
    log(f"{USER_B} sees Spawn button (no instance yet)")

    # User A spawns instance
    page_a.goto(f"{BASE_URL}/challenge/{TEST_CHALLENGE}/")
    page_a.wait_for_load_state("domcontentloaded")
    time.sleep(1)
    spawn_btn = page_a.locator("button:has-text('Spawn instance')")
    assert spawn_btn.count() > 0, f"No spawn button for {USER_A}"
    spawn_btn.first.click()
    page_a.wait_for_load_state("domcontentloaded")
    log(f"{USER_A} clicked Spawn")

    # Wait for Ready
    for tick in range(60):
        time.sleep(2)
        page_a.reload()
        page_a.wait_for_load_state("domcontentloaded")
        body = page_a.text_content("body")
        if "Stop instance" in body:
            log(f"Instance ready after {(tick+1)*2}s")
            break
    else:
        raise AssertionError("Instance did not become Ready within 120s")

    # User B reloads — should now see the instance
    page_b.goto(f"{BASE_URL}/challenge/{TEST_CHALLENGE}/")
    page_b.wait_for_load_state("domcontentloaded")
    time.sleep(1)
    body_b = page_b.text_content("body")
    assert "Stop instance" in body_b, \
        f"{USER_B} should see Stop button (team instance), got: {body_b[:200]}"
    log(f"{USER_B} sees the team instance spawned by {USER_A}")


def test_instance_stop_sharing(context_a, context_b):
    """Test: User A stops instance → User B sees it gone."""
    log_section("TEST: Instance Stop Sharing")

    page_a = context_a.pages[0]
    page_b = context_b.pages[0]

    # Precondition: instance should be running (from previous test)
    page_a.goto(f"{BASE_URL}/challenge/{TEST_CHALLENGE}/")
    page_a.wait_for_load_state("domcontentloaded")
    time.sleep(1)
    body_a = page_a.text_content("body")
    assert "Stop instance" in body_a, \
        f"Precondition failed: {USER_A} should see running instance"

    # User A stops the instance
    stop_btn = page_a.locator("button:has-text('Stop instance')")
    stop_btn.first.click()
    page_a.wait_for_load_state("domcontentloaded")
    log(f"{USER_A} clicked Stop")

    # Wait for instance to be gone
    for tick in range(30):
        time.sleep(2)
        page_a.reload()
        page_a.wait_for_load_state("domcontentloaded")
        body = page_a.text_content("body")
        if "Spawn instance" in body:
            log(f"Instance stopped after {(tick+1)*2}s")
            break
    else:
        raise AssertionError("Instance did not stop within 60s")

    # User B reloads — should see Spawn button again
    page_b.goto(f"{BASE_URL}/challenge/{TEST_CHALLENGE}/")
    page_b.wait_for_load_state("domcontentloaded")
    time.sleep(1)
    body_b = page_b.text_content("body")
    assert "Spawn instance" in body_b, \
        f"{USER_B} should see Spawn button after stop, got: {body_b[:200]}"
    log(f"{USER_B} sees instance is gone (Spawn button back)")


def main():
    print("\n" + "=" * 60)
    print("  CTF Platform — Team Sharing Tests (Playwright)")
    print("=" * 60)

    # Clean up before running
    log_section("SETUP: Cleaning test data")
    cleanup_test_instance()
    cleanup_test_data()
    log("Cleaned up previous test data")

    failed = []

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)

        # Create two separate browser contexts (separate sessions)
        context_a = browser.new_context(
            viewport={"width": 1280, "height": 800},
            ignore_https_errors=True,
        )
        context_b = browser.new_context(
            viewport={"width": 1280, "height": 800},
            ignore_https_errors=True,
        )

        page_a = context_a.new_page()
        page_b = context_b.new_page()

        # Log in both users
        log_section("SETUP: Logging in both teammates")
        keycloak_login(page_a, USER_A, USER_A)
        log(f"{USER_A} logged in")
        keycloak_login(page_b, USER_B, USER_B)
        log(f"{USER_B} logged in")

        # Run instance tests first (before flag submission marks it solved)
        tests = [
            ("Instance Spawn Sharing", test_instance_spawn_sharing),
            ("Instance Stop Sharing", test_instance_stop_sharing),
            ("Flag Submission Sharing", test_flag_submission_sharing),
        ]

        for name, test_fn in tests:
            try:
                test_fn(context_a, context_b)
            except Exception as e:
                print(f"\n  ✗ FAILED: {name}")
                print(f"    Error: {e}")
                # Screenshots for debugging
                for label, ctx in [("A", context_a), ("B", context_b)]:
                    pg = ctx.pages[0] if ctx.pages else None
                    if pg:
                        path = f"/tmp/team-test-fail-{name.lower().replace(' ', '-')}-user{label}.png"
                        try:
                            pg.screenshot(path=path, timeout=5000)
                            print(f"    Screenshot {label}: {path}")
                        except Exception:
                            pass
                failed.append(name)

        # Cleanup
        log_section("CLEANUP")
        cleanup_test_instance()
        cleanup_test_data()
        log("Test data cleaned up")

        browser.close()

    print(f"\n{'='*60}")
    if failed:
        print(f"  RESULT: {len(failed)} test(s) FAILED: {', '.join(failed)}")
        print(f"{'='*60}\n")
        sys.exit(1)
    else:
        print(f"  RESULT: All team sharing tests PASSED ✓")
        print(f"{'='*60}\n")
        sys.exit(0)


if __name__ == "__main__":
    main()
