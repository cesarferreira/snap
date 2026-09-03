# Snap — Product Requirements Document

## 1. Overview

**Snap** is a fast, minimal, command-line window manipulation tool for macOS.

It provides the useful parts of graphical window managers such as Rectangle without requiring a menu-bar application, persistent daemon, configuration file, or full tiling window manager.

Snap follows a simple philosophy:

> Manipulate my windows, then get out of the way.

Examples:

    snap left 50
    snap right 50
    snap 75
    snap full
    snap center
    snap tile

Each command starts immediately, applies a short window transition, and exits.

Snap is **not** a traditional window manager. It does not continuously manage window positions, create virtual workspaces, replace Mission Control, or maintain a window tree.

---

# 2. Goals

Snap should:

- Be extremely fast.
- Be usable entirely from the terminal.
- Require no background daemon.
- Require no configuration for normal usage.
- Manipulate the currently focused window by default.
- Support percentage-based sizing.
- Support common positioning operations.
- Fill the current screen without invoking macOS native fullscreen.
- Automatically tile visible windows on the current display.
- Work naturally when bound to external keyboard shortcuts.
- Animate window geometry changes without requiring a background process.
- Behave predictably with multiple monitors.
- Use macOS APIs directly rather than depending on Rectangle, yabai, AeroSpace, or Hammerspoon.

Typical commands should begin responding effectively instantaneously.

---

# 3. Non-goals

Snap v1 will NOT:

- Be a persistent tiling window manager.
- Run a daemon.
- Have a menu-bar application.
- Have a GUI.
- Have a TUI.
- Manage macOS Spaces.
- Implement virtual desktops.
- Automatically rearrange windows when applications open or close.
- Replace Mission Control.
- Depend on another window manager.
- Use macOS native fullscreen.
- Require disabling SIP.

These constraints are deliberate.

---

# 4. Platform

## v1

macOS only.

Snap will use the macOS Accessibility APIs to discover and manipulate application windows.

The first execution requiring window manipulation should detect whether Accessibility permission has been granted.

If permission is missing:

    $ snap left 50

    snap needs Accessibility permission.

    System Settings →
    Privacy & Security →
    Accessibility

Snap should optionally trigger the standard macOS Accessibility permission request when appropriate.

---

# 5. CLI Philosophy

Commands should read naturally.

The basic grammar is:

    snap [position] [size]

Examples:

    snap 50
    snap left 50
    snap right 75
    snap top 50
    snap bottom 25

Special operations use simple verbs:

    snap full
    snap center
    snap tile

Avoid requiring flags for common operations.

Bad:

    snap --position left --width 50 --height 100

Good:

    snap left 50

---

# 6. Percentage Model

Snap's primary abstraction is percentage of the current display's **usable area**.

Supported percentages in v1:

    25
    50
    75
    100

The percentage determines how much of the relevant screen dimension the window occupies.

Snap should account for:

- Menu bar
- Dock
- Display bounds
- Display scaling
- Multiple monitors

The resulting window must remain inside the usable area of its target display.

---

# 7. Focused Window

Unless explicitly stated otherwise, Snap operates on the currently focused window.

Example:

    snap left 50

means:

> Resize the focused window to 50% of the usable display width and anchor it to the left side of its current display.

Snap should determine:

1. The currently focused application.
2. Its focused/main window.
3. The display containing the largest portion of that window.

That display becomes the target display.

If there is no manipulable focused window:

    $ snap left 50

    error: no focused window

Exit with a non-zero status.

---

# 8. Basic Sizing

## `snap 25`

Resize the focused window to approximately 25% of the usable screen while keeping it centered.

## `snap 50`

Resize the focused window to approximately 50% of the usable screen while keeping it centered.

## `snap 75`

Resize the focused window to approximately 75% of the usable screen while keeping it centered.

## `snap 100`

Equivalent to:

    snap full

This fills the usable display.

For non-directional percentages, both width and height should scale relative to the usable display.

For example:

    snap 50

results in a centered window approximately:

    width  = screen width  × 0.50
    height = screen height × 0.50

---

# 9. Directional Positioning

Supported directions:

    left
    right
    top
    bottom

## Horizontal

    snap left 25
    snap left 50
    snap left 75

    snap right 25
    snap right 50
    snap right 75

Horizontal positioning uses the full usable display height.

Example:

    snap left 50

produces:

    ┌──────────────┬──────────────┐
    │              │              │
    │   WINDOW     │              │
    │              │              │
    └──────────────┴──────────────┘

The window occupies:

    width  = 50%
    height = 100%

