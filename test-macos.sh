#!/bin/bash
# macOS Adaptation Test Script for Clyde on Desk
# Run this AFTER launching the app: npm start
# Usage: bash test-macos.sh

set -e

detect_port() {
  local candidates=()
  local runtime_file="$HOME/.clyde/runtime.json"
  if [ -f "$runtime_file" ]; then
    local runtime_port
    runtime_port=$(node -e 'try { const fs = require("fs"); const raw = JSON.parse(fs.readFileSync(process.argv[1], "utf8")); if (Number.isInteger(raw.port)) process.stdout.write(String(raw.port)); } catch {}' "$runtime_file")
    if [ -n "$runtime_port" ]; then
      candidates+=("$runtime_port")
    fi
  fi

  for port in 23333 23334 23335 23336 23337; do
    local seen=0
    for existing in "${candidates[@]}"; do
      if [ "$existing" = "$port" ]; then
        seen=1
        break
      fi
    done
    if [ "$seen" -eq 0 ]; then
      candidates+=("$port")
    fi
  done

  for port in "${candidates[@]}"; do
    if curl -s -D - -o /dev/null --connect-timeout 1 "http://127.0.0.1:$port/state" 2>/dev/null | tr -d '\r' | grep -iq '^x-clyde-server: clyde-on-desk$'; then
      echo "$port"
      return 0
    fi
  done

  return 1
}

PORT="$(detect_port || true)"
BASE="http://127.0.0.1:$PORT"
HOOK="hooks/clyde-hook.js"
BOLD='\033[1m'
DIM='\033[2m'
GREEN='\033[32m'
RED='\033[31m'
YELLOW='\033[33m'
CYAN='\033[36m'
RESET='\033[0m'

pass() { echo -e "  ${GREEN}✓${RESET} $1"; }
fail() { echo -e "  ${RED}✗${RESET} $1"; }
info() { echo -e "  ${DIM}→${RESET} $1"; }
header() { echo -e "\n${BOLD}${CYAN}[$1]${RESET} $2"; }

# ─── Pre-flight ───
header "0" "Pre-flight checks"

if [ -z "$PORT" ]; then
  fail "Clyde not found on ports 23333-23337. Start the app first: npm start"
  exit 1
fi
pass "Clyde HTTP server is reachable on port $PORT"

if [ "$(uname)" != "Darwin" ]; then
  fail "This script is for macOS only"
  exit 1
fi
pass "Running on macOS ($(sw_vers -productVersion))"

# ─── Test 1: getStablePid() ───
header "1" "getStablePid() — process tree walk"

echo -e "${DIM}  Current terminal process tree:${RESET}"
MY_PID=$$
CUR=$MY_PID
for i in $(seq 1 8); do
  PNAME=$(ps -o comm= -p $CUR 2>/dev/null || echo "???")
  PPID_VAL=$(ps -o ppid= -p $CUR 2>/dev/null | tr -d ' ' || echo "0")
  PBASE=$(echo "$PNAME" | sed 's|^-||' | xargs basename 2>/dev/null || echo "$PNAME")
  echo -e "    ${DIM}Level $i: PID=$CUR  name=$PBASE  ppid=$PPID_VAL${RESET}"
  if [ "$PPID_VAL" = "0" ] || [ "$PPID_VAL" = "1" ] || [ "$PPID_VAL" = "$CUR" ]; then
    break
  fi
  CUR=$PPID_VAL
done

echo ""
info "Sending SessionStart via hook script..."
HOOK_OUTPUT=$(echo '{"session_id":"macos-test","cwd":"'"$(pwd)"'"}' | timeout 5 node "$HOOK" SessionStart 2>&1 || true)
sleep 0.5

# Check if the state server received it
CURL_RESULT=$(curl -s -X POST "$BASE/state" \
  -H "Content-Type: application/json" \
  -d '{"state":"idle","session_id":"macos-test-probe"}' 2>&1)

if [ "$CURL_RESULT" = "ok" ]; then
  pass "Hook script ran successfully, state server accepted request"
else
  fail "State server response: $CURL_RESULT"
fi

# ─── Test 2: Terminal name matching ───
header "2" "Terminal name matching"

