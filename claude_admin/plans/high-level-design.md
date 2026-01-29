# Claude Admin - High Level Design

## Vision

A terminal-based management system for Claude Code sessions that provides:
- Centralized monitoring of multiple Claude sessions
- Smart notifications when attention is needed
- Streamlined feature development workflow with git worktrees
- Commit-based code review integration

---

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Developer Workflow                             │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
            ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
            │   Worktree  │ │   Worktree  │ │   Worktree  │
            │  feature/A  │ │  feature/B  │ │  feature/C  │
            │             │ │             │ │             │
            │  ┌───────┐  │ │  ┌───────┐  │ │  ┌───────┐  │
            │  │Claude │  │ │  │Claude │  │ │  │Claude │  │
            │  │Session│  │ │  │Session│  │ │  │Session│  │
            │  └───┬───┘  │ │  └───┬───┘  │ │  └───┬───┘  │
            └──────┼──────┘ └──────┼──────┘ └──────┼──────┘
                   │               │               │
                   └───────────────┼───────────────┘
                                   │
                                   ▼
                    ┌──────────────────────────────┐
                    │        claude-admind         │
                    │         (Daemon)             │
                    │                              │
                    │  • Session Registry          │
                    │  • State Monitor             │
                    │  • Notification Engine       │
                    │  • Review Coordinator        │
                    └──────────────┬───────────────┘
                                   │
                          ┌────────┴────────┐
                          ▼                 ▼
               ┌──────────────────┐  ┌──────────────────┐
               │   claude-admin   │  │  macOS Notifs    │
               │     (TUI)        │  │                  │
               │                  │  │  "Session A      │
               │  Interactive     │  │   needs input"   │
               │  Management      │  │                  │
               └──────────────────┘  └──────────────────┘
```

---

## Core Components

### 1. Daemon (`claude-admind`)

Central background service that:
- Tracks all Claude sessions across tmux
- Monitors session output for state changes
- Manages session lifecycle
- Sends notifications
- Coordinates code reviews

```
┌─────────────────────────────────────────────────────────┐
│                     claude-admind                        │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │   Session   │  │    State    │  │   Review    │     │
│  │  Registry   │  │   Monitor   │  │ Coordinator │     │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘     │
│         │                │                │             │
│         └────────────────┼────────────────┘             │
│                          ▼                              │
│                 ┌─────────────────┐                     │
│                 │  Event Bus      │                     │
│                 └────────┬────────┘                     │
│                          │                              │
│         ┌────────────────┼────────────────┐             │
│         ▼                ▼                ▼             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │  IPC Server │  │   Notifier  │  │   Tmux      │     │
│  │  (socket)   │  │   (macOS)   │  │   Control   │     │
│  └─────────────┘  └─────────────┘  └─────────────┘     │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 2. TUI Client (`claude-admin`)

Interactive terminal interface:

```
┌─────────────────────────────────────────────────────────────────┐
│  claude-admin                                    ◉ daemon: OK   │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  SESSIONS                              │  OUTPUT PREVIEW        │
│  ════════                              │                        │
│                                        │  Implementing OAuth    │
│  my-project/                           │  flow with Google...   │
│  ├─ [●] feature/auth    WORKING        │                        │
│  ├─ [!] feature/api     NEEDS INPUT    │  > Created file:       │
│  └─ [✓] feature/ui      REVIEW         │    src/auth/google.rs  │
│                                        │                        │
│  other-repo/                           │  Waiting for review    │
│  └─ [ ] main            IDLE           │  of commit abc123...   │
│                                        │                        │
│                                        │                        │
├─────────────────────────────────────────────────────────────────┤
│  [n]ew  [a]ttach  [r]eview  [k]ill  [q]uit     3 active, 1 review│
└─────────────────────────────────────────────────────────────────┘
```

### 3. Feature Workflow (`/feature`)

```
┌────────────────────────────────────────────────────────────────┐
│                    /feature auth "Add OAuth"                    │
└────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌──────────────────┐
                    │  Create Worktree │
                    │  feature/auth    │
                    └────────┬─────────┘
                              │
                              ▼
                    ┌──────────────────┐
                    │  Create Tmux     │
                    │  Window          │
                    └────────┬─────────┘
                              │
                              ▼
                    ┌──────────────────┐
                    │  Start Claude    │
                    │  with context    │
                    └────────┬─────────┘
                              │
                              ▼
                    ┌──────────────────┐
                    │  Register with   │
                    │  Daemon          │
                    └──────────────────┘
```

---

## Session States

```
                    ┌─────────┐
                    │  IDLE   │
                    └────┬────┘
                         │ user input / task started
                         ▼
                    ┌─────────┐
            ┌──────►│ WORKING │◄──────┐
            │       └────┬────┘       │
            │            │            │
            │     ┌──────┴──────┐     │
            │     ▼             ▼     │
       ┌─────────────┐   ┌──────────┐ │
       │ NEEDS_INPUT │   │  REVIEW  │─┘ review approved
       └──────┬──────┘   └────┬─────┘
              │               │
              │ user responds │ all commits reviewed
              └───────┬───────┘
                      ▼
                 ┌─────────┐
                 │  DONE   │
                 └─────────┘
```

