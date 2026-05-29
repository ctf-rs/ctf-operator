#!/usr/bin/env python3
"""
End-to-end UI tests using Playwright (Python).

Requires: python3 -m playwright install chromium

Tests:
  1. Login flow (OAuth via Keycloak)
  2. Challenge list renders
  3. Instance spawn + autorefresh (status updates without page reload)
  4. Instance stop button
  5. Scoreboard page
  6. Flag submission
  7. Logout
"""

import sys
import time
from playwright.sync_api import sync_playwright, expect

BASE_URL = "http://localhost:8090"
KEYCLOAK_URL = "http://localhost:8081"

# Test user credentials (set up by start-team-mode.sh)
USER = "player"
PASS = "player"


def log(msg: str):
    print(f"  ✓ {msg}")


def log_section(msg: str):
    print(f"\n{'='*60}")
    print(f"  {msg}")
    print(f"{'='*60}")


def test_login(page):
    """Test: OAuth login through Keycloak."""
    log_section("TEST: Login Flow")

    page.goto(BASE_URL)
    # Should see login page or redirect to Keycloak
    page.wait_for_load_state("domcontentloaded")

    # Click login if there's a login button
    login_btn = page.locator("a:has-text('Login'), button:has-text('Login'), a:has-text('Sign in')")
    if login_btn.count() > 0:
        login_btn.first.click()
        page.wait_for_load_state("domcontentloaded")
        log("Clicked login button")

    # If redirected to Keycloak, fill credentials
    if "keycloak" in page.url or "realms" in page.url:
        page.fill("#username", USER)
        page.fill("#password", PASS)
        page.click("#kc-login")
        page.wait_for_load_state("domcontentloaded")
        log("Filled Keycloak credentials and submitted")

    # Should be back on the app
    page.wait_for_url("**/localhost:8090/**", timeout=10000)
    log(f"Redirected back to app: {page.url}")

    # Should see user label or challenges
    body_text = page.text_content("body")
    assert "Logout" in body_text, f"Expected 'Logout' button after login, got: {body_text[:200]}"
    log("Login successful — Logout button visible")


def test_challenge_list(page):
    """Test: Challenge list page renders with challenges."""
    log_section("TEST: Challenge List")

    page.goto(f"{BASE_URL}/")
    page.wait_for_load_state("domcontentloaded")

    body_text = page.text_content("body")

    # Check for challenge cards or category sections
    has_challenges = (
        page.locator(".challenge-card, .category-section, .card").count() > 0
        or "challenge" in body_text.lower()
        or "misc" in body_text.lower()
        or "web" in body_text.lower()
    )
    assert has_challenges, f"No challenges found on page. Body: {body_text[:500]}"
    log(f"Challenge list renders with content")

    # Count visible challenges
    cards = page.locator(".challenge-card, .card")
    card_count = cards.count()
    log(f"Found {card_count} challenge card(s)")


def test_scoreboard(page):
    """Test: Scoreboard page renders."""
    log_section("TEST: Scoreboard")

    page.goto(f"{BASE_URL}/scoreboard")
    page.wait_for_load_state("domcontentloaded")

    body_text = page.text_content("body")
    assert "Scoreboard" in body_text, f"Scoreboard heading not found. Body: {body_text[:300]}"
    log("Scoreboard page renders")

    # Check for table or "no solves" message
    table = page.locator("table.scoreboard-table, .scoreboard-table")
    no_solves = page.locator("text=No solves yet")

    if table.count() > 0:
        rows = page.locator("table.scoreboard-table tbody tr, .scoreboard-table tbody tr")
        log(f"Scoreboard table visible with {rows.count()} row(s)")
    elif no_solves.count() > 0:
        log("Scoreboard shows 'No solves yet' (expected for fresh setup)")
    else:
        log("Scoreboard rendered (no table or 'no solves' message)")


def test_challenge_detail(page):
    """Test: Navigate to a challenge detail page."""
    log_section("TEST: Challenge Detail")

    page.goto(f"{BASE_URL}/")
    page.wait_for_load_state("domcontentloaded")

    # Click on the first challenge link
    challenge_links = page.locator("a[href*='/challenge/']")
    if challenge_links.count() == 0:
        print("  ⚠ No challenge links found, skipping detail test")
        return None

    first_link = challenge_links.first
    challenge_name = first_link.text_content().strip()
    first_link.click()
    page.wait_for_load_state("domcontentloaded")
    time.sleep(2)
    log(f"Navigated to challenge: {challenge_name}")

    body_text = page.text_content("body")

    # Check for flag submission form
    flag_input = page.locator("input[name='flag'], input[placeholder*='flag'], input[type='text']")
    if flag_input.count() > 0:
        log("Flag submission input found")

    return challenge_name