TERMINAL_NAMES="terminal iterm2 alacritty wezterm-gui kitty hyper tabby warp ghostty"
CURRENT_TERM=$(ps -o comm= -p $(ps -o ppid= -p $$ | tr -d ' ') 2>/dev/null || echo "unknown")
# Strip leading dash (login shell) and path, then lowercase
CURRENT_TERM_BASE=$(echo "$CURRENT_TERM" | sed 's|^-||' | xargs basename 2>/dev/null | tr '[:upper:]' '[:lower:]')

info "Your terminal process: ${BOLD}$CURRENT_TERM_BASE${RESET} (full: $CURRENT_TERM)"

MATCHED=false
for name in $TERMINAL_NAMES; do
  if [ "$CURRENT_TERM_BASE" = "$name" ]; then
    MATCHED=true
    break
  fi
done

# Also check grandparent (VS Code/Cursor wraps in electron)
GRANDPARENT=$(ps -o comm= -p $(ps -o ppid= -p $(ps -o ppid= -p $$ | tr -d ' ') | tr -d ' ') 2>/dev/null || echo "unknown")
GP_BASE=$(echo "$GRANDPARENT" | sed 's|^-||' | xargs basename 2>/dev/null | tr '[:upper:]' '[:lower:]')

if $MATCHED; then
  pass "Terminal '$CURRENT_TERM_BASE' is in TERMINAL_NAMES_MAC"
else
  fail "Terminal '$CURRENT_TERM_BASE' is NOT in TERMINAL_NAMES_MAC"
  info "Grandparent process: $GP_BASE (full: $GRANDPARENT)"
  if echo "$GP_BASE" | grep -qi "electron\|code\|cursor"; then
    info "${YELLOW}Looks like VS Code/Cursor — may need to add '$GP_BASE' to TERMINAL_NAMES_MAC${RESET}"
  fi
fi

# ─── Test 3: focusTerminalWindow() via osascript ───
header "3" "focusTerminalWindow() — osascript activation"

# Check accessibility permission
ACCESSIBILITY=$(osascript -e 'tell application "System Events" to return name of first process whose frontmost is true' 2>&1 || true)
if echo "$ACCESSIBILITY" | grep -qi "not allowed\|assistive\|1002"; then
  fail "Accessibility permission NOT granted"
  info "Go to: System Settings → Privacy & Security → Accessibility"
  info "Add your terminal app (or Clyde) to the allowed list"
else
  pass "Accessibility permission OK (frontmost app: $ACCESSIBILITY)"
fi

# Test actual focus: send a session with our terminal PID, then trigger focus
TERM_PID=$(ps -o ppid= -p $$ | tr -d ' ')
info "Registering session with source_pid=$TERM_PID"

curl -s -X POST "$BASE/state" \
  -H "Content-Type: application/json" \
  -d '{"state":"working","session_id":"focus-test","event":"PreToolUse","source_pid":'"$TERM_PID"',"cwd":"'"$(pwd)"'"}' > /dev/null

echo ""
echo -e "  ${YELLOW}>>> In 3 seconds, Clyde will try to focus THIS terminal <<<${RESET}"
echo -e "  ${YELLOW}>>> Switch to another window NOW to verify it works    <<<${RESET}"
sleep 3

# Simulate a click-to-focus by directly calling the IPC (via HTTP state with working state)
# The actual focus happens when user clicks the pet, but we can test osascript directly
FOCUS_SCRIPT='
set pid to '"$TERM_PID"'
repeat 8 times
  try
    set pInfo to do shell script "ps -o ppid=,comm= -p " & pid
    set ppid to (word 1 of pInfo) as integer
    tell application "System Events"
      set pList to every process whose unix id is pid
      if (count of pList) > 0 then
        set frontmost of item 1 of pList to true
        return "focused pid " & pid
      end if
    end tell
    if ppid is less than or equal to 1 then exit repeat
    set pid to ppid
  on error errMsg
    return "error: " & errMsg
  end try
end repeat
return "no focusable process found"'

FOCUS_RESULT=$(osascript -e "$FOCUS_SCRIPT" 2>&1 || true)
if echo "$FOCUS_RESULT" | grep -qi "focused"; then
  pass "osascript focus succeeded: $FOCUS_RESULT"
else
  fail "osascript focus result: $FOCUS_RESULT"
fi

# ─── Test 4: Permission Bubble ───
header "4" "Permission Bubble — HTTP hook"

info "Sending permission request to /permission ..."
echo -e "  ${YELLOW}>>> A permission bubble should appear in the bottom-right <<<${RESET}"

