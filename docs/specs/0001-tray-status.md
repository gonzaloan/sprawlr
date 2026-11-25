# Account status from the menu bar or system tray

Status: Built · Owner: gonzaloan · Date: 2026-11-24

## Problem

Whoever looks after an AWS account with sprawlr (an on-call engineer, a tech lead) learns its state only by opening the full window, reading "What needs attention" and pressing "Refresh health". Between those moments nothing tells them that errors started at InvoiceProcessor or that checkout-orders-db has 1.25× headroom. Closing the window quits the app, so keeping an eye on the account means leaving a 1440×900 window open for information that fits in one line. Problems get noticed when someone happens to open the app, not when they appear.

## Outcome

- The icon shows the account's level without opening anything: Act now, Plan for it, All clear or No status. The levels differ by shape alone, so they can be told apart in greyscale and as a macOS template icon.
- One click (macOS, Windows) or the menu (Linux) shows the status line, the top 3 items, flows with errors and the cleanup amount. One more click opens that item in the full app.
- The popover and the briefing never disagree: same items, same order, same numbers, because both read the snapshot.
- No new AWS cost by default. With default settings sprawlr makes no AWS call the person did not ask for. With background refresh on, it makes at most 96, 24 or 6 health refreshes a day (every 15 min, 1 h or 4 h).

## Scope

**In:**
- A tray icon with 4 states, a tooltip and a native menu.
- A popover window (360×480) with ready, empty, error and refreshing states, plus a settings view.
- `status` computed in the domain and stored in the snapshot.
- Optional background health refresh, optional notifications and optional launch at login, all off by default.
- Close-to-tray with a one-time hint, and a single running instance (a second launch focuses the first).
- A `?view=tray` preview in the web demo.

**Out (and why):**
- **Full rescans from the tray or in the background.** Discovery is long, and a scan needs the region choices made in the main window. The tray only refreshes health.
- **Error logs in background refresh.** Logs Insights bills per GB scanned (ADR 0005). See Open questions.
- **Several accounts in the tray.** The store keeps one latest snapshot. Tracking more would need history, which does not exist yet.
- **Trends ("worse than yesterday").** These also need stored history.
- **A global shortcut to open the popover.** It needs another plugin and permission, and nobody has asked for it.
- **Buttons inside notifications.** The notification plugin offers actions only on mobile.
- **A tray in the web demo.** Browsers have no tray. The preview route covers development and screenshots.

## Experience

### Tray icon

| State | When | Badge on the sprawlr mark | Tooltip (macOS, Windows) |
|---|---|---|---|
| Act now | `level = act_now` | solid triangle with "!" | `sprawlr: Act now. 2 to act on now, 3 to plan for` |
| Plan for it | `plan_for_it` | hollow ring | `sprawlr: Plan for it. Nothing urgent, 3 to plan for` |
| All clear | `all_clear` | none, mark only | `sprawlr: All clear. Nothing needs attention` |
| No status | nothing stored, or `not_measured` | dashed outline | `sprawlr: No status yet. Open sprawlr to scan an account` |

- While a refresh runs, or after one fails, the icon keeps the last known level. The tooltip adds ` · Refreshing…`, ` · Last refresh failed` or ` · Sign in again to refresh`.
- On Windows and Linux the badge is also coloured, using the `--alarm` and `--warn` values. Colour is never the only signal.

### Menu (right click on every OS)

```
┌───────────────────────────────────────────┐
│ Act now: 2 to act on now, 3 to plan for   │  disabled: the status line
├───────────────────────────────────────────┤
│ Show status                               │
│ Open sprawlr                              │
│ Refresh health                            │  "Refreshing…", disabled, while one runs
├───────────────────────────────────────────┤
│ Quit sprawlr                              │
└───────────────────────────────────────────┘
```

Changes from the brief, and why:
- **"Show status" is added.** Linux sends no click events, so the menu is the only way to open the popover there.
- **The status line is the first item.** Linux shows no tooltip.
- **"Refresh now" becomes "Refresh health".** That is the label the app already uses for the same action.
- **"Quit" becomes "Quit sprawlr".** With the window closed, it says what is quitting.

### Popover (demo account)

