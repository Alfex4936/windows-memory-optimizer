---
name: Memori
description: A measured native Windows memory and startup utility.
colors:
  background: "#13151B"
  navigation: "#0D0F14"
  surface: "#191C24"
  control: "#222630"
  control-hover: "#2C313D"
  text: "#F4F5F8"
  text-secondary: "#A5ADBF"
  rule: "#353B4A"
  accent: "#D2F780"
  cpu: "#C5B5FF"
  caution: "#F0C17A"
  error: "#F29B9B"
typography:
  display:
    fontFamily: "egui monospace"
    fontSize: "28px"
  management-reading:
    fontFamily: "egui monospace"
    fontSize: "43px"
  compact-reading:
    fontFamily: "egui monospace"
    fontSize: "21px"
  title:
    fontFamily: "Malgun Gothic, egui proportional fallback"
    fontSize: "27px"
  section:
    fontFamily: "Malgun Gothic, egui proportional fallback"
    fontSize: "19px"
  summary:
    fontFamily: "Malgun Gothic, egui proportional fallback"
    fontSize: "16px"
  body:
    fontFamily: "Malgun Gothic, egui proportional fallback"
    fontSize: "14px"
  button:
    fontFamily: "Malgun Gothic, egui proportional fallback"
    fontSize: "13px"
  label:
    fontFamily: "Malgun Gothic, egui proportional fallback"
    fontSize: "12px"
  micro:
    fontFamily: "Malgun Gothic, egui proportional fallback"
    fontSize: "11px"
rounded:
  control: "8px"
  control-hover: "8px"
  notice: "6px"
  details: "12px"
  toolbar: "8px"
  compact-action: "11px"
  widget-action: "9px"
  widget-shell: "20px"
  compact-shell: "19px"
spacing:
  item-x: "10px"
  item-y: "8px"
  widget-item-x: "8px"
  widget-item-y: "6px"
  button-x: "14px"
  button-y: "7px"
  field: "7px"
  inset: "12px"
  details-inset: "16px"
  widget-inset: "18px"
  content-x: "30px"
  content-y: "24px"
components:
  button-widget-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.navigation}"
    typography: "{typography.body}"
    rounded: "{rounded.widget-action}"
    height: "38px"
  button-compact-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.navigation}"
    rounded: "{rounded.compact-action}"
    size: "32px"
  button-widget-expand:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text-secondary}"
    rounded: "{rounded.toolbar}"
    size: "32px"
  button-memory-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.navigation}"
    typography: "{typography.button}"
    rounded: "{rounded.control}"
    width: "200px"
    height: "36px"
  button-secondary:
    backgroundColor: "{colors.control}"
    textColor: "{colors.text}"
    typography: "{typography.button}"
    rounded: "{rounded.control}"
    padding: "7px 14px"
    height: "32px"
  button-secondary-hover:
    backgroundColor: "{colors.control-hover}"
    textColor: "{colors.text}"
    rounded: "{rounded.control-hover}"
  input-search:
    backgroundColor: "{colors.navigation}"
    textColor: "{colors.text}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "{spacing.field}"
  navigation-selected:
    backgroundColor: "{colors.control}"
    textColor: "{colors.accent}"
    typography: "{typography.body}"
    height: "42px"
  notice:
    backgroundColor: "{colors.control}"
    textColor: "{colors.accent}"
    rounded: "{rounded.notice}"
    padding: "{spacing.inset}"
  widget-shell:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text}"
    rounded: "{rounded.widget-shell}"
    padding: "{spacing.widget-inset}"
  compact-shell:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text}"
    rounded: "{rounded.compact-shell}"
    padding: "{spacing.inset}"
  toolbar-toggle:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text-secondary}"
    rounded: "{rounded.toolbar}"
    size: "28px"
  sort-header-active:
    backgroundColor: "{colors.control}"
    textColor: "{colors.accent}"
    typography: "{typography.label}"
    rounded: "{rounded.notice}"
    height: "30px"
  startup-filter-selected:
    textColor: "{colors.accent}"
    typography: "{typography.label}"
    rounded: "{rounded.toolbar}"
    height: "30px"
  startup-badge:
    typography: "{typography.micro}"
    rounded: "12px"
    height: "24px"
  startup-switch-on:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.navigation}"
    rounded: "12px"
    width: "44px"
    height: "24px"
  startup-switch-off:
    backgroundColor: "{colors.control}"
    textColor: "{colors.text-secondary}"
    rounded: "12px"
    width: "44px"
    height: "24px"
  startup-details:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.details}"
    padding: "{spacing.details-inset}"
  startup-details-short:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.details}"
    padding: "{spacing.inset}"
---

# Design System: Memori

## Overview