## Vertical

    snap top 25
    snap top 50
    snap top 75

    snap bottom 25
    snap bottom 50
    snap bottom 75

Vertical positioning uses the full usable display width.

Example:

    snap top 50

produces:

    ┌─────────────────────────────┐
    │           WINDOW            │
    ├─────────────────────────────┤
    │                             │
    └─────────────────────────────┘

The window occupies:

    width  = 100%
    height = 50%

---

# 10. Full Screen

Command:

    snap full

Alias:

    snap 100

This is one of Snap's core operations.

It MUST NOT invoke macOS native fullscreen.

Specifically, Snap must NOT:

- Create a new Space.
- Trigger the green fullscreen button.
- Enter `NSWindow` fullscreen mode.
- Move the application to a fullscreen desktop.
- Cause a fullscreen transition animation.

Instead, Snap simply resizes and positions the focused window to fill the **usable bounds of its current display**.

Conceptually:

    x      = usableScreen.x
    y      = usableScreen.y
    width  = usableScreen.width
    height = usableScreen.height

This behaves like Rectangle's "maximize" operation.

Example:

    $ snap full

Before:

    ┌─────────────────────────────┐
    │                             │
    │      ┌────────────┐         │
    │      │  Ghostty   │         │
    │      └────────────┘         │
    │                             │
    └─────────────────────────────┘

After:

    ┌─────────────────────────────┐
    │                             │
    │          Ghostty            │
    │                             │
    │                             │
    └─────────────────────────────┘

The menu bar and Dock remain respected according to macOS's usable screen bounds.

---

# 11. Center

Command:

    snap center

Moves the focused window to the center of its current display without changing its size.

The existing width and height must remain unchanged.

If the window is larger than the usable screen, its position should be clamped so that as much of the window as possible remains visible.

---

# 12. Automatic Tiling

Command:

    snap tile

Snap automatically arranges visible windows on the **current display**.

The target display is determined from the focused window.

Snap MUST NOT rearrange windows on other displays.

This is critical for predictable multi-monitor behaviour.

---

# 13. Windows Included in Tiling

`snap tile` should include normal, visible application windows on the target display.

Exclude:

- Minimized windows
- Hidden applications
- Windows on other displays
- Desktop elements
- Menu-bar elements
- Popovers
- Tooltips
- Dialogs where possible
- Utility/palette windows where identifiable
- Windows that cannot be resized

The focused window should always receive the primary tile.

---

# 14. Tiling Layouts

Layouts should be deterministic.

## One window

Equivalent to:

    snap full

Layout:

    ┌─────────────────────────────┐
    │                             │
    │              1              │
    │                             │
    └─────────────────────────────┘

---

## Two windows

50 / 50 split.

    ┌──────────────┬──────────────┐
    │              │              │
    │      1       │      2       │
    │              │              │
    └──────────────┴──────────────┘

The focused window receives position 1.

---

## Three windows

Master + stack.

    ┌──────────────────┬──────────┐
    │                  │    2     │
    │        1         ├──────────┤
    │                  │    3     │
    └──────────────────┴──────────┘

Window 1 receives approximately 50% of the screen width.

Windows 2 and 3 share the remaining half vertically.

The focused window receives position 1.

---

## Four windows

2 × 2 grid.

    ┌──────────────┬──────────────┐
    │      1       │      2       │
    ├──────────────┼──────────────┤
    │      3       │      4       │
    └──────────────┴──────────────┘

The focused window receives position 1.

---

## Five or more windows

Use a balanced grid.

Snap should calculate:

    columns = ceil(sqrt(windowCount))
    rows    = ceil(windowCount / columns)

The final row may contain fewer windows.

Example with 6:

    ┌─────────┬─────────┬─────────┐
    │    1    │    2    │    3    │
    ├─────────┼─────────┼─────────┤
    │    4    │    5    │    6    │
    └─────────┴─────────┴─────────┘

The algorithm should prioritize:

1. Balanced tile dimensions.
2. Maximum screen utilization.
3. Deterministic placement.
4. Focused window first.

---

# 15. Tile Ordering

Window ordering must be deterministic.

Order:

1. Focused window.
2. Remaining windows ordered by their current visual position:
   - top to bottom
   - left to right

This means repeatedly running:

    snap tile

should not randomly reorder windows.

---

# 16. Gaps

Default:

    0px

Snap should initially tile windows edge-to-edge.

Optional v1 flag:

    snap tile --gap 8

Example:

    ┌────────────┐  ┌────────────┐
    │            │  │            │
    │     1      │  │     2      │
    │            │  │            │
    └────────────┘  └────────────┘