```
┌──────────────────────────────────────────┐
│ sprawlr                       [Settings] │
│ 111122223333 · prod-admin                │
│ Read 12 min ago                          │
├──────────────────────────────────────────┤
│ /!\ Act now                              │
│ 2 to act on now, 3 to plan for           │
├──────────────────────────────────────────┤
│ ▌Act now                                 │
│ ▌Errors start at InvoiceProcessor      › │
│ ▌Act now                                 │
│ ▌checkout-orders-db saturates first    › │
│ ▌Plan for it                             │
│ ▌InvoicesTable is on 2 flows           › │
│  See all 7 in sprawlr                    │
├──────────────────────────────────────────┤
│ Flows with errors, last 24 h             │
│  Web requests to checkout        45    › │
│  MonthlyClose runs               26    › │
├──────────────────────────────────────────┤
│ Probably not needed                      │
│  5 resources, about $37 a month        › │
├──────────────────────────────────────────┤
│ [Refresh health]          [Open sprawlr] │
│ A few cents of CloudWatch per refresh.   │
└──────────────────────────────────────────┘
```

**Look.** The popover uses the app's survey-map design language:
- `--paper` ground with a 1 px `--rule` border. There is no transparency, because rounded transparent windows need macOS private APIs.
- Barlow Condensed headings.
- The `.insight--high/medium/low` left rules, and `.btn` / `.btn--link` buttons.
- No raw colours.

**Rows and where they open.** Every row is a button. After one is used, the popover hides.
- An attention item opens the same place the briefing would: its flow if it has one, otherwise its first resource.
- A flow row opens that flow.
- The cleanup row opens "Probably not needed".
- "See all 7 in sprawlr" and "Open sprawlr" open the overview.

**Text details:**
- The account line shows "default credentials" when no profile is set, and "Demo account" in the preview.
- "Read …" goes from "just now" to "N min ago" to "N h ago", then shows the date. It updates every minute while the popover is open.
- If the snapshot has error patterns, the cost hint reads "A few cents of CloudWatch, plus Logs Insights per GB scanned." A manual refresh reads the logs again, as the app does today.
- Up to 3 flows with errors are listed, then "and N more".

### States

| State | Shows | Buttons |
|---|---|---|
| Never scanned (nothing stored) | "No account read yet. Scan an account in sprawlr to see its status here." | "Open sprawlr" |
| Health not read (`not_measured`) | Status line "Health not read. Scan with alarms, metrics and traces on to see errors and load." Items are still listed. | "Open sprawlr" |
| Refreshing | The button reads "Refreshing…" and is disabled. The progress step shows below it (`aria-live="polite"`), and the last status stays visible. | none |
| Sign-in needed (no credentials, or an expired SSO session) | "Can't sign in with prod-admin. Run `aws sso login --profile prod-admin`, then try again." The command is selectable text. With background refresh on, it adds "Background refresh is paused until a refresh works." The last status stays below, with its "Read" time. | "Try again", "Open sprawlr" |
| Refresh failed (any other error) | "Refresh failed: \<message\>. The status below is from \<time\>." With background refresh on, it adds "Next try in 2 h." | "Try again" |
| Partly read (AccessDenied on some reads) | "2 parts could not be read. Open sprawlr to see which." under the status line | "Open sprawlr" |
| Web preview (`?view=tray`) | The demo status, with "Refreshing needs the desktop app." under a disabled "Refresh health" | none |

### Settings view ("Settings" opens it, "Back" returns)

```
┌──────────────────────────────────────────┐
│ [Back]                         Settings  │
│ Refresh health in the background         │
│  (•) Off                                 │
│  ( ) Every 15 minutes      about 96 a day│
│  ( ) Every hour            about 24 a day│
│  ( ) Every 4 hours          about 6 a day│
│  Each refresh is billed by CloudWatch.   │
│  Error logs are never read in the        │
│  background. Paused while signed out.    │
│ [ ] Notify me when something needs       │
│     action now                           │
│ [ ] Start sprawlr when I log in          │
│ [x] Keep sprawlr in the menu bar when    │
│     its window is closed                 │
└──────────────────────────────────────────┘
```

- On Windows and Linux, "menu bar" reads "system tray".
- If the OS blocks notifications, the checkbox switches back off and this shows below it: "Notifications are turned off for sprawlr in your system settings."
- If autostart fails, this shows: "Could not change login items: \<message\>."