---

## Code Review Workflow

Key insight: Claude works best when changes are split into focused commits. The review system supports reviewing each commit independently.

```
┌─────────────────────────────────────────────────────────────────┐
│                    Code Review Flow                              │
└─────────────────────────────────────────────────────────────────┘

Developer assigns task to Claude
              │
              ▼
┌──────────────────────────────────────────────────────────────────┐
│  Claude works on feature, creating commits:                      │
│                                                                  │
│    commit 1: "Add user model"                                    │
│    commit 2: "Add auth middleware"                               │
│    commit 3: "Add login endpoint"                                │
│    commit 4: "Add tests"                                         │
│                                                                  │
│  Claude signals: READY_FOR_REVIEW                                │
└──────────────────────────────────────────────────────────────────┘
              │
              ▼
┌──────────────────────────────────────────────────────────────────┐
│  Daemon notifies: "feature/auth ready for review (4 commits)"   │
└──────────────────────────────────────────────────────────────────┘
              │
              ▼
┌──────────────────────────────────────────────────────────────────┐
│  Developer opens review in TUI:                                  │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  REVIEW: feature/auth                                       │ │
│  │                                                             │ │
│  │  Commits:                                                   │ │
│  │  [1/4] ○ Add user model          [d]iff  [a]pprove  [c]omment│
│  │  [2/4] ○ Add auth middleware                                │ │
│  │  [3/4] ○ Add login endpoint                                 │ │
│  │  [4/4] ○ Add tests                                          │ │
│  │                                                             │ │
│  │  [Enter] Review selected  [A] Approve all  [q] Back         │ │
│  └────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
              │
              │ For each commit with comments:
              ▼
┌──────────────────────────────────────────────────────────────────┐
│  Comments sent back to Claude session:                           │
│                                                                  │
│    "Review feedback for commit 'Add user model':                 │
│     - Consider adding email validation                           │
│     - Missing index on user.email field"                         │
│                                                                  │
│  Claude addresses feedback, amends or adds fixup commits         │
└──────────────────────────────────────────────────────────────────┘
              │
              │ Cycle until all approved
              ▼
┌──────────────────────────────────────────────────────────────────┐
│  All commits approved → Session moves to DONE                    │
└──────────────────────────────────────────────────────────────────┘
```

### Review Data Model

```
Review {
    session_id: SessionId
    branch: String
    commits: Vec<CommitReview>
    status: ReviewStatus  // Pending | InProgress | Approved | ChangesRequested
}

CommitReview {
    sha: String
    message: String
    status: CommitStatus  // Pending | Approved | ChangesRequested
    comments: Vec<ReviewComment>
}

ReviewComment {
    file: Option<String>
    line: Option<u32>
    body: String
    resolved: bool
}
```

---

## Notification System

```
┌─────────────────────────────────────────────────────────────────┐
│                    Notification Events                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  State Change Events:                                            │
│  ───────────────────                                             │
│  • WORKING → NEEDS_INPUT    "Session X needs your input"         │
│  • WORKING → REVIEW         "Session X ready for review"         │
│  • WORKING → DONE           "Session X completed"                │
│  • WORKING → ERROR          "Session X encountered an error"     │
│                                                                  │
│  Delivery:                                                       │
│  ─────────                                                       │
│  • macOS native notification (terminal-notifier / osascript)     │
│  • Optional: sound alerts                                        │
│  • TUI status bar updates                                        │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Communication Protocols

### Daemon ↔ TUI (IPC)

Unix domain socket at `~/.claude-admin/daemon.sock`

```
Messages:
  TUI → Daemon:
    - ListSessions
    - CreateSession { repo, branch, context }
    - AttachSession { id }
    - KillSession { id }
    - GetReview { session_id }
    - SubmitReviewComment { session_id, commit_sha, comment }
    - ApproveCommit { session_id, commit_sha }

  Daemon → TUI:
    - SessionList { sessions }
    - SessionUpdate { id, state, ... }
    - ReviewData { review }
    - Error { message }
```

### Daemon ↔ Claude Session

Communication via:
1. **Output monitoring** - daemon watches tmux pane output
2. **Marker patterns** - Claude outputs special markers that daemon recognizes
3. **File-based** - Claude writes to `~/.claude-admin/sessions/{id}/` for structured data

---

## Editor Integration

### Overview

Support external editors (VSCode, vim) for code review and quick edits. Editor is configurable in settings.

```
┌─────────────────────────────────────────────────────────────────┐
│                    Editor Integration Flow                       │
└─────────────────────────────────────────────────────────────────┘

                    ┌──────────────────┐
                    │   claude-admin   │
                    │      (TUI)       │
                    └────────┬─────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
              ▼              ▼              ▼
       ┌───────────┐  ┌───────────┐  ┌───────────┐
       │  VSCode   │  │    Vim    │  │  (other)  │
       │           │  │           │  │           │
       └───────────┘  └───────────┘  └───────────┘
