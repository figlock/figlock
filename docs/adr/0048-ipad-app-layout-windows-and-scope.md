# ADR-0048: iPad app - one universal build, a split view layout, several windows, one lock

- Status: Accepted
- Date: 2026-11-28
- Decision: D49 in `docs/decisions.md`
- Amends: ADR-0041 (decision 1: the app is no longer iPhone only)
- Builds on: ADR-0041, ADR-0046
- Task: FIG-263 (GitHub #430). Unblocks FIG-264 (designs), FIG-265 (universal build), FIG-266 (split view screen).
  Supersedes the placeholder FIG-178.

## Context

ADR-0041 shipped the iPhone app and promised that iPad "reuses the same targets, with a layout ADR", and that nothing
in the core or in `Shared` assumes a phone screen. This is that ADR. What the code does today, and what it means on
an iPad:

- **Targets.** `apps/ios/project.yml` has three targets (app, AutoFill extension, UI tests), each with
  `TARGETED_DEVICE_FAMILY: "1"`. `Info.plist` allows portrait only. `SUPPORTS_MACCATALYST`,
  `SUPPORTS_MAC_DESIGNED_FOR_IPHONE_IPAD` and `SUPPORTS_XR_DESIGNED_FOR_IPHONE_IPAD` are already `NO`.
- **Root view.** `VaultRootView` is a two tab `TabView` (Vault, Settings), each tab a `NavigationStack`. Every edit,
  generator and Settings screen is a full height `.sheet`. That is right for a phone and wrong for a 13 inch screen.
- **One lock, but per scene events.** `AppLock` is one object in `AppEnvironment`, created once. But
  `FigLockApp` reacts to `scenePhase` by calling `lock.appDidEnterBackground()`, which starts the auto-lock clock
  (and with auto-lock "immediately" locks at once). On an iPad a person can have two windows; when one goes to the
  background while the other is being used, the vault would lock under their hands, and the clock would restart
  whenever any window comes back.
- **The unlocked session lives in a view.** `VaultRootView` builds the `VaultBrowser` (a full log replay) and the
  `VaultEditor` (the writer) in `.task` and keeps them in `@State`. A second window would build a second copy of
  both: twice the memory, and two writers on one log in one process.
- **Capture detection reads the main screen.** `CaptureMonitor` uses `UIScreen.main.isCaptured`. That says nothing
  about a window on an external display or one scene being recorded.
- **Share sheets.** The Emergency Kit share sheet is a `UIActivityViewController`. It is hosted as the content of a
  SwiftUI sheet, so it may need no popover anchor, but any such controller that UIKit presents as a popover on iPad
  needs one. FIG-265 found no other use; FIG-277 checks the kit share on an iPad.
- **Quick unlock.** ADR-0046 binds `QK` to `.biometryCurrentSet`. That works with Touch ID as with Face ID, but the
  strings say "Face ID" (`NSFaceIDUsageDescription`, Settings, onboarding).
- **The extension.** The credential provider runs on iPad too, in a popover or a form sized sheet, with the same
  memory limit and the same "never derive a key" rule (ADR-0041 decision 7).

Everything below stays inside "ciphertext only on storage" and "no server of ours". iPad adds no new crypto.

## Decision

**1. One universal build on the existing targets.** `TARGETED_DEVICE_FAMILY` becomes `"1,2"` on all three targets.
Same bundle ids, same App Group, same keychain access group, one App Store record (a separate iPad target would
duplicate every one of those and double the App Store and TestFlight work). The app does not run as an enlarged
iPhone app: that mode has no Split View, no Stage Manager and no keyboard layout. Minimum stays **iOS 18** (every iPad
that runs iPadOS 18 has a Secure Enclave, and has Touch ID, Face ID or only a passcode).
`UIRequiresFullScreen` is never set. iPhone stays portrait only; on iPad all four orientations are allowed
(`UISupportedInterfaceOrientations~ipad`). This amends ADR-0041 decision 1.

**2. Layout: a split view where there is room, the iPhone stack where there is not.**
- Regular width (full screen, large Stage Manager window, Split View 1/2 on a 13 inch): `NavigationSplitView`
  with three columns: **sidebar** (All, Favourites, kinds, tags, Recently deleted), **item list** (search and the
  filter chips where they still make sense), **detail**. With nothing selected the detail column shows an empty state
  that carries no item data. In portrait on an 11 inch the sidebar starts hidden and the person can open it.
- Compact width (Split View 1/3, Slide Over, a narrow Stage Manager window): exactly the iPhone layout, `TabView`
  with Vault and Settings and a pushed detail. Resizing a window can cross the line at any time.
- **Navigation state lives in a model, not in the view.** A per scene `@Observable` model holds the sidebar filter,
  the search text, the selected item id and the list's scroll anchor. Both layouts read and write it, so a window
  that goes narrow and wide again keeps its selection and scroll position, and a rotation changes nothing. On compact
  width the model's selected id drives the pushed detail, the model's filter drives the chips.
- **No vault-derived state is written to disk by the UI.** No `@SceneStorage` or state restoration for the selected
  item, filter or search. After the app is relaunched the person sees the lock screen and then All with nothing
  selected. (Window positions and sizes are the system's and hold no vault data.)

**3. Presentation.**
- **Edit and new item:** a form sized sheet on regular width (`presentationSizing(.form)`), the full sheet of today
  on compact. **Settings:** a page sized sheet opened from the foot of the sidebar on regular width, the Settings
  tab on compact; the Files style tiles (FIG-257) stay. **Generator:** a popover from the toolbar button or the
  password field on regular width (`presentationCompactAdaptation(.sheet)` makes it a sheet on compact).
  **Delete and other confirmations:** a `confirmationDialog` anchored to the control that asked.
- **Onboarding, pairing, unlock and recovery** are not sheets on iPad. They are one centred column capped at form
  width (about 540 pt) on the app background; a sheet over an empty screen reads wrong. A sheet is used only for
  what comes over the vault.
- **Every UIKit controller that is presented as a popover on iPad (share sheet, document picker, print) is given a
  source anchor.** The kit share sheet is hosted as sheet content and is checked on an iPad in FIG-277.
- Concrete sizes, spacing and the 11 and 13 inch screens come from the approved designs (FIG-264), not from this ADR.

**4. Several windows, one lock, one session.**
- The app supports several windows (`UIApplicationSupportsMultipleScenes`), so a person can compare two items or keep
  a login open next to a form. All windows show the same vault.
- **One `AppLock`, one unlocked session per process.** The `VaultBrowser`, the `VaultEditor` and the
  `ClipboardCopier` move out of `VaultRootView` into the app environment, built once when the vault unlocks and
  dropped when it locks. A scene holds only its own navigation model and what it presents. There is one writer, so
  windows cannot disagree about the log. An item deleted in one window that is open in another shows "no longer
  available" there.
- **Lock applies to every window at once.** Unlocking in one window unlocks all; locking from any, or by the
  auto-lock rule, locks all and clears everything shown (the root swap of today already does that).
- **The auto-lock clock follows the app, not a scene.** `appDidEnterBackground` runs when the last window has left the
  foreground (the app-level `scenePhase`, which is the aggregate over scenes), not when any one window does, and
  `appWillEnterForeground` when the first returns. Closing one window locks nothing.
- **The privacy cover is per scene.** Each window covers itself when its own scene is not active or is being
  captured. Only the window that is active shows the quick unlock prompt (one Face ID or Touch ID sheet at a time);
  the others wait and open when the lock opens.
- **Stage Manager thumbnails and the app switcher** must not show secrets. Whether the system's thumbnails honour the
  secure layer of `SecretScreen` for a window that is visible but not focused is **measured on a device in FIG-269**.
  If a thumbnail can show content, the cover rule widens until it cannot, even at a small cost to a window that is
  visible next to the focused one.

**5. External display, mirroring and recording.**
- Capture is read per scene (`UITraitCollection.sceneCaptureState`, with `UIScreen.isCaptured` kept as a second
  signal), not from `UIScreen.main`. A scene that is mirrored, AirPlayed or recorded shows the cover (as the iPhone
  does today), whatever other windows do.
- A window that the person moved to an **extended** external display is shown normally: it is the person's own
  second monitor, not a capture. A screen recording of that display is a capture and is covered.
- FIG-270 tests mirroring, an extended display and recording on a device and fixes the detail.

**6. Keyboard and pointer are first class.**
- Every action has a keyboard shortcut declared with `keyboardShortcut` or `commands`, so the hold-to-show menu lists
  them. Proposed set (FIG-268 finalises it): New item `Cmd+N`, Search `Cmd+F`, Edit `Cmd+E`, Delete `Cmd+Delete`,
  Copy password `Shift+Cmd+C`, Copy username `Shift+Cmd+U`, Generator `Cmd+G`, Lock now `Cmd+L`, Settings `Cmd+,`,
  New window `Cmd+Option+N`, Close sheet `Esc`, Submit unlock `Return`. Standard `Cmd+C` keeps copying selected text.
- Tab order is the visual order, up and down arrows move in the list, and every swipe action also has a context menu
  and a pointer hover state.
- **Secrets are never draggable.** No drag and drop of a secret value or item out of the app, no `draggable` on a
  password, and the pasteboard rules of ADR-0041 decision 6 (local only, expiring) hold for every copy. No Handoff of
  vault state, no Spotlight indexing (titles are encrypted, ADR-0041).

**7. Touch ID iPads next to Face ID.** The biometric name is read from `LAContext.biometryType` and used in every
string, accessibility label and settings row; nothing hard-codes "Face ID". `NSFaceIDUsageDescription` stays for
the iPads that have it. ADR-0046 applies unchanged: `.biometryCurrentSet`, and a changed set (a new fingerprint)
ends quick unlock. An iPad with no biometrics enrolled has quick unlock off, as an iPhone without them does. FIG-271
tests Touch ID on an iPad simulator or device.

**8. The AutoFill extension.** Runs on iPad as it does on iPhone (read only, no derivation, quick unlock only). Its
screen adapts to the popover or form sheet the system gives it and to a hardware keyboard; it is not a copy of the
split view. FIG-272.

**9. Agent approvals.** The approval card (ADR-0041 decision 9) comes as a sheet in the active window of an iPad that
is unlocked and shows nothing in a window that is covered. Approval still needs biometrics and signs with the device
key. FIG-274.

**10. Out of scope, on purpose.** Apple Pencil features (Scribble in text fields comes free and is not tested as a
feature), widgets, Live Activities, Mac Catalyst and "Designed for iPad" on the Mac (the Mac has the desktop app,
ADR-0040; the three build settings stay `NO`), visionOS, a separate iPad app, an iPad only feature that the iPhone
does not have, and landscape on iPhone.

## Consequences

- iPhone behaviour does not change except where it is made correct for several scenes (the lock clock and the capture
  signal). Those two changes land with the universal build (FIG-265) and the windows task (FIG-269) with tests that
  run on the iPhone too.
- Moving the browser and editor into the app environment is a refactor of `VaultRootView` that FIG-266 needs first.
  It is part of FIG-266, in its own commit, with the existing iPhone UI tests green before the split view is added.
- Two layouts means two sets of screenshots and UI tests (FIG-276), and size class changes become a tested path.
- A secret on a second window is more exposure than before. The mitigations are the single lock, the per scene cover,
  no drag and drop, and the device measurement in FIG-269. This is called out for the threat model update (FIG-275).
- The designs (FIG-264) and the keyboard set (FIG-268) are open details this ADR deliberately leaves to them.
- `docs/prd.md` (phase table), `apps/ios/README.md` and the website say "iPhone only"; they are updated when the iPad
  build ships (FIG-265, FIG-279), not by this ADR.

## Alternatives considered

- **A separate iPad target or app:** two bundle ids, two App Store records, two keychain and App Group set ups, and a
  copy of every screen. Rejected; one universal target is the standard way and the code is SwiftUI.
- **Run the iPhone app unchanged on iPad:** no Split View, no Stage Manager, no keyboard layout, a small window.
  Rejected.
- **`UIRequiresFullScreen` (no multitasking):** removes the multi window problem by removing multitasking. Rejected: a
  password manager is used next to another app (Slide Over, Split View) more than most apps.
- **A single window only (no `UIApplicationSupportsMultipleScenes`):** simplest and least exposure. Rejected because
  two windows of one vault is the iPad feature that justifies the layout work, and the single lock and single session
  in decision 4 keep it safe. It stays an easy fallback: removing the plist key turns it off.
- **`TabView` with `.sidebarAdaptable` and a nested `NavigationSplitView`:** puts two sidebars on screen (tabs and
  filters) and changes the iPhone tab structure. Rejected in favour of a split view on regular width and the
  unchanged iPhone layout on compact, joined by the navigation model.
- **`NavigationSplitView` on iPhone too:** one root, but it rewrites the shipped iPhone navigation (FIG-257, FIG-259)
  for no iPhone gain. Rejected.
- **Restore the selected item after a relaunch (`@SceneStorage`):** nicer, but it writes which item was viewed to disk
  outside the vault. Rejected.
- **Edit as a popover, Settings as a pushed column:** an edit form is too tall for a popover, and a Settings tree
  pushed inside the item column mixes two tasks. Rejected.