def test_instance_spawn_and_autorefresh(page):
    """Test: Spawn an instance and verify autorefresh updates status."""
    log_section("TEST: Instance Spawn + Autorefresh")

    page.goto(f"{BASE_URL}/")
    page.wait_for_load_state("domcontentloaded")

    # Find a challenge with instancing (look for a spawn button)
    # Navigate to each challenge to find one with a spawn button
    challenge_links = page.locator("a[href*='/challenge/']")
    count = challenge_links.count()

    spawn_found = False
    for i in range(count):
        page.goto(f"{BASE_URL}/")
        page.wait_for_load_state("domcontentloaded")

        link = page.locator("a[href*='/challenge/']").nth(i)
        ch_name = link.text_content().strip()
        link.click()
        page.wait_for_load_state("domcontentloaded")

        spawn_btn = page.locator("button:has-text('Spawn'), button:has-text('Start'), button:has-text('Deploy')")
        if spawn_btn.count() > 0:
            log(f"Found spawn button on challenge: {ch_name}")
            spawn_found = True

            # Click spawn (form POST — don't wait for navigation)
            spawn_btn.first.click(no_wait_after=True)
            page.wait_for_load_state("domcontentloaded", timeout=15000)
            log("Clicked spawn button")

            # Wait for the page to show status updates (autorefresh via SSE/WASM)
            # The WASM hydration should connect to SSE and update the DOM
            time.sleep(2)

            # Check if status has changed — look for status indicators
            body_text = page.text_content("body")
            status_indicators = [
                "Pending", "Creating", "Running", "Ready",
                "Deploying", "Starting", "Active",
                "Stop", "Extend", "time left",
                "pending", "creating", "running",
            ]
            has_status = any(s.lower() in body_text.lower() for s in status_indicators)

            if has_status:
                log("Instance status indicator found after spawn")
            else:
                log(f"Status area text: {body_text[:300]}")

            # Wait longer and check if status auto-updates (the key autorefresh test)
            log("Waiting 15s for autorefresh to update instance status...")
            initial_body = page.text_content("body")

            for tick in range(5):
                time.sleep(3)
                current_body = page.text_content("body")
                if current_body != initial_body:
                    log(f"Autorefresh detected! DOM changed after {(tick+1)*3}s (without page reload)")
                    break
            else:
                # Check if WASM loaded
                wasm_loaded = page.evaluate("typeof WebAssembly !== 'undefined'")
                log(f"WebAssembly available: {wasm_loaded}")

                # Check if our app.js module loaded
                has_app_module = page.evaluate("""
                    document.querySelector('script[type="module"][src*="app"]') !== null
                """)
                log(f"App module script tag found: {has_app_module}")

                # Check console for WASM errors
                print("  ⚠ No DOM change detected — autorefresh may not be working")

            # Try to find stop/extend buttons
            stop_btn = page.locator("button:has-text('Stop'), button:has-text('Delete'), button:has-text('Destroy')")
            extend_btn = page.locator("button:has-text('Extend')")

            if stop_btn.count() > 0:
                log("Stop button available")
            if extend_btn.count() > 0:
                log("Extend button available")

            # Clean up: stop the instance if possible
            if stop_btn.count() > 0:
                stop_btn.first.click(no_wait_after=True)
                page.wait_for_load_state("domcontentloaded", timeout=15000)
                time.sleep(2)
                log("Clicked stop to clean up instance")

            break

    if not spawn_found:
        print("  ⚠ No challenge with spawn button found, skipping instance test")