### Keyboard

- When the popover opens, focus goes to the first item. In an empty state it goes to the only button.
- Tab and Shift+Tab go in this order, and wrap: header, items, flows, cleanup, "Refresh health", "Open sprawlr", "Settings".
- Up and Down move within a list. Enter or Space activates. Focus uses the existing `:focus-visible` ring.
- Esc hides the popover from any view.
- Clicking outside the popover hides it. Clicking the icon while it is open hides it, and it stays hidden.
- The root element is `role="dialog"` with `aria-label="sprawlr status"`.

### Lifecycle

1. On start, sprawlr builds the icon from the stored snapshot and opens the main window as it does today.
2. Closing the main window hides it. This is the default on macOS and Windows.
   - The first time only, a native message appears with an "OK" button: "sprawlr is still running in the menu bar. Click its icon to see the account status, or choose Quit sprawlr from its menu to exit."
   - On Windows it reads "…in the system tray. If you can't see it, look under the ^ arrow on the taskbar."
3. "Quit sprawlr" stops background refresh and exits.
4. Launching sprawlr while it is already running shows and focuses the existing main window.
5. Whenever a snapshot is saved (by a scan, a refresh from either window, or a background refresh):
   - the icon, tooltip, menu status line and popover update;
   - if the main window shows the stored scan, its data updates in place. Its place, environment and selection do not change.

## Design

### Domain: `crates/sprawlr-domain/src/status.rs` (new)

```rust
pub enum Level { ActNow, PlanForIt, AllClear, NotMeasured }       // serde snake_case
pub struct AccountStatus {
    pub level: Level,
    pub headline: String,                   // "2 to act on now, 3 to plan for"
    pub account: Option<String>,            // the account id the nodes carry
    pub counts: Counts,                     // act_now, plan_for_it, good_to_know
    pub top: Vec<StatusItem>,               // first 3 insights, in briefing order
    pub error_origins: Vec<StatusItem>,     // every "Errors start at" item
    pub flows_with_errors: Vec<FlowErrors>, // errors_24h > 0, most first
    pub cleanup: Cleanup,                   // resources, monthly_cost (USD, unrounded)
}
pub struct StatusItem { pub key: String, pub priority: Priority, pub title: String, pub open: Target }
pub enum Target { Flow(String), Node(NodeId) }
pub struct Notice { pub title: String, pub body: String, pub open: Target }

pub fn of(nodes: &[Node], insights: &Insights) -> AccountStatus;
pub fn news(before: &AccountStatus, after: &AccountStatus) -> Option<Notice>;
```

The first matching row sets the level:

| Level | When | Headline |
|---|---|---|
| `act_now` | at least one insight is High | "{h} to act on now", plus ", {m} to plan for" if any are Medium |
| `plan_for_it` | no High, at least one Medium | "Nothing urgent, {m} to plan for" |
| `not_measured` | no node has an alarm state or metrics | "Health not read" |
| `all_clear` | anything else | "Nothing needs attention" |

**Levels:**
- The labels reuse the briefing's words ("Act now", "Plan for it") rather than the brief's "something to watch". That way the icon, the popover and the briefing use one vocabulary.
- `not_measured` exists because "All clear" after a scan with telemetry off would be false.

**Items:**
- `key` is `"{kind}:{first node id}"`. It stays the same across refreshes, because titles carry no counts.
- `open` is the item's flow if it has one, otherwise its first node. This is where the briefing goes when the item is clicked.

**Notifications (`news`):**
- It returns at most one notice per refresh: when the level moves into `act_now`, or when an `error_origins` key is new.
- The title is "sprawlr: Act now". The body is the item's title ("Errors start at InvoiceProcessor"), plus " and N more" when several changed.
- An item present both before and after never triggers a notification.

**Wiring:**
- `graph.snapshot()` fills in `status`.
- `logs` gains `was_read(nodes)`, used by both `refresh_health` and `RefreshOptions`.

### Snapshot contract