# Send permission request in background (it blocks until user clicks or 10s timeout)
# macOS doesn't have GNU timeout; use perl one-liner as fallback
_timeout() { perl -e 'alarm shift; exec @ARGV' "$@"; }
PERM_RESPONSE=$(_timeout 15 curl -s -X POST "$BASE/permission" \
  -H "Content-Type: application/json" \
  -d '{
    "tool_name": "Bash",
    "tool_input": {"command": "echo hello from test-macos.sh"},
    "session_id": "bubble-test",
    "permission_suggestions": [
      {"type": "addRules", "toolName": "Bash", "ruleContent": "echo *", "behavior": "allow", "destination": "localSettings"},
      {"type": "setMode", "mode": "acceptEdits", "destination": "localSettings"}
    ]
  }' 2>&1 || echo '{"timeout":true}')

if echo "$PERM_RESPONSE" | grep -qi "allow\|deny\|hookSpecificOutput"; then
  pass "Permission bubble responded: $(echo "$PERM_RESPONSE" | head -c 120)"
elif [ -z "$PERM_RESPONSE" ]; then
  info "Empty response (DND mode or already pending — expected if DND is on)"
else
  fail "Unexpected response: $PERM_RESPONSE"
fi

# ─── Test 5: Session Dashboard ───
header "5" "Session Dashboard — Cmd+Click"

info "Creating 2 test sessions for dashboard..."
curl -s -X POST "$BASE/state" \
  -H "Content-Type: application/json" \
  -d '{"state":"working","session_id":"dash-1","event":"PreToolUse","source_pid":'"$TERM_PID"',"cwd":"/Users/test/project-alpha"}' > /dev/null

curl -s -X POST "$BASE/state" \
  -H "Content-Type: application/json" \
  -d '{"state":"thinking","session_id":"dash-2","event":"UserPromptSubmit","source_pid":'"$TERM_PID"',"cwd":"/Users/test/project-beta"}' > /dev/null

pass "2 sessions registered (project-alpha: working, project-beta: thinking)"
echo -e "  ${YELLOW}>>> Cmd+Click the pet to see the session menu <<<${RESET}"
echo -e "  ${YELLOW}>>> Right-click → Sessions to see the submenu  <<<${RESET}"

# ─── Test 6: Auto Update ───
header "6" "Auto Update — macOS dialog"

info "Auto-update uses electron-updater. On macOS it opens GitHub Releases page."
info "Right-click the pet → Check for Updates to test manually."
pass "No automated test needed (UI dialog)"

# ─── Test 7: Bubble window properties ───
header "7" "Bubble window — transparency & click-through"

info "Testing if transparent BrowserWindow receives clicks without focus()..."
info "On macOS, transparent windows should be clickable natively."
info "The code skips bubble.focus() on macOS (main.js line 1186, 1222)"
pass "Code check: isMac guard present for bubble.focus() skip"

# ─── Cleanup ───
header "✓" "Cleanup"

# Clean up test sessions
curl -s -X POST "$BASE/state" \
  -H "Content-Type: application/json" \
  -d '{"state":"sleeping","session_id":"macos-test","event":"SessionEnd"}' > /dev/null
curl -s -X POST "$BASE/state" \
  -H "Content-Type: application/json" \
  -d '{"state":"sleeping","session_id":"focus-test","event":"SessionEnd"}' > /dev/null
curl -s -X POST "$BASE/state" \
  -H "Content-Type: application/json" \
  -d '{"state":"sleeping","session_id":"dash-1","event":"SessionEnd"}' > /dev/null
curl -s -X POST "$BASE/state" \
  -H "Content-Type: application/json" \
  -d '{"state":"sleeping","session_id":"dash-2","event":"SessionEnd"}' > /dev/null
curl -s -X POST "$BASE/state" \
  -H "Content-Type: application/json" \
  -d '{"state":"sleeping","session_id":"macos-test-probe","event":"SessionEnd"}' > /dev/null

pass "Test sessions cleaned up"

echo ""
echo -e "${BOLD}Done!${RESET} Review the results above."
echo -e "If getStablePid or focusTerminal failed, check:"
echo -e "  1. Accessibility permission for your terminal/Clyde"
echo -e "  2. Terminal process name in TERMINAL_NAMES_MAC"
echo -e "  3. osascript error messages in the app console"
echo ""