Memori pairs graphite surfaces with lime for RAM and primary actions, and lavender for CPU. The widget puts current readings and manual trim within reach. The management window pairs memory history with an aligned process list and gives startup, storage cleanup, and power controls dedicated views.

**Key Characteristics:**

- Measured numerical readings and stable table tracks.
- Lime RAM and action indicators with a lavender CPU series.
- Korean action labels, explicit state text, and local feedback.
- A contained widget and a spacious, ruled management view.

## Colors

Graphite layers support the lime RAM and action channel and the lavender CPU channel. Amber and coral carry exceptional states.

### Primary

- **Lime** (`accent`): primary trim actions, selected navigation label, memory usage, history trace, used RAM meter segments, and successful feedback. Primary action text uses navigation ink.

### Secondary

- **Warm Caution** (`caution`): stale data, uncertain startup states, and reasons an application cannot be trimmed. A known Windows-disabled startup entry uses the quiet off-state treatment rather than warning color.
- **Soft Coral** (`error`): failed operations in the management notice.

### Neutral

- **Instrument Surface** (`surface`) fills signal panels and alternate rows.
- **Instrument Ink** (`background`): central canvas and enclosed widget.
- **Navigation Ink** (`navigation`): sidebar, status strip, and text fields.
- **Raised Slate** (`control`): resting controls, selected navigation, notices, startup details, and selected toolbar toggle.
- **Hover Slate** (`control-hover`): ordinary control hover fill.
- **Warm White** (`text`): primary content.
- **Quiet Blue Gray** (`text-secondary`): support copy, labels, and secondary state text.
- **Track Rule** (`rule`): list tracks, plot guides, control borders, and inactive meter segments.

**The Signal Rule.** Pair state color with visible text or a named control. Lime identifies a measured value or an action. State labels describe what was measured or changed.

Selection uses `accent.gamma_multiply(0.25)`. Plot fill fades from 0.24 at the trace to 0.03 at the baseline.

## Typography

**Display and Body Font:** Malgun Gothic, loaded from Windows when available; otherwise egui's proportional fallback. Readings and the process table's numeric columns use egui's built-in monospace face.

**Character:** readable Korean typography lets alignment and numerical scale supply distinction. Technical process names stay intact. `RichText::strong()` is egui's stronger-color treatment; it does not install a bold weight.

### Hierarchy

- **Display** (`display`) shows RAM and CPU in the expanded widget. The management reading is larger.
- **Compact Reading** (`compact-reading`): RAM percentage in the compact rail.
- **Title** (`title`): management page heading.
- **Section** (`section`): process-list heading.
- **Summary** (`summary`): physical-memory summary and startup introduction.
- **Body** (`body`): rows, primary information, and widget action.
- **Button** (`button`): ordinary native action labels.
- **Label** (`label`): support text, table headers, and status.
- **Micro** (`micro`): plot axes, widget feedback, and footer metadata.

The same-family wordmark uses 21 in the widget and 22 in the sidebar; startup item names use 15. These are local sizes, not a ratio-based scale. The configured native Heading style is 24; explicit page/section sizes govern those surfaces.

## Layout

The default window is an undecorated, non-resizable widget, restoring the saved compact/expanded mode and pin state. Without preferences it opens expanded and always on top. Shared `widget_size()` measurements are compact 258 × 78 and expanded 376 × 566. Both have a transparent outer margin (5 compact, 6 expanded) and fine enclosure. The compact rail uses inset 12, a RAM label/percentage drag region, a broom cleanup action, and an expand button. The expanded widget uses the widget inset token and keeps its readings, meter, main action, feedback, five-row CPU/RAM process list, and three management links enclosed. CPU and RAM headers sort independently of the management table until the full-process link transfers the selected order. Its painted logo/name are left-aligned, with collapse, pin, and close controls on the right.

Both header regions use native caption hit-testing (`WM_NCHITTEST` → `HTCAPTION`), scoped to this app window and removed when the drag-region owner is dropped. The expanded logo, painted wordmark, blank header, and its top/left inset form the drag region; collapse, pin, and close are excluded. Compact cleanup and expand controls are likewise excluded. The native hit-test area owns the Windows move cursor through WM_SETCURSOR, so the cursor hint follows the actual draggable area; the egui grab cursor is reserved for the native-registration fallback. A primary-button `StartDrag` request is only the fallback when native registration is unavailable. Widget labels do not select text; double-clicking the caption region does not maximize the widget.

The widget restores its last position in physical desktop pixels, clamped into the nearest monitor's work area when necessary. Position, pin, and compact mode are stored together under `HKCU\Software\Memori\Settings`; movement writes are debounced by 400 ms and settings are also saved on exit. The 16-byte `MEM2` value reads older `MEM1` preferences as expanded. Management-window coordinates never overwrite the widget position. Normal widget launch restores mode; `--compact` forces the compact rail and `--expanded` forces the expanded widget. The smaller management-window QA flag is `--small-window`.