- New field: `Snapshot.status: Option<AccountStatus>` with `#[serde(default)]`, mirrored in `app/src/domain/types.ts`.
- Older snapshots load with `None`. `sprawlr_app::account_status(&Snapshot)` returns the stored status or computes it, so the UI never computes rules (ADR 0004).
- The Rust driver reads the same value for the icon, tooltip and menu.
- The demo data must be regenerated.

### Application: `crates/sprawlr-app`

| Piece | Kind | What |
|---|---|---|
| `RefreshOptions { error_logs }` | changed use case input | `refresh_health` takes it. Manual refreshes use `RefreshOptions::like(&snapshot)`, which keeps today's behaviour. Background refresh uses `error_logs: false`. |
| `refresh_in_background(source, store, notifier)` | new use case | Loads `store.latest()`. If nothing is stored, it returns `Ok(None)` without calling the source. Otherwise it refreshes health without logs, saves, and passes `news(before, after)` to the notifier when there is one. It is bound only by `HealthSource + ErrorLogSource`, so it cannot discover structure. |
| `Notifier` | new port | `fn notify(&self, notice: &Notice) -> anyhow::Result<()>`. Keeps notifications out of the domain and lets tests use a double. |
| `SettingsStore`, `DesktopSettings` | new port and type | `background: Option<Every>` (default `None`), `notifications: false`, `close_to_tray: Option<bool>` (`None` means the platform default), `close_hint_shown: false`, `last_profile: Option<String>` |
| `background::next_wait(every, failures)` | new pure function | `every × 2^failures`, capped at 24 h. A success resets the failure count. |
| `SignInNeeded { profile }` | new error type | Adapters return it inside `anyhow` when credentials are missing or the SSO session has expired. Drivers check for it with `downcast_ref`. |

- **No `Clock` port.** No use case reads the time. The driver owns the timer and calls `next_wait`. "Read 12 min ago" is view logic in the frontend.
- **Launch at login is not stored.** The autostart plugin's `is_enabled()` is the source of truth, so a login item removed in system settings never disagrees with what sprawlr shows.

### Adapters

- `sprawlr-fs::FileSettingsStore` keeps `settings.json` next to `last-snapshot.json`.
- `sprawlr-aws` maps credential-provider failures to `SignInNeeded`.
- `app/src-tauri/src/notify.rs` holds `TauriNotifier`, which implements `Notifier` with `tauri-plugin-notification`. It depends on Tauri, so it lives in the driver.

### Driver: `app/src-tauri`

- **`tray.rs`**
  - Builds the icon with `TrayIconBuilder`: the 4 icons in `icons/tray/`, the tooltip and the menu.
  - Calls `show_menu_on_left_click(false)`, and `icon_as_template(true)` on macOS.
  - Passes every tray event to the positioner, and updates the icon whenever a snapshot is saved.
- **`popover.rs`**
  - Defines the window `tray` in `tauri.conf.json`: 360×480, `visible: false`, `decorations: false`, `resizable: false`, `skipTaskbar: true`, `alwaysOnTop: true`.
  - Toggles and positions it, and hides it on `Focused(false)`.
  - Ignores a toggle that arrives right after a blur-hide, because the click on the icon is what caused the blur.
- **`background.rs`**
  - One task, which runs only when all of these hold: `background` is set, a snapshot is stored, no scan or refresh is running, and refresh is not paused by `SignInNeeded`.
  - A successful manual refresh or scan lifts the pause.
  - It uses `last_profile`.
- **`main.rs`**
  - Registers the plugins, single-instance first.
  - `CloseRequested` hides the window when close-to-tray is on. `ExitRequested` calls `prevent_exit()` unless the exit came from "Quit sprawlr".
  - `scan` and `refresh_health` now also record `last_profile`, update the tray and emit `snapshot-changed`.
  - New commands:
    - `refresh_stored`
    - `refresh_state`
    - `open_main(target)`, which shows the main window and emits `open-target`
    - `hide_popover`
    - `get_settings`
    - `save_settings`, which also turns autostart on or off
  - The main window gets an explicit `"label": "main"`.

The profile lives in the settings file, not in the snapshot. Snapshots get shared and committed (ADR 0003), and a local profile name does not belong in them.

### Frontend: `app/src`