```

### VSCode Integration

```
┌─────────────────────────────────────────────────────────────────┐
│                     VSCode Workflow                              │
└─────────────────────────────────────────────────────────────────┘

  Developer selects session in TUI
              │
              │  press [e] to edit / [r] to review
              ▼
  ┌──────────────────────────────────────────────────────────────┐
  │  1. Open worktree folder in VSCode                            │
  │     `code --reuse-window /path/to/worktree`                   │
  │                                                               │
  │  2. Open Source Control view with changes                     │
  │     - Show diff of commits ready for review                   │
  │     - Or open specific files                                  │
  │                                                               │
  │  3. Developer adds review comments                            │
  │     - Inline comments in code (special syntax)                │
  │     - Or via VSCode extension / comment file                  │
  └──────────────────────────────────────────────────────────────┘
              │
              ▼
  ┌──────────────────────────────────────────────────────────────┐
  │  Review Comments Format (in code):                            │
  │                                                               │
  │    // @claude: Consider using a HashMap here for O(1) lookup  │
  │    // @claude: This needs error handling for the None case    │
  │    // @claude: Extract this into a separate function          │
  │                                                               │
  │  Or in dedicated file `.claude-review`:                       │
  │                                                               │
  │    src/auth.rs:42: Add rate limiting here                     │
  │    src/user.rs:15-20: This block should be async              │
  │                                                               │
  └──────────────────────────────────────────────────────────────┘
              │
              │  Developer closes review / runs command
              ▼
  ┌──────────────────────────────────────────────────────────────┐
  │  claude-admin detects review comments                         │
  │                                                               │
  │  1. Scans for @claude: comments in changed files              │
  │  2. Reads .claude-review file if present                      │
  │  3. Sends feedback to Claude session                          │
  │  4. Claude addresses each comment                             │
  │  5. Comments removed after addressed (or marked resolved)     │
  └──────────────────────────────────────────────────────────────┘
```

### Editor Commands from TUI

| Key | Action |
|-----|--------|
| `e` | Open worktree in editor |
| `r` | Open review mode (diff view + ready for comments) |
| `d` | Open diff of selected commit in editor |

### Configuration

```toml
# ~/.claude-admin/config.toml

[editor]
default = "vscode"  # or "vim", "nvim", "code-insiders"

[editor.vscode]
command = "code"
args = ["--reuse-window"]
review_extension = true  # use VSCode extension if available

[editor.vim]
command = "nvim"
diff_tool = "vimdiff"

[review]
comment_prefix = "@claude:"
review_file = ".claude-review"
```

### Review Comment Syntax

**Inline comments** (in source files):
```rust
fn process_user(user: User) -> Result<(), Error> {
    // @claude: Add validation for user.email format
    let email = user.email;

    // @claude: This should handle the case where user is already processed
    self.users.insert(user.id, user);

    Ok(())
}
```

**Review file** (`.claude-review` in worktree root):
```
# Review comments for Claude
# Format: file:line[-end_line]: comment

src/auth.rs:42: Add rate limiting to prevent brute force
src/auth.rs:67-75: Extract token validation to separate function
src/models/user.rs:15: Missing derive for Serialize
general: Consider adding integration tests for the auth flow
```

### Session History (Future)

```
┌─────────────────────────────────────────────────────────────────┐
│                     Session History                              │
└─────────────────────────────────────────────────────────────────┘

~/.claude-admin/history/
├── sessions.db           # SQLite database
└── logs/
    ├── session-abc123/
    │   ├── output.log    # Full Claude output
    │   ├── commits.json  # Commits created
    │   └── reviews.json  # Review history
    └── ...

History tracks:
- Session start/end times
- Task description
- Commits created
- Review cycles (comments → fixes)
- Final outcome (merged, abandoned, etc.)
- Learnings / patterns (manual notes)
```

---

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Review granularity | Per-commit | Matches how Claude should structure work; focused reviews |
| Review signaling | Explicit command | Claude runs `claude-admin review` when ready |
| Daemon scope | Global | One daemon per machine manages all repos |

---

## Review Signaling

Claude explicitly signals review readiness via CLI:

```bash
# Claude runs this when ready for review
claude-admin review --commits HEAD~3..HEAD --message "Auth feature ready"

# Or to request review of specific commits
claude-admin review --commits abc123,def456
```

This command:
1. Notifies daemon of review request
2. Daemon changes session state to REVIEW
3. macOS notification sent to developer
4. Session pauses until review completes or continues with feedback