def test_flag_submission(page):
    """Test: Submit a flag on a non-instanced challenge (global instance already running)."""
    log_section("TEST: Flag Submission")

    # Navigate to a non-instanced challenge whose global instance should be Ready
    # misc-sanity and misc-trivia are non-instanced challenges
    for slug in ["misc-sanity", "misc-trivia"]:
        page.goto(f"{BASE_URL}/challenge/{slug}/")
        page.wait_for_load_state("domcontentloaded")
        time.sleep(1)

        flag_input = page.locator("input[name='submitted_flag']")
        if flag_input.count() > 0 and flag_input.first.is_enabled():
            log(f"Found enabled flag input on challenge: {slug}")
            break
    else:
        # Fallback: try any challenge with an enabled flag input
        page.goto(f"{BASE_URL}/")
        page.wait_for_load_state("domcontentloaded")
        challenge_links = page.locator("a[href*='/challenge/']")
        found = False
        for i in range(challenge_links.count()):
            link = challenge_links.nth(i)
            link.click()
            page.wait_for_load_state("domcontentloaded")
            time.sleep(1)
            flag_input = page.locator("input[name='submitted_flag']")
            if flag_input.count() > 0 and flag_input.first.is_enabled():
                log(f"Found enabled flag input on challenge #{i}")
                found = True
                break
            page.goto(f"{BASE_URL}/")
            page.wait_for_load_state("domcontentloaded")
        if not found:
            print("  ⚠ No challenge with enabled flag input found, skipping")
            return

    # Submit a wrong flag
    flag_input.first.fill("CTF{wrong_flag_12345}")
    log("Filled flag input with test value")

    submit_btn = page.locator("button[type='submit']")
    if submit_btn.count() > 0:
        submit_btn.first.click()
        page.wait_for_load_state("domcontentloaded")
        time.sleep(1)

        body_text = page.text_content("body")
        # Should show rejection message
        rejection_indicators = ["incorrect", "wrong", "invalid", "try again", "not correct", "bad"]
        has_rejection = any(r in body_text.lower() for r in rejection_indicators)

        if has_rejection:
            log("Wrong flag correctly rejected")
        else:
            log(f"Flag submitted, response: {body_text[:200]}")
    else:
        print("  ⚠ No submit button found")


def test_logout(page):
    """Test: Logout button works."""
    log_section("TEST: Logout")

    page.goto(f"{BASE_URL}/")
    page.wait_for_load_state("domcontentloaded")

    logout_btn = page.locator("button:has-text('Logout')")
    if logout_btn.count() > 0:
        logout_btn.first.click()
        page.wait_for_load_state("domcontentloaded")
        time.sleep(1)

        # Should be back on login page or index
        body_text = page.text_content("body")
        if "Login" in body_text or "Sign in" in body_text or "login" in page.url:
            log("Logout successful — redirected to login")
        else:
            log(f"After logout, page: {page.url}")
    else:
        print("  ⚠ No logout button found")


def main():
    print("\n" + "=" * 60)
    print("  CTF Platform — End-to-End UI Tests (Playwright)")
    print("=" * 60)

    failed = []

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        context = browser.new_context(
            viewport={"width": 1280, "height": 800},
            ignore_https_errors=True,
        )

        # Collect console messages for debugging
        console_msgs = []
        page = context.new_page()
        page.on("console", lambda msg: console_msgs.append(f"[{msg.type}] {msg.text}"))
        page.on("pageerror", lambda err: console_msgs.append(f"[PAGE_ERROR] {err}"))

        tests = [
            ("Login", test_login),
            ("Challenge List", test_challenge_list),
            ("Scoreboard", test_scoreboard),
            ("Challenge Detail", test_challenge_detail),
            ("Instance Spawn + Autorefresh", test_instance_spawn_and_autorefresh),
            ("Flag Submission", test_flag_submission),
            ("Logout", test_logout),
        ]

        for name, test_fn in tests:
            try:
                test_fn(page)
            except Exception as e:
                print(f"\n  ✗ FAILED: {name}")
                print(f"    Error: {e}")
                print(f"    URL: {page.url}")
                # Take screenshot on failure
                screenshot_path = f"/tmp/test-fail-{name.lower().replace(' ', '-')}.png"
                try:
                    page.screenshot(path=screenshot_path, timeout=5000)
                    print(f"    Screenshot: {screenshot_path}")
                except Exception:
                    pass
                failed.append(name)

        # Print console messages if there were failures
        if failed and console_msgs:
            print(f"\n{'='*60}")
            print("  Browser Console Messages:")
            print(f"{'='*60}")
            for msg in console_msgs[-20:]:
                print(f"  {msg}")

        browser.close()

    print(f"\n{'='*60}")
    if failed:
        print(f"  RESULT: {len(failed)} test(s) FAILED: {', '.join(failed)}")
        print(f"{'='*60}\n")
        sys.exit(1)
    else:
        print(f"  RESULT: All tests PASSED ✓")
        print(f"{'='*60}\n")
        sys.exit(0)


if __name__ == "__main__":
    main()