Gap is specified in logical display points.

If implementing `--gap` meaningfully complicates v1, it may be deferred.

Zero-gap tiling is mandatory.

---

# 17. Multi-Monitor Behaviour

Snap should be conservative around multiple monitors.

Focused-window operations:

    snap left 50
    snap right 50
    snap full

operate on the display containing the focused window.

`snap tile` affects ONLY windows on that display.

Example:

    Monitor A              Monitor B

    ┌─────────────┐        ┌─────────────┐
    │ Ghostty     │        │ Slack       │
    │ Chrome      │        │ Spotify     │
    └─────────────┘        └─────────────┘

If Ghostty is focused:

    snap tile

may rearrange Ghostty and Chrome.

Slack and Spotify MUST NOT move.

---

# 18. CLI Output

Successful commands should normally produce no output.

Example:

    $ snap left 50
    $

This makes Snap suitable for scripts and keyboard bindings.

Errors go to stderr.

Example:

    $ snap left 42
    error: unsupported size '42'

    supported sizes: 25, 50, 75, 100

Exit status:

    0 = success
    1 = runtime failure
    2 = invalid arguments
    3 = Accessibility permission unavailable

---

# 19. Help

    snap --help

Example:

    snap — fast macOS window manipulation

    USAGE
      snap <size>
      snap <position> [size]
      snap <command>

    SIZES
      25              25% of screen
      50              50% of screen
      75              75% of screen
      100             fill screen

    POSITIONS
      left            anchor to left
      right           anchor to right
      top             anchor to top
      bottom          anchor to bottom

    COMMANDS
      full            fill current display
      center          center current window
      tile            tile visible windows

    EXAMPLES
      snap 50
      snap left 50
      snap right 75
      snap top 50
      snap full
      snap center
      snap tile

---

# 20. Keyboard Shortcut Integration

Snap itself does not need to register global keyboard shortcuts.

Users can bind Snap through:

- macOS Shortcuts
- Raycast
- Karabiner-Elements
- skhd
- BetterTouchTool
- shell scripts
- terminal launchers

Example mappings:

    ctrl + alt + left     → snap left 50
    ctrl + alt + right    → snap right 50
    ctrl + alt + 1        → snap 25
    ctrl + alt + 2        → snap 50
    ctrl + alt + 3        → snap 75
    ctrl + alt + enter    → snap full
    ctrl + alt + t        → snap tile

Snap deliberately separates **window manipulation** from **hotkey management**.

---

# 21. Architecture

Recommended implementation:

**Swift**

Reasons:

- Native Accessibility APIs.
- Native macOS display APIs.
- Straightforward Accessibility permission handling.
- No bridge to Objective-C/macOS APIs required.
- Small standalone binary.
- Easy Homebrew distribution.

Potential modules:

    SnapCLI
        Argument parsing and command dispatch

    WindowManager
        Discover focused window
        Discover visible windows
        Move/resize windows

    DisplayManager
        Determine target display
        Determine usable bounds
        Coordinate conversion

    LayoutEngine
        Pure geometry calculations

    TileEngine
        Window selection
        Ordering
        Layout assignment

    AccessibilityManager
        Permission detection
        Permission request
        Accessibility errors

The layout engine should contain no macOS Accessibility logic.

Given:

    screen = 1728 × 1117
    command = left 50

it should simply return:

    Rect(
        x: 0,
        y: 0,
        width: 864,
        height: 1117
    )

This makes most behaviour unit-testable without manipulating real windows.

---

# 22. Performance

Snap should respond immediately even when the visible transition takes a
fraction of a second.

Target:

    geometry calculation and first frame < 100 ms
    default transition duration = 180 ms

after process startup where practical.

Avoid:

- Persistent processes
- Network access
- Heavy dependencies
- Configuration parsing
- Unnecessary application enumeration

Focused-window commands should inspect only the information necessary to manipulate that window.

`snap tile` may enumerate visible application windows.

---

# 23. Window Constraints

Some macOS applications impose minimum or maximum window sizes.

Snap should request the calculated geometry and then tolerate the application adjusting it.

Failure to achieve the exact requested dimensions should not necessarily be considered an error.

If a window cannot be moved or resized at all, Snap should return a useful error.

Example:

    error: window cannot be resized

During `snap tile`, an unmanageable window should be skipped rather than causing the entire operation to fail.

---

# 24. Coordinate Systems

macOS APIs may expose different coordinate systems between Accessibility and display APIs.

Snap must normalize coordinates internally.

The rest of the application should operate on a single internal `Rect` representation.

Coordinate conversion should be isolated inside the macOS integration layer.