The management window opens at 1120 × 790, minimum 880 × 650. Returning from the widget centers it within the current monitor's actual work area, preserving that monitor's origin and DPI. The fixed sidebar (174) has horizontal/vertical insets (18 / 26). Main content uses the content inset tokens; the status strip uses horizontal/vertical insets (22 / 8). Native window decoration returns in this mode.

Ordinary item gaps use the two item tokens. Expanded widget rows use the widget x/y gaps; the compact rail uses a horizontal gap of 8 and no vertical item gap. The measured spacing is mixed, not a universal eight-point scale.

**The Stable Tracks Rule.** Fix data and state columns; let the name column absorb remaining width. Text truncates inside its allocated cell rather than moving adjacent values.

- Process rows use a minimum height of 32. The name flexes while PID, CPU, working set, and status share fixed tracks. Below 720 points of content width, the tracks narrow so each sort remains available.
- Startup rows: fixed height 66, flexible name/source region, state column 130, and action column 136. Manageable entries use a small switch; other entries retain the Windows settings target (112 × 32).
- Lists scroll vertically. Process search width is 200. Startup search uses 220, reducing to 160 when its remaining layout width is below 230. With details open, the startup list reserves 260 below it, reducing to 216 when remaining content height is below 360. Its minimum list height is 66 with details open and 100 otherwise; the closed-state reserve is 112. Startup row height stays 66 in either case. The details command region scrolls independently, bounded at 76 high or 40 in the shorter layout.
- The chart uses a 112-point height or 90 in short windows. The time window grows from 15 to 30, 60, or 120 seconds as readings arrive.

## Elevation & Depth

Tonal layering and fine borders provide depth. The application assigns no decorative shadows to the widget, sidebar, notices, details, or lists. Native window chrome and egui tooltips retain framework behavior; these are not an application elevation scale.

Plot guides use light rules (0.7); list tracks are finer (0.5). Inactive control borders are 1; hovered/focused borders use lime at 1; active borders use lime at 1.5. The history line is 2, with endpoint radius 3.5. The area below the measured history uses a subtle lime fade.

## Shapes

Ordinary controls use an eight-point radius. Notices use the notice radius; startup details use their separate, softer details radius. Widget toolbar controls use the toolbar radius; the compact lime cleanup target uses the compact-action radius; the expanded primary action uses the widget-action radius. Expanded and compact enclosures use their distinct shell tokens, giving the compact rail a softer capsule silhouette. The expanded widget and management trim buttons share the primary action radius.

The folded lime monogram has a lavender inset on a graphite tile. The widget and native window use the same PNG, and the executable embeds the matching Windows ICO. Meters light from left to right as use grows, with 15 segments in compact mode, 20 for each expanded reading, and 24 for each drive.

## Components

### Buttons

The full-width widget action uses widget-primary tokens and the label “메모리 정리.” The management primary action uses its own minimum width/height and a selection-count label. A minimum target size is not a maximum.

The compact rail uses an icon-only lime broom target with the compact-primary tokens. Its accessible name and tooltip state the trim action, eligible-app count, and temporary effect. It follows the expanded action's eligibility, busy, and stale-data disabling rules. A positive trimmed-app count replaces the broom with a check for two seconds; this is brief result feedback, not a persistent health score.

Secondary actions use secondary tokens and a rule border; hover uses the hover variant and lime outline. Keyboard focus uses egui's highlighted state. Pressed controls retain the native active visual and stronger lime border. Disabled actions use the framework's disabled treatment.

Busy actions say “정리 중…”. Widget trimming is disabled with no eligible targets, an active trim, or stale data. Management trimming requires selection. The expanded widget has three evenly sized buttons for process, startup, and optimization management.

### Navigation

Three labeled sidebar rows use a raised surface and lime indicator when selected. Changing page clears the prior search and detail selection. The optimization page has separate tabs for storage cleanup and power management.

### Inputs / Fields

Single-line search uses navigation ink, rule border, field inset, and proportional body type. Its hint identifies process name/PID or startup apps. Focus uses the native lime border. Empty results show an explanation and a next step.

### Cards / Containers

The widget is the distinctive enclosed container. Management content uses ruled lists rather than decorative cards. Notices retain their notice radius and inset; error notices can expose a scrollable failure list. Startup details form a distinct full-width panel using the details tokens and a fine rule border, separated clearly from the rows above.

### Selection and toolbar controls

Each process checkbox exposes a name containing its process name and PID; the header checkbox names its filtered “select all eligible” scope. Ineligible checkboxes are disabled and explain their reason on hover.

