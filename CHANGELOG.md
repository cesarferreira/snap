# Changelog

All notable changes to this project will be documented in this file.

## [0.6.0] - 2026-09-03

### 🚀 Features

- Animate window transitions ([#36](https://github.com/cesarferreira/snap/issues/36))

### 💼 Other

- Updated readme
- Revise README formatting and content
- Update README.md
- Clean up README by removing commented-out section
- Fix installation commands in README.md
- No error when focus out of place

### 🚜 Refactor

- Remove daemon and last command ([#35](https://github.com/cesarferreira/snap/issues/35))
## [0.5.0] - 2026-09-02

### 🚀 Features

- Add on-disk focus-history cache for snap last
- Resolve (pid, window_number) back to a live AXUIElement
- Add AXObserver wrapper for focused-window-changed notifications
- Add focus-watch daemon (NSWorkspace + AXObserver)
- Add launchd install/uninstall/status for the focus-watch daemon
- Add 'snap last' and 'snap daemon install/uninstall/run' to the CLI
- Wire up 'snap last' and 'snap daemon' commands

### 🐛 Bug Fixes

- Initialize NSApplication so NSWorkspace app-switch notifications fire
- Snap last reports when the daemon isn't running, not a stale error
- Fix

### 💼 Other

- Add block2 and NSNotification/NSOperation features for focus-watch daemon
- Progress
- Error logging
- Daemon
- Updated readme
- Updated readme
- Update README.md

### 📚 Documentation

- Add design spec for snap last focus-history daemon
- Add implementation plan for snap last
- Document snap last and the opt-in focus-history daemon

### 🎨 Styling

- Cargo fmt focus_watch.rs

### ⚙️ Miscellaneous Tasks

- Ignore .worktrees/ directory
- Remove planning docs for snap last
## [0.3.0] - 2026-09-02

### 🚀 Features

- Auto-publish to homebrew-tap on release; fix README install docs

### 🐛 Bug Fixes

- Migrate AppKit/AX off cocoa and objc to drop the block future-incompat warning

### 📚 Documentation

- Trim CI trivia from README install section

### 🎨 Styling

- Cargo fmt
## [0.2.0] - 2026-09-02

### 🚀 Features

- Cycle 25/50/75% when SIZE is omitted
- Add snap display next/previous/N ([#1](https://github.com/cesarferreira/snap/issues/1))
- Allow arbitrary 1-100 percent sizes ([#5](https://github.com/cesarferreira/snap/issues/5))
- Add corner anchors top-left/top-right/bottom-left/bottom-right ([#2](https://github.com/cesarferreira/snap/issues/2))
- Add snap third left/center/right with cycle ([#7](https://github.com/cesarferreira/snap/issues/7))
- Add snap grow/shrink/almost ([#6](https://github.com/cesarferreira/snap/issues/6))
- Add snap list ([#3](https://github.com/cesarferreira/snap/issues/3))
- Add snap tile columns/rows/master variants ([#8](https://github.com/cesarferreira/snap/issues/8))
- Add snap focus and shared spatial neighbor picker ([#13](https://github.com/cesarferreira/snap/issues/13))
- Add snap swap left/right/up/down ([#12](https://github.com/cesarferreira/snap/issues/12))
- Add snap stack accordion ([#14](https://github.com/cesarferreira/snap/issues/14))
- Add --app to target a window by application name ([#4](https://github.com/cesarferreira/snap/issues/4))
- Add snap undo with on-disk last-rect cache ([#9](https://github.com/cesarferreira/snap/issues/9))
- Add snap doctor diagnostic command ([#10](https://github.com/cesarferreira/snap/issues/10))

### 🐛 Bug Fixes

- Avoid partially moving windows that can't be resized
- Enter the size cycle at 50% instead of 25%
- Widen cross-source rect-match tolerance to 2.0

### 💼 Other

- First commit
- Tiling works
- Tiling padding
- Stage manager padding
- Remove Development section from README
- Renamed package
- Add emoji to project title in README
- Example
- Changed readme
- Changed readme
- Changed readme

### 📚 Documentation

- Rewrite README to cover command surface, config, and Stage Manager support

### ⚙️ Miscellaneous Tasks

- Build and test on macOS instead of Linux/Windows
- Add Homebrew formula + release checklist ([#11](https://github.com/cesarferreira/snap/issues/11))