This is particularly important for:

- vertically arranged monitors
- monitors positioned left of the primary display
- displays with different resolutions
- Retina scaling

---

# 25. Testing

## Layout unit tests

The layout engine should be extensively unit tested.

Test:

    left 25
    left 50
    left 75

    right 25
    right 50
    right 75

    top 25
    top 50
    top 75

    bottom 25
    bottom 50
    bottom 75

    centered 25
    centered 50
    centered 75

    full

Use multiple screen dimensions.

---

## Tile tests

Test layouts for:

    1 window
    2 windows
    3 windows
    4 windows
    5 windows
    6 windows
    7+ windows

Verify:

- no overlapping tiles
- tiles stay inside usable bounds
- deterministic ordering
- focused window receives primary tile
- expected master layout for 3 windows

---

## Multi-monitor tests

Test displays with:

- different resolutions
- negative coordinates
- vertical arrangements
- Retina scaling

Verify that operations use the correct target display.

---

# 26. Distribution

Primary installation method:

    brew install snap

If the `snap` Homebrew formula name is unavailable, use a custom tap:

    brew install <owner>/tap/snap

Also publish binaries through GitHub Releases.

Target architectures:

    arm64
    x86_64

Prefer a universal macOS binary if practical.

---

# 27. README Experience

The README should communicate the product within the first few seconds.

Suggested opening:

    # snap

    Window management from the terminal.

    $ snap left 50
    $ snap right 50
    $ snap full
    $ snap tile

Then show an animated demonstration.

The README should emphasize:

> No daemon. No GUI. No config.

---

# 28. v1 Command Surface

The complete intended v1 interface should remain deliberately small:

    snap 25
    snap 50
    snap 75
    snap 100

    snap left 25
    snap left 50
    snap left 75

    snap right 25
    snap right 50
    snap right 75

    snap top 25
    snap top 50
    snap top 75

    snap bottom 25
    snap bottom 50
    snap bottom 75

    snap center
    snap full
    snap tile

    snap --help
    snap --version

Optional:

    snap tile --gap <points>

Anything beyond this requires a strong justification.

---

# 29. Future Ideas

These should NOT block v1.

## Window listing

    snap list

Example:

    ID       APP               TITLE
    182      Ghostty           snap
    194      Chrome            GitHub
    201      Android Studio    robot-android
    220      Slack             #general

This could eventually enable scripting.

---

## Application targeting

    snap --app Ghostty left 50

or:

    snap Ghostty left 50

Do not choose syntax until the feature is actually required.

---

## Window movement between displays

Potential commands:

    snap display next
    snap display previous

The window should preserve its approximate relative size/position.

---

## More layouts

Potential commands:

    snap tile columns
    snap tile rows
    snap tile master

Do not implement until actual demand exists.

---

## Custom percentages

Potential future support:

    snap left 33
    snap right 67

v1 deliberately restricts percentages to:

    25
    50
    75
    100

This keeps the interface predictable and testable.

---

## TUI

A future:

    snap ui

could provide interactive window selection and placement.

This is explicitly outside v1.

Snap should first prove that the command-line interaction is sufficient.

---

# 30. Product Principles

When deciding whether a feature belongs in Snap, apply these rules:

### 1. Stateless

Every invocation should be independent.

    command → manipulate windows → exit

### 2. Predictable

The same command against the same window arrangement should produce the same result.

### 3. Composable

Snap should work naturally with shells, scripts, launchers, and hotkey tools.

### 4. Native

Use macOS APIs directly.

Do not require another window manager.

### 5. Small

If Snap starts needing a daemon, persistent layout tree, workspace abstraction, or large configuration system, it is probably becoming the wrong product.

### 6. Fast

Window manipulation should feel immediate.

### 7. Safe

Commands should affect the smallest reasonable scope.

In particular:

    snap tile

means:

> Tile the visible windows on **this display**.

Not:

> Rearrange my entire desktop.

---

# 31. Definition of Done — v1

Snap v1 is complete when a user can install it and reliably run:

    snap left 50
    snap right 50
    snap top 50
    snap bottom 50

    snap 25
    snap 50
    snap 75

    snap full
    snap center
    snap tile

with:

- correct handling of the focused window
- correct usable-screen calculations
- correct multi-monitor behaviour
- deterministic tiling
- no background process
- no GUI
- no native macOS fullscreen
- no SIP modifications
- clear Accessibility permission handling
- automated geometry/layout tests
- arm64 macOS support
- documentation sufficient to install and use the tool

The core promise should fit in one sentence:

> **Snap puts your windows where you want them, straight from the terminal.**