The expanded toolbar contains named collapse, always-on-top, and close controls (28 × 28). Pin exposes accessible selected state. The compact expand control is 32 × 32; its accessible name is “위젯 펼치기,” while collapse is “컴팩트 모드로 접기.” All icons use custom line geometry and retain accessible names. The expanded header exposes “위젯 이동 영역”; the compact header names RAM percentage and movement. Painted header readings and wordmark are not selectable text.

### Sortable process table

Each name, PID, CPU, working-set, and status header is a sort button with minimum height 30. Active headers use a control-tone fill, notice rounding, and lime text; hover/focus also reveals the fill, with a lime focus border. Painted triangles show one active ascending/descending direction, or two quiet indicators for an inactive column. The accessible header name includes the column, current direction, and sorting action. A text summary above the table repeats the criterion and direction.

The initial order is working set descending. Repeatedly selecting the same column reverses direction; switching to memory starts descending, while name, PID, and status start ascending. PID and working set use numeric values, names compare without case, and status groups eligibility/reason. Equal values resolve by PID and process creation time so refresh does not shuffle ties. Sorting survives refresh within the session and is not stored across app restarts.

### Memory instruments

The segmented meter shows the current used/available split. The management view uses a real timestamped history line. With fewer than three samples, the plot explicitly says that history is being collected.

The compact rail shows current RAM percentage rather than a compressed plot. Its RAM reading turns caution-colored for missing/stale data and error-colored for a failed operation. Hovering the reading exposes memory totals or the operation notice; expanding returns the detailed readings and feedback.

**The Measured Evidence Rule.** Show observed readings and temporary working-set deltas. Empty or newly collected history remains visibly incomplete; it is never filled with invented activity.

### Startup rows

Names toggle source/command details. A soft badge pairs a small dot (radius 2.5) with explicit text: lime “켜짐,” quiet “꺼짐 · 보관됨” / “꺼짐 · Windows,” or caution “확인 필요.” The badge is 24 high with radius 12, micro type, and a state-color fill derived with `gamma_multiply(0.12)`. Its accessible label names automatic execution and the state. Unselected off-state item names use secondary text. Alternate and hovered rows use the instrument surface. Fixed row rectangles keep columns aligned. The selected row uses the full control fill, a lime outline (1), and primary name text, with selected/open-or-close semantics exposed by its name button.

Manageable entries use a labeled accessible switch: lime track with a navigation-tone thumb on the right when on, control-tone track with a secondary-text thumb on the left when off. The minimum requested size is 44 × 24; inherited interaction sizing can increase its height to 28. The thumb radius is 8; focus has a lime outline around the switch. Tooltips describe disabling with original preservation or enabling by restoration. Other entries retain “Windows 설정.” Busy startup mutations disable their controls.

Counted filters “전체,” “켜짐,” “꺼짐,” and “확인 필요” combine with search. The off filter includes both Memori-preserved and Windows-disabled states. The selected filter uses lime label text, toolbar rounding, and a minimum height of 32. State/action tracks stay fixed while names truncate; an empty filtered result explains how to change the filter or query.

The startup screen also has a labeled switch, “로그인 시 Memori 위젯 열기.” It manages only this executable's `Memori` entry in the current-user Run key, using the quoted executable path followed by `--widget`. Its checked state reports this registration; Windows can independently block startup. Keep this explicit Memori preference separate from the other applications' row actions.

### Startup details

The details panel spans the content width with an instrument surface, lime border, details rounding, and details inset. The shorter layout uses the ordinary inset instead. A visible “상세 정보” heading uses summary type; a standard “닫기” button exposes the accessible name “상세 정보 닫기.” The selected application's body-sized name stays on one truncated line with its full name available on hover. The “실행 명령” area has a recessed fill, followed by the management explanation in a bounded scroll area.

Clicking the same entry again, the close button, or Escape in the full startup view closes details. Closing clears the selected entry and returns the reserved space to the list. Escape handling is limited to the startup management view; widget caption, mode, pin, and position behavior remain independent.

### Motion

No decorative motion or custom easing is defined. egui's inherited control animation time is 100 ms. Memory collection is approximately every two seconds; changes request repaint, and one-second repaint advances relative age text. These are update intervals, not animation durations. Readings older than ten seconds receive a stale state.

## Do's and Don'ts

### Do:

- **Do** keep numerical and state tracks fixed while names flex and truncate.
- **Do** keep every widget reading, action, and link inside its enclosed surface.
- **Do** retain visible Korean button/header labels and state text, with accessible names for compact icon actions, toolbar controls, switches, and checkboxes.
- **Do** pair caution, error, selection, and progress colors with explicit state text.
- **Do** show only collected history and describe trim results as temporary working-set changes.

### Don't:

- **Don't** imply a score, permanent recovered memory, or automatic repair.
- **Don't** hide essential state or the primary action when the management window is compact.
- **Don't** replace aligned lists with a decorative card grid.
- **Don't** turn seed-only measurements or framework defaults into an unsupported brand claim.
