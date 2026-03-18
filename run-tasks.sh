#!/usr/bin/env bash
set -euo pipefail

TEAM="JEM"
PROJECT="Ishi"
LOG_DIR="./task-logs"
mkdir -p "$LOG_DIR"

while true; do
  TIMESTAMP=$(date +%Y%m%d-%H%M%S)
  PICK_LOG="$LOG_DIR/pick-$TIMESTAMP.log"

  echo "=== $(date) === Picking next task..."

  amp --dangerously-allow-all -x "
List issues in Linear team '$TEAM', project '$PROJECT' with state 'Backlog'.
Pick the single highest-priority one (or the first if all equal priority).
Respond with ONLY a single line in this exact format:
TASK:<issue_identifier>|<issue_title>
If there are no Backlog issues, respond with exactly: NO_TASKS
Do not include any other text.
" --stream-json 2>"$PICK_LOG.err" \
    | tee "$PICK_LOG" \
    | jq --unbuffered -r 'select(.type=="assistant") | .message.content[]? | select(.type=="text") | .text // empty'

  PICK_RESULT=$(jq -r 'select(.type=="assistant") | .message.content[]? | select(.type=="text") | .text // empty' "$PICK_LOG")

  if echo "$PICK_RESULT" | grep -q "NO_TASKS"; then
    echo "=== $(date) === No backlog tasks remaining. Exiting loop."
    break
  fi

  TASK_LINE=$(echo "$PICK_RESULT" | grep "^TASK:" | head -1)
  if [ -z "$TASK_LINE" ]; then
    echo "=== $(date) === Could not parse task from response. Retrying..."
    sleep 5
    continue
  fi

  TASK_ID=$(echo "$TASK_LINE" | sed 's/^TASK:\([^|]*\)|.*/\1/')
  TASK_TITLE=$(echo "$TASK_LINE" | sed 's/^TASK:[^|]*|//')
  IMPL_LOG="$LOG_DIR/impl-${TASK_ID}-$TIMESTAMP.log"

  echo "=== $(date) === Working on $TASK_ID: $TASK_TITLE"

  amp --dangerously-allow-all -x "
You are implementing a task for the ishi project (a Rust TUI app for Linear).

## Your task
Linear issue: $TASK_ID — $TASK_TITLE

## Instructions
1. Fetch the full description of Linear issue $TASK_ID (team '$TEAM') and read it carefully.
2. Set the issue state to 'In Progress'.
3. Read the existing codebase to understand conventions, structure and patterns.
4. Implement the feature or fix described in the issue.
5. Write thorough tests for your changes.
6. Run the full test suite with 'cargo test' and fix any failures until all tests pass.
7. Run 'cargo clippy' and fix any warnings.
8. If you discover additional work needed (bugs, missing features, TODOs), create new Linear issues in team '$TEAM' project '$PROJECT' for each.
9. When implementation is complete and all tests pass:
   a. Run: jj describe -m '[$TASK_ID] $TASK_TITLE'
   b. Run: jj git push
   c. Run: jj new
10. Set the Linear issue $TASK_ID state to 'Done'.
11. Print a brief summary of what you implemented.
" --stream-json 2>"$IMPL_LOG.err" \
    | tee "$IMPL_LOG" \
    | jq --unbuffered -r 'select(.type=="assistant") | .message.content[]? | select(.type=="text") | .text // empty'

  echo "=== $(date) === Finished $TASK_ID. Looping..."
  sleep 5
done

echo "All tasks complete! ✅"
