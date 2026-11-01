# Plugin hosting

Third-party plugins are Audio Units and CLAP plugins (VST3 is out for
good: its license would force the project's). Both end up as a
`Processor` in a chain, so the chain, the mixer and the project file never
know they are holding a plugin. The code is `crates/hosting`; this page
covers what M4 added on top of the M0 wrappers.

## Two threads, one plugin: the handle

The spike left one decision to M4: a `Processor` is a single object that
moves between the UI thread and the audio thread with the graph swap,
while plugin APIs split a plugin into a main-thread half and an
audio-thread half and expect the host to use both at once (save state,
show an editor, read parameters) while audio runs. The answer is the
second of the two options the spike offered: the engine keeps the
main-thread half on the UI thread and talks to the audio half through
the queues it already has.

Concretely, `Processor::handle()` returns a `cantina_dsp::Handle` once,
before the processor goes into a graph. Stock pedals return nothing: all
there is to them is their parameters, which the engine already tracks.
A plugin returns the UI-thread side of itself. The shell keeps it next to
the node id (`LivePedal` in `src-tauri/src/engine.rs`) for as long as the
pedal is in the chain, and uses it for:

- **state**: `state()` and `set_state()` while the processor runs;
- **latency**: `latency()` as it is now, since a plugin can change it
  after it was prepared;
- **changes the plugin made on its own**: `take_changes()` returns the
  parameters that moved since the last call, from an editor or a preset
  loaded inside the plugin. The shell polls every handle a few times a
  second from the meter pump and records each change as a `SetParam`
  edit (coalesced like a knob drag), so the generic knobs and undo
  follow whatever the plugin's own UI does;
- **the format's own features** through `as_any()`, which is how the
  editor windows reach the Audio Unit instance or the CLAP GUI
  extension.

Nothing on a handle is ever called from the audio thread. For an Audio
Unit the two halves share one `AudioComponentInstance` behind an `Arc`
(property and parameter calls on an AUv2 instance are safe from another
thread while it renders, which is how every host works), and the
instance is disposed when both halves are gone. The processor's own
`state()` and `set_state()` still exist for a processor that is not in a
graph, which is how a project loads: the state goes in before the node
is added.

## Audio Units

`AudioUnitProcessor` instantiates the component for a fixed channel
count, reads the parameter list (name, range, unit, default) so the UI
can show knobs before audio runs, and configures stream formats and the
render callback in `prepare`. `process` copies the input aside, lets the
unit pull it through the callback and renders over the buffer. A unit
that fails to configure or render bypasses and counts the failure; it
never stops the stream.

State is the unit's `kAudioUnitProperty_ClassInfo`, the property list
every host saves as a preset, serialized as a binary plist. Restoring it
sets the property back and notifies parameter listeners, so an editor
view redraws. Latency is `kAudioUnitProperty_Latency` times the output
rate. `AudioUnitHandle::take_changes` compares every parameter against
the value it last reported, which is cheap (a few dozen property reads)
and needs no listener on a run loop.

Apple's AUDelay, which ships with every Mac, is the test subject: an
impulse comes out at the delay time, the plist state round-trips into a
fresh instance, and the handle sees a parameter change made behind its
back while the processor is prepared and rendering.

## CLAP

`ClapProcessor` goes through [clack](https://github.com/prokopyl/clack),
whose types enforce CLAP's thread split. The audio half, clack's
`PluginAudioProcessor`, lives in the processor and moves with it. The
main half, clack's `PluginInstance`, lives behind a mutex in a
`ClapShared` that the processor and its `ClapHandle` both hold. The
processor locks it only in `prepare`, `reset` and `drop`, which run off
the audio thread; the handle locks it for state, latency and parameter
flushes. Nothing locks it on the audio thread, which is what keeps the
shared instance honest with CLAP's "main thread" rule.

Parameter changes from the UI are queued as CLAP events and delivered at
the next `process`, the sanctioned real-time path; before the plugin is
active they go through a main-thread flush. Changes the plugin makes on
its own come back as output events from `process` and cross to the handle
over a lock-free ring, so `take_changes` costs nothing on the audio
thread. While the plugin is inactive and asks for a flush
(`host_params.request_flush`), the handle runs one on the main thread and
collects what comes out. The host also implements `host_latency`, so a
plugin that changes its latency is noticed (`ClapHandle::latency_changed`)
and `host_params.rescan` is recorded.

CLAP parameters carry no unit, so the wrapper asks the plugin to format
each parameter's default value (`value_to_text`) and reads the suffix:
`dB`, `Hz`, `kHz`, `s`, `ms` or `%`. A plugin that formats nothing useful
falls back to a guess from the name.

State is the plugin's `state` extension, saved and loaded on the main
thread while the plugin is active, which CLAP allows and the plugin is
responsible for making safe. A plugin without the extension saves its
parameter values.

The workspace's own test plugin (`crates/clap-fixture`) is the subject:
it applies a gain and a delay it reports as latency, saves its two
values, and echoes every parameter change it is sent as an output event,
the way a plugin tells its host about a change. The tests check all of
that through both the processor and the handle, including state saved
while the plugin is active and restored into a fresh instance.

## State in the project

A plugin's state lives in the project as `plugins/<pedal id>.state`: a
four-byte magic, the pedal's spec, then the plugin's own bytes. The spec
is there so a file left behind by a removed pedal can never land in a
different plugin that later gets the same id. The shell reads every
state file when a project opens, writes them all on Save (after asking
each running plugin for its current state), and refreshes them from the
running plugins whenever the stream closes. A stream rebuild, after a
sample rate or buffer size change or an interface going away, drops the
whole graph and builds it again from the project, so each plugin is
created anew, given its state, then the project's parameter values on
top (those are what undo tracks, so they win), and only then added to
the graph. Files for pedals that are gone stay, like audio files for
deleted clips, so undoing the removal finds the state again.

The project's parameter values and the plugin's state overlap: both hold
the parameters. The polling above keeps the values current, so a save
stores a state and a value list that agree, and a load that applies the
values over the state changes nothing. Both formats behave the same from
the chain's point of view: the shell holds a `Handle` and never asks
which kind it is.

## Editor windows

Audio Unit and CLAP editors are native `NSView`s. Each opens in its own
`NSWindow`, added as a child of the main window so it stays above it and
moves with it, never embedded in the web view: the plan's rule, since a
native view inside WebKit's view is a fight nobody wins. The code is
`crates/hosting/src/editor.rs`; it runs on the main thread only, which
AppKit demands, and takes a `MainThreadMarker` as proof.

- **Audio Units** ship a view through the `CocoaUI` property: a bundle
  and the name of a factory class in it, which is loaded and asked for
  the view (`uiViewForAudioUnit:withSize:`). A unit without one gets
  Apple's `AUGenericView` from CoreAudioKit, so every Audio Unit opens a
  window: the vendor's when there is one, Apple's generic knobs
  otherwise.
- **CLAP** plugins embed their view into a container view we hand them
  through the `gui` extension (`create`, `set_parent`, `show`). A plugin
  without the extension, or without Cocoa support, has no window; the
  pedal card shows no Edit button and the generic knobs are its editor.

The window holds its own share of the plugin's main-thread half (the
same `Arc` the handle holds), so it can be closed after the pedal is
gone without the view outliving the instance. The shell keeps the open
editors in a cell only the main thread may reach, schedules every open,
close and tick onto the main thread with Tauri's `run_on_main_thread`,
and closes a plugin's editor when the pedal is removed or the stream
closes (a rebuild recreates every plugin).

Resizing is polled rather than observed: a few times a second the shell
ticks every open editor on the main thread. An Audio Unit view that
changed its own frame gets the window resized to match (the generic
view does the opposite: the window drives it, so it is resizable). A
CLAP plugin asks through `host_gui.request_resize`, which the host
grants by recording the size; the tick applies it to the window and
confirms it with `set_size`. A user resize of a resizable CLAP window
goes to the plugin through `adjust_size` and `set_size`. A window the
user closed is noticed on the tick and forgotten, and its position is
saved in `settings.json` by spec, so the same plugin reopens where it
was, in any project.

Parameter changes made in a vendor's editor reach the generic knobs and
undo through the handle polling above: an Audio Unit view calls
`AudioUnitSetParameter`, which the next `take_changes` sees as a
difference; a CLAP plugin reports them as output events.

The editor test (`crates/hosting/tests/editor.rs`) has its own `main`
so it runs on the main thread: it opens AUDelay's editor, Apple's
generic view, checks it laid out and sits where it was asked, closes it
with the processor already gone, and checks the GUI-less test plugin is
refused with no window.

## The scanner

Listing what is installed is cheap and runs no plugin code: Audio Units
come from the component registry, CLAP bundles from
`~/Library/Audio/Plug-Ins/CLAP` and `/Library/Audio/Plug-Ins/CLAP`,
searched recursively. Learning anything more (the channel layout, whether
there is an editor, which plugins a CLAP bundle holds) means loading and
instantiating the plugin, and some plugin always crashes doing that. So
the scanner does it in a child process.

The child is the Cantina binary itself, run with `--scan-plugins`, so
nothing extra has to be bundled. The parent (`scan::Scanner`) writes one
JSON **source** per line to the child's stdin: an Audio Unit component
(`type:subtype:manufacturer`, name, version) or a CLAP bundle (path, newest
modification time inside it, total size). The child prints one line per
event, each prefixed with `@cantina ` so a plugin that writes to stdout
while loading cannot be mistaken for a result:

```
@cantina {"begin":"clap:/Library/Audio/Plug-Ins/CLAP/Thing.clap"}
@cantina {"examining":"Thing"}
@cantina {"plugin":{"format":"clap","spec":"clap:/…/Thing.clap#com.vendor.thing","name":"Thing",…}}
@cantina {"end":"clap:/Library/Audio/Plug-Ins/CLAP/Thing.clap"}
```

Each plugin is reported the moment it is described. When the child dies,
the parent knows which source it was in (the `begin` without an `end`)
and which plugin it was instantiating (the last `examining`), records
that as a **failure** with the exit status, and starts a new child on the
rest. What the dead child reported before it died is kept. A child that
prints nothing for thirty seconds is killed and its source recorded the
same way. The hosting crate's tests exercise both: the CLAP test fixture
carries a second plugin that aborts the process when instantiated, and a
`sleep` stands in for a hung child.

### The cache and rescan

The result is a `PluginList`, saved as `plugins.json` in the app's data
folder (`~/Library/Application Support/com.jbouder.cantina/`). Every entry
carries the spec the engine builds it from (`au:aufx:dely:appl`,
`clap:<bundle path>#<plugin id>`), name, vendor, version, main input and
output channel counts, and whether it ships an editor. The list also
remembers every source with the fingerprint it had. A rescan lists the
sources again and hands the child only the ones whose fingerprint
changed or that are new; unchanged sources keep their cached entries and
failures, and sources that went away are dropped. A full rescan hands it
everything. The first launch, with no cache, scans when the UI first asks
for the list.

In the app: the **+** slot at the end of a pedalboard opens the picker
(`ui/src/components/PedalPicker.tsx`), whose Plugins section lists them
by format and vendor with Rescan and Full rescan, and the failed plugins
under a disclosure with why. Choosing one adds it to the end of the chain.
