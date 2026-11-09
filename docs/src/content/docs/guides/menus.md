---
title: "Menus"
description: "What each item in Cantina's menu bar does, with its shortcut."
sidebar:
  order: 4
---

Everything in Cantina's menu bar is also somewhere in the window; the menus
are where a Mac app's commands are expected to be, with their shortcuts
beside them. An item that cannot apply right now, such as **Save** with
nothing to save, is greyed out. The [shortcut reference](/reference/shortcuts/)
lists every key on one page.

The menus grow with Cantina: the assistant, tabs, rigs and jam mode each
add their own items when they arrive.

## Cantina

| Item | Shortcut | What it does |
| --- | --- | --- |
| About Cantina | | The version, the license and the credits for the drum samples and impulse responses. |
| Check for Updates… | | Opens Settings and asks for a newer release. If there is one, a banner offers to install it. |
| Settings… | ⌘, | Audio device, buffer size, the latency warning, skins, motion, what launch opens, and updates. |
| Hide, Hide Others, Show All, Quit | ⌘H, ⌥⌘H, ⌘Q | As in every Mac app. |

## File

| Item | Shortcut | What it does |
| --- | --- | --- |
| New Project… | ⌘N | Opens **Projects** with the name of a new project ready to type. The start dialog at launch also makes new projects, with a rate, tempo, meter and tracks. |
| Open… | ⌘O | Opens **Projects** to pick another project. |
| Open Recent | | The last ten projects you had open, newest first. **Clear Menu** empties it. With unsaved edits, Cantina asks before dropping them. |
| Close Window | ⌘W | Closes the window. |
| Save | ⌘S | Writes the project to its folder. |
| Duplicate… | ⇧⌘S | Copies the project as it is now, unsaved edits and all, to a new name, and switches to the copy. The original stays as it was last saved. Its unsaved edits stay recoverable: it offers them back when you open it. |
| Rename… | | Saves the project and renames its folder. Undo history carries on. |
| Import Audio… | ⇧⌘I | Copies WAV files into the project and puts them on the selected audio track at the playhead, one after another, or on a new track when a drum track is selected. A file at another sample rate is converted in the copy, and one with more than two channels is folded to stereo. The original file is never changed. Undo takes each file back off. |
| Import Pedal… | | Adds a `.pedal.json` to your pedals, and to the selected track's chain. |
| Import Pattern… | | Adds a `.pattern.json` to the project, onto the selected drum track. |
| Import Kit… | | Copies a kit folder (one with a `kit.json`) into `~/Music/Cantina/Kits`, after checking it loads, so drum tracks can pick it. |
| Export Mixdown… | ⌘E | The [export sheet](/guides/mixdown/#export-the-mix). |
| Bounce Track in Place | | [Bounces](/guides/mixdown/#bounce-a-track-in-place) the selected track, as **Bounce** in its editor does. |
| Show Project in Finder | | Opens a Finder window with the project's folder selected. |
| Show Library in Finder | | Opens `~/Music/Cantina`, which holds your projects and the Pedals, Patterns, Kits and IRs folders. |

At launch Cantina shows the [start dialog](/guides/getting-started/#open-cantina)
rather than a project; **New Project…** and **Open…** work as above
whether it is up or not. To open the last project at launch instead, turn
on **Open the last project at launch** in **Settings → Launch**.

## Edit

| Item | Shortcut | What it does |
| --- | --- | --- |
| Undo, Redo | ⌘Z, ⇧⌘Z | Undo and redo the last edit to the project. The item names the edit, as in **Undo Move Clip** or **Redo Add Clip Guitar 3**, so you know what it will change. In a text field they undo your typing instead. |
| Cut, Copy, Paste | ⌘X, ⌘C, ⌘V | In a text field, the text. With the timeline focused, the selected clip: Cut and Copy keep it, and Paste puts a copy on the selected track at the playhead, or in the first space after it that holds the clip. Audio goes on audio tracks and patterns on drum tracks. The copy plays the same recording, so pasting never copies audio. |
| Duplicate Clip | ⌘D | A copy of the selected clip in the first space after it. |
| Delete Clip | ⌫ in the timeline | Removes the selected clip. Its recording stays in the project, so Undo, or Paste after a Cut, brings it back. |
| Select All | ⌘A | In a text field. The timeline selects one clip at a time. |
| Split Clip at Playhead | S in the timeline | Cuts the selected clip in two at the playhead. |

## Transport

| Item | Key | What it does |
| --- | --- | --- |
| Play / Stop | Space | Plays from the playhead, or stops. The item reads Stop while the transport rolls. |
| Record | R | Records the armed tracks. See [recording](/guides/recording/). |
| Count In | | Checked: Record counts in four beats before the take starts. Unchecked: the take starts at once. Remembered for next time. |
| Loop | L | Checked while playback loops over the loop region. With no region yet, it loops four bars from the playhead's bar. See [looping](/guides/recording/#loop-a-section). |
| Loop Selected Clip | | Sets the loop to the selected clip and turns it on. |
| Metronome | K | Turns the click on and off; checked while it is on. |
| Go to Start | Home | Moves the playhead to the start of the song. |
| Go to End | End | Moves the playhead to the end of the last clip. |
| Tempo… | | Puts the cursor in the tempo field. |

## Track

These act on the selected track, and are greyed out with none.

| Item | Shortcut | What it does |
| --- | --- | --- |
| New Audio Track | ⌥⌘N | Adds an audio track with an empty pedalboard and selects it. |
| New Drum Track | | Adds a drum track with the first kit and selects it. |
| Arm for Recording | | Checked while the track is armed: it monitors its input and records with Record. Audio tracks only. |
| Mute, Solo | | Checked while the track is muted or soloed, as the buttons on its header. |
| Rename Track… | | Opens the track's name for typing. |
| Remove Track | | Removes the track, its clips and its pedals. Undo brings it back. |
| Add Pedal… | | The pedal picker, adding to the end of the track's chain. |
| Bypass Chain | | Checked when every pedal on the track is off. Choosing it turns them all off, or all back on, as one edit. |

The keys in these menus have no ⌘, so they do nothing while you are typing
in a text field. The menu shows them without a shortcut for that reason.

## View

| Item | Shortcut | What it does |
| --- | --- | --- |
| Arrange | | Back to the arrangement: closes the stage, performance mode, the pedal designer and the jam. |
| Jam | J | Opens or leaves [jam mode](/guides/jam/), checked while it is open. While jamming, Play plays the jam and Record is off. |
| Pedal Designer | | Opens the selected track's first designed pedal in the [designer](/guides/designer/), or starts a new design on it. |
| Stage | | The [stage](/guides/stage/) sheet. |
| Performance Mode | P | The stage full screen. |
| Tapster (Assistant) | ⌘J | Shows or hides the [assistant](/guides/assistant/)'s chat panel. Only once the assistant is connected. Called Assistant with the droid persona off. |
| Skin | | One item per [skin](/guides/skins/), the current one checked. The same choice as in Settings. |
| Zoom In, Zoom Out, Zoom to Fit | ⌘=, ⌘−, ⌘0 | Zoom the timeline, or fit the whole song. |
| Show Meters | | Checked while the in and out meters show under the toolbar. Remembered for next time. |
| Enter Full Screen | ⌃⌘F | The window full screen. |

## Window

| Item | Shortcut | What it does |
| --- | --- | --- |
| Minimize, Zoom | ⌘M | As in every Mac app. |
| Stage on Second Display | | Opens the stage in its own window on another display, as performance mode's **Second display** does. |
| Bring All to Front | | As in every Mac app. |

## Help

| Item | What it does |
| --- | --- |
| Search | macOS's own: finds any menu item by name. |
| Cantina Help | Opens this site. |
| Keyboard Shortcuts | Every shortcut, by menu and by part of the window. The same list as the [shortcut reference](/reference/shortcuts/). |
| Release Notes | Opens the releases on GitHub. |
| Report an Issue… | Opens a new GitHub issue with your Cantina version filled in. |
