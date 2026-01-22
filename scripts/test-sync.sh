#!/bin/bash
BASE_DIR=$(cd "$(dirname "$0")" && pwd)
SESSION="sync_test"

# Start a new session, detached
tmux new-session -d -s $SESSION

# Pane 1: The Sync Server
tmux send-keys -t $SESSION "cd $BASE_DIR/.. && cargo run -p diaryx_sync_server" C-m

# Pane 2: Split vertically for Client A
tmux split-window -h -t $SESSION
tmux send-keys -t $SESSION "cd $BASE_DIR/../apps/web && bun run dev --port 5174" C-m

# Pane 3: Split horizontally under Client A for Client B
tmux split-window -v -t $SESSION
tmux send-keys -t $SESSION "cd $BASE_DIR/../apps/web && bun run dev --port 5175" C-m

# Attach to the session
tmux attach-session -t $SESSION