- **`features/tray/`:** `TrayPopover.tsx`, `StatusHeader.tsx`, `AttentionList.tsx`, `TraySettings.tsx`, `TrayEmpty.tsx`.
- **`domain/status.ts`:** `readAgo(takenAt, now)`, `levelLabel`, `popoverState(snapshot, refresh)`, and `monthlyCost`, which the Briefing also uses so both print "$37". Each gets vitest cases.
- **`main.tsx`:** renders `TrayPopover` when `backend.view()` is `"tray"`, and `App` otherwise.
- **`App.tsx`:**
  - Listens to `onSnapshotChanged` and `onOpenTarget`.
  - If the main window shows another snapshot (a file or the demo), opening an item first switches it to the stored scan.

New members of the `Backend` port (`platform/backend.ts`):

| Member | `tauri.ts` | `web.ts` |
|---|---|---|
| `can.tray: "menu bar" \| "system tray" \| null` | depends on the OS | `null` |
| `view(): "main" \| "tray"` | window label | `"tray"` when the URL has `?view=tray` |
| `openMain(target?)`, `hidePopover()` | commands | no-op |
| `refreshStored()` | `refresh_stored` | rejects with "Refreshing needs the desktop app." |
| `refreshState()`, `onRefreshState(l)` | a command, and the `refresh-state` event | always idle; the listener never fires |
| `onSnapshotChanged(l)`, `onOpenTarget(l)` | events | never fire |
| `settings()`, `saveSettings(s)` | commands | `null` / rejects |

`RefreshState` is defined in `backend.ts`. It is one of `idle`, `running {step}`, `failed {message}` or `sign_in_needed {profile}`, plus `readsErrorLogs` and `nextAt`.

### Platforms

| | macOS | Windows | Linux |
|---|---|---|---|
| Left click | toggles the popover | toggles the popover | sends no event |
| Right click | menu | menu | menu |
| Tooltip | yes | yes | unsupported; the status line is in the menu instead |
| Icon | template (monochrome) | colour and shape | colour and shape |
| Popover position | `TrayCenter`, constrained | `TrayCenter`, constrained | top right of the monitor |
| Close-to-tray default | on | on | off |

