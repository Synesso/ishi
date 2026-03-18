#!/usr/bin/env bash
set -euo pipefail

TEAM="JEM"
PROJECT="Ishi"
LOG_DIR="./task-logs"
mkdir -p "$LOG_DIR"

while true; do
  TIMESTAMP=$(date +%Y%m%d-%H%M%S)
  LOG_FILE="$LOG_DIR/task-$TIMESTAMP.log"

  echo "=== $(date) === Checking for tasks..." | tee -a "$LOG_FILE"

  RESULT=$(amp --dangerously-allow-all -x "
You are running in an automated loop. Be thorough but focused.

1. List open issues in Linear team '$TEAM', project '$PROJECT' (state: unstarted or started, exclude done/cancelled).
2. If there are NO open tasks, respond with exactly: NO_TASKS_REMAINING
3. If there are tasks, pick ONE task — preferably the highest priority unstarted one.
4. Assign the task to yourself and set its state to 'In Progress'.
5. Read the task description carefully. Implement the feature or fix described.
6. Write tests for your changes. Run the full test suite and ensure all tests pass.
7. If you discover additional work needed (bugs, missing features, TODOs), create new Linear issues in team '$TEAM' project '$PROJECT' for each.
8. When implementation is complete and tests pass:
   a. Use 'jj describe -m \"[TICKET-ID] description of change\"' to describe the change.
   b. Push to the git remote with 'jj git push'.
9. Mark the Linear task as 'Done'.
10. Respond with a summary of what you did.
" 2>&1 | tee -a "$LOG_FILE")

  if echo "$RESULT" | grep -q "NO_TASKS_REMAINING"; then
    echo "=== $(date) === No tasks remaining. Exiting loop." | tee -a "$LOG_FILE"
    break
  fi

  echo "=== $(date) === Task completed. Looping..." | tee -a "$LOG_FILE"
  sleep 5
done

echo "All tasks complete! ✅"