**Tray behaviour:**
- Tray events are not emitted on Linux, even though the icon shows and its menu still opens on right click ([system tray guide](https://v2.tauri.app/learn/system-tray/), [TrayIconEvent](https://docs.rs/tauri/2.0.0/tauri/tray/enum.TrayIconEvent.html)).
- `show_menu_on_left_click` and `tooltip` are unsupported on Linux. `icon_as_template` works on macOS only ([TrayIconBuilder](https://docs.rs/tauri/latest/tauri/tray/struct.TrayIconBuilder.html)).

**Popover position (positioner plugin):**
- `TrayCenter` puts the window above the icon. If that would leave the screen (the macOS menu bar, or a Windows taskbar at the top), it goes at or below the icon instead. `move_window_constrained` keeps the window on the monitor.
- The tray positions call `panic!("Tray position not set")` until a Click, Enter, Move or Leave event has reached `on_tray_event`. The driver therefore keeps its own "tray rect seen" flag and falls back to a fixed position when it is not set, which is always the case on Linux ([ext.rs](https://docs.rs/crate/tauri-plugin-positioner/2.3.1/source/src/ext.rs), [lib.rs](https://docs.rs/crate/tauri-plugin-positioner/2.3.1/source/src/lib.rs)).
- The plugin must be registered even when only its Rust trait is used.

**Other platform facts:**
- Close-to-tray is off by default on Linux because Tauri cannot tell whether the desktop shows tray icons. Without the icon, a hidden window would leave an invisible running app.
- Windows notifications only work for installed apps. Dev builds show PowerShell's name and icon ([notification](https://v2.tauri.app/plugin/notification)).
- Single-instance must be the first plugin registered. On Linux it uses DBus, and Snap and Flatpak need extra permissions for that ([single-instance](https://v2.tauri.app/plugin/single-instance/)).
- The Debian package needs `libappindicator3-1` for the tray ([Debian](https://v2.tauri.app/distribute/debian/)).
- The notification, autostart and single-instance plugins need Rust 1.90 or later ([autostart](https://v2.tauri.app/plugin/autostart)).

### Plugins and capabilities

- **`Cargo.toml`:**
  - `tauri` gets the `tray-icon` feature.
  - New dependencies: `tauri-plugin-positioner` (with the `tray-icon` feature), `tauri-plugin-notification`, `tauri-plugin-autostart` (`MacosLauncher::LaunchAgent`) and `tauri-plugin-single-instance`.
- **Capabilities:**
  - Every plugin is called from Rust, so no webview gets a plugin permission.
  - New `capabilities/tray.json`: `"windows": ["tray"]`, `"permissions": ["core:default"]`. `default.json` does not change.
  - App commands are allowed for all windows by default ([capabilities](https://v2.tauri.app/security/capabilities/)). If commands are ever restricted with `AppManifest::commands`, the tray window needs only the commands listed above.

### Costs, permissions, privacy

**Costs:**
- **Default:** no AWS call happens without a click. A manual "Refresh health" costs what it costs today.
- **Background refresh:**
  - GetMetricData is billed per metric requested and is never in the free tier ([CloudWatch pricing](https://aws.amazon.com/cloudwatch/pricing/)). That is why the settings view shows refreshes per day.
  - It never runs Logs Insights or discovery.
  - No new IAM actions are needed.

**Privacy:**
- Notifications are off by default. They can appear on lock screens, so they carry only the item's title: resource names, never error messages, counts or ARNs.
- The tooltip does not show the account id.
- `settings.json` holds the profile name, never credentials.

**Docs:** no ADR is needed, because no rule in AGENTS.md changes. When this is built, update the ports table in `docs/architecture.md` and the README.

## Acceptance criteria

Rust tests (domain and app):
- [ ] Given the demo account, when its snapshot is taken, then `status.level` is `act_now`, the headline is "2 to act on now, 3 to plan for", and `top` is, in order:
  - "Errors start at InvoiceProcessor", which opens the flow "Web requests to checkout";
  - "checkout-orders-db saturates first", which opens checkout-orders-db;
  - "InvoicesTable is on 2 flows", which opens InvoicesTable.
- [ ] Given the demo, then `flows_with_errors` is "Web requests to checkout" (45) then "MonthlyClose runs" (26), and `cleanup` is 5 resources at 36.50 USD a month.
- [ ] Given insights whose highest priority is Medium, then the level is `plan_for_it` and the headline is "Nothing urgent, N to plan for".
- [ ] Given only Low insights and nodes with metrics, then the level is `all_clear` with "Nothing needs attention". Given the same insights and no node with metrics or an alarm state, then the level is `not_measured`.
- [ ] Given a stored snapshot JSON without `status`, when it loads, then it parses, and `account_status` returns the same status a new snapshot would.
- [ ] `news` returns:
  - one notice, given `before` is `plan_for_it` and `after` is `act_now`;
  - `None`, given the same items before and after;
  - one notice naming the new item, given a new error-origin key while both are `act_now`;
  - exactly one notice, given both changes at once.
- [ ] Given `DesktopSettings::default()`, then `background` is `None` and `notifications` is false.
- [ ] Given nothing is stored, when `refresh_in_background` runs, then it returns `None` and the source double records no calls.
- [ ] Given a stored snapshot with error patterns, when `refresh_in_background` runs, then `read_health` is called once and `read_errors` is never called.
- [ ] `next_wait(1 h, n)` for n = 0, 1, 2, 3 is 1, 2, 4 and 8 h, and `next_wait(4 h, 3)` is 24 h.
- [ ] Given the source fails with `SignInNeeded`, then no further background refresh runs until a manual refresh or a scan succeeds.

Desktop (vitest for `domain/status.ts`, plus a manual pass on each OS):
- [ ] Given a fresh install, when sprawlr runs untouched for an hour, then no health refresh runs: the stored `taken_at` does not change and the driver log shows no refresh.
- [ ] macOS and Windows: left click toggles the popover next to the icon, right click opens the menu, and left click never opens the menu.
- [ ] Linux: right click opens the menu, "Show status" opens the popover at the top right, and nothing panics.
- [ ] The four icon states can be told apart by shape: on macOS in light and dark menu bars, and in greyscale screenshots on Windows and Linux.
- [ ] Given the popover opens, then:
  - focus is on the first item;
  - Tab and Shift+Tab visit every control in the order above and wrap;
  - Up and Down move within a list;
  - Enter opens the main window on that item and hides the popover.
- [ ] Given the popover is open, when Esc is pressed, another app is clicked or the icon is clicked, then the popover hides and stays hidden.
- [ ] Given the demo, the popover and the briefing show the same top items in the same order, and the same "about $37 a month".
- [ ] Given a 15-minute background refresh saves a snapshot while the main window shows an app with a resource selected, then the data updates and the place, environment and selection stay the same.
- [ ] macOS and Windows: closing the main window keeps the icon, the hint appears on the first close only, and "Quit sprawlr" removes the icon and ends the process. Linux with default settings: closing the window quits.
- [ ] Given sprawlr is running, when it is launched again, then the running main window comes to the front and there is still one icon.
- [ ] "Start sprawlr when I log in" is off on a fresh install. Turning it on makes `is_enabled()` true, and turning it off makes it false.
- [ ] Web: there is no tray. `/?view=tray` renders the demo popover with "Refresh health" disabled and "Refreshing needs the desktop app."

## Decisions (2026-11-24)

The open questions of the draft, as decided by the owner:

1. **Alarms raise the level.** A CloudWatch alarm firing on a production resource is an "Act now" item ("Alarm firing on X") unless an act-now insight already covers that resource.
2. **Stale after 24 hours.** An older status shows no level: the icon shows the "No status" shape and the tooltip says when it was last read.
3. **Old error patterns are dated.** Background refresh skips logs. Patterns read earlier than the health next to them say so ("Error messages from 08:00; background refreshes skip logs"). `LogErrors.read_at` records when.
4. **Once a day per item.** A notified item is not notified again for 24 hours, even if it goes away and comes back (`DesktopSettings.notified`).
5. **Notifications only inform.** Clicking one brings sprawlr forward (the OS default); no actions.
6. **macOS lives in the menu bar.** With the window closed, sprawlr uses the Accessory activation policy (no Dock icon); opening the window restores it.
7. **Launch at login starts in the tray.** Autostart passes `--tray`; the main window stays hidden. The label reads "Start sprawlr in the menu bar when I log in".
8. **Linux as drafted.** Top-right position and close-to-tray off by default. If the desktop has no tray at all, sprawlr runs without one instead of failing.
9. **No per-refresh cost estimate yet.** The UI keeps "a few cents of CloudWatch".
10. **Opening an item switches to the stored scan** without asking.
11. **Settings live in the popover only.**

## As built

Differences from the design above, and why:
- `AccountStatus.items` holds every item in briefing order and `top()` returns the first three. A separate `error_origins` list was not needed: `news` compares all act-now items, which also covers alarms.
- `Target` has a third variant, `Cleanup`, for the "Probably not needed" row.
- `status::of` takes the graph and the insights (findings are not needed).
- Close-to-tray off: closing the main window quits explicitly, because the hidden popover window would otherwise keep the process alive.
- No `popoverState` or component split beyond `TrayPopover`, `TraySettings`, `RefreshLine` and `LevelMark`; `can.tray` is a boolean and the place name ("menu bar" / "system tray") comes with the settings.
- Pause and back-off live in `sprawlr_app::Schedule` (tested), not in the driver. `refresh_in_background` takes `now` from its caller.
- Settings are changed through `SettingsStore::update`, one locked load-change-save, so a background save never undoes a setting just changed.
- `SignInNeeded` carries the adapter's own hint (`aws sso login --profile …`, `aws configure …`, or "AWS does not recognise the access keys"). The AWS adapter finds credential failures by type in the error chain and by STS error code, so an SSO endpoint outage does not pause background refresh.
- Health is only read when the profile signs in to the account the scan describes; otherwise the refresh fails and says which profile to use. The last profile is saved only after a read with it works.
- Without a tray (no AppIndicator on Linux; its loader panics, so startup catches that), sprawlr shows its window, ignores `--tray` and never hides to the tray.
- "Partly read" shows as "N parts could not be read" under the status when the last read had warnings.
- Positioner 2.4 returns an error rather than panicking when the tray position is unknown; the driver still uses its own "tray seen" flag.
- The desktop notification plugin reports permission as granted, so the "turned off in system settings" message only appears where the OS really refuses.
