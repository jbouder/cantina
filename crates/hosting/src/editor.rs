//! Native editor windows for plugins.
//!
//! Audio Unit and CLAP editors are `NSView`s. Each opens in its own
//! `NSWindow`, a child of the main window so it stays above it and moves
//! with it, rather than being embedded in the web view: the plan's rule.
//! An Audio Unit's view comes from its `CocoaUI` property, a bundle and a
//! factory class that makes the view; a unit without one gets Apple's
//! `AUGenericView`, so every Audio Unit has a window. A CLAP plugin's
//! view is embedded by the plugin into a container view we give it
//! through the `gui` extension; a plugin without the extension, or
//! without Cocoa support, has no window and the generic knobs in the
//! web UI are its editor.
//!
//! Everything here runs on the main thread, which AppKit demands, and
//! takes a [`MainThreadMarker`] as proof. The window holds the plugin's
//! main-thread half (the `Arc` the handle shares), so closing it late is
//! always safe: the plugin cannot have gone away under its own view.
//!
//! Resizing works by polling: the shell calls [`Editor::tick`] a few
//! times a second on the main thread. An Audio Unit view that changed
//! its own frame gets the window resized to match; a CLAP plugin's
//! resize request, recorded by the host callbacks, is granted there; and
//! a window the user closed is reported so the shell can forget it.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize, PluginGui, Window};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSFloatingWindowLevel, NSView, NSWindow,
    NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_audio_toolbox::{
    kAudioUnitProperty_CocoaUI, kAudioUnitScope_Global, AudioUnit, AudioUnitCocoaViewInfo,
    AudioUnitGetProperty, AudioUnitGetPropertyInfo,
};
use objc2_core_audio_kit::AUGenericView;
use objc2_core_foundation::{CFRetained, CFString, CFURL};
use objc2_foundation::{NSBundle, NSPoint, NSRect, NSSize, NSString, NSURL};

use crate::au::{AudioUnitHandle, UnitOwner};
use crate::clap::{ClapHandle, ClapShared};

/// A plugin's editor in its own window.
pub struct Editor {
    window: Retained<NSWindow>,
    /// The window's content view; the plugin's view sits inside it.
    container: Retained<NSView>,
    content: Content,
    parent: Option<Retained<NSWindow>>,
    /// Whether the plugin's own view sizes the window (false) or the
    /// user does (true).
    resizable: bool,
    open: bool,
}

enum Content {
    Au {
        view: Retained<NSView>,
        _unit: Arc<UnitOwner>,
    },
    Clap {
        shared: Arc<ClapShared>,
        /// The size last given to the plugin, to notice a user resize.
        size: GuiSize,
    },
}

/// Where a window's bottom-left corner is on screen, in points.
pub type Origin = (f64, f64);

fn rect(size: NSSize) -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), size)
}

fn gui_size(size: NSSize) -> GuiSize {
    GuiSize {
        width: size.width.round().max(1.0) as u32,
        height: size.height.round().max(1.0) as u32,
    }
}

fn ns_size(size: GuiSize) -> NSSize {
    NSSize::new(size.width as f64, size.height as f64)
}

impl Editor {
    /// Opens an Audio Unit's editor: its Cocoa view when it ships one,
    /// Apple's generic view otherwise.
    pub fn open_au(
        mtm: MainThreadMarker,
        handle: &AudioUnitHandle,
        parent: Option<NonNull<c_void>>,
        origin: Option<Origin>,
        title: &str,
    ) -> Result<Editor> {
        let unit = handle.unit();
        let (view, custom) = match cocoa_view(mtm, unit) {
            Some(view) => (view, true),
            None => {
                let generic = unsafe { AUGenericView::initWithAudioUnit(mtm.alloc(), unit) };
                (Retained::into_super(generic), false)
            }
        };
        let mut size = view.frame().size;
        if size.width < 1.0 || size.height < 1.0 {
            size = NSSize::new(480.0, 320.0);
            view.setFrameSize(size);
        }
        // The generic view lays itself out to whatever size it is given;
        // a vendor's view decides its own.
        let resizable = !custom;
        let editor = Self::build(
            mtm,
            &view,
            size,
            resizable,
            parent,
            origin,
            title,
            Content::Au {
                view: view.clone(),
                _unit: handle.owner(),
            },
        );
        Ok(editor)
    }

    /// Opens a CLAP plugin's editor through its `gui` extension. Fails,
    /// with no window, when the plugin has no Cocoa editor.
    pub fn open_clap(
        mtm: MainThreadMarker,
        handle: &ClapHandle,
        parent: Option<NonNull<c_void>>,
        origin: Option<Origin>,
        title: &str,
    ) -> Result<Editor> {
        let shared = Arc::clone(handle.shared());
        let (size, resizable, container) = {
            let mut instance = shared.lock();
            let gui = instance
                .plugin_shared_handle()
                .get_extension::<PluginGui>()
                .with_context(|| format!("{} has no editor", handle.info().name))?;
            let plugin = instance.plugin_handle();
            let config = GuiConfiguration {
                api_type: GuiApiType::COCOA,
                is_floating: false,
            };
            if !gui.is_api_supported(&plugin, config) {
                bail!("{} has no Cocoa editor", handle.info().name);
            }
            gui.create(&plugin, config)
                .map_err(|e| anyhow!("{e}"))
                .with_context(|| format!("creating the editor of {}", handle.info().name))?;
            let resizable = gui.can_resize(&plugin);
            let size = gui.get_size(&plugin).unwrap_or(GuiSize {
                width: 480,
                height: 320,
            });
            let container = NSView::initWithFrame(mtm.alloc(), rect(ns_size(size)));
            let window = Window::from_generic_ptr(
                GuiApiType::COCOA,
                Retained::as_ptr(&container) as *mut c_void,
            );
            // SAFETY: the container lives as long as the editor, and the
            // editor destroys the GUI before letting go of it.
            let embedded = unsafe { gui.set_parent(&plugin, window) };
            if let Err(e) = embedded {
                gui.destroy(&plugin);
                bail!("{} refused the window: {e}", handle.info().name);
            }
            if let Err(e) = gui.show(&plugin) {
                gui.destroy(&plugin);
                bail!("{} would not show its editor: {e}", handle.info().name);
            }
            (size, resizable, container)
        };
        Ok(Self::build_with_container(
            mtm,
            container,
            ns_size(size),
            resizable,
            parent,
            origin,
            title,
            Content::Clap { shared, size },
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        mtm: MainThreadMarker,
        view: &NSView,
        size: NSSize,
        resizable: bool,
        parent: Option<NonNull<c_void>>,
        origin: Option<Origin>,
        title: &str,
        content: Content,
    ) -> Editor {
        let container = NSView::initWithFrame(mtm.alloc(), rect(size));
        view.setFrameOrigin(NSPoint::new(0.0, 0.0));
        if resizable {
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
        }
        container.addSubview(view);
        Self::build_with_container(
            mtm, container, size, resizable, parent, origin, title, content,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_with_container(
        mtm: MainThreadMarker,
        container: Retained<NSView>,
        size: NSSize,
        resizable: bool,
        parent: Option<NonNull<c_void>>,
        origin: Option<Origin>,
        title: &str,
        content: Content,
    ) -> Editor {
        let mut style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable;
        if resizable {
            style |= NSWindowStyleMask::Resizable;
        }
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                rect(size),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // We hold the only reference that matters; AppKit must not free
        // the window behind our back when the user closes it.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        window.setContentView(Some(&container));
        // SAFETY: the pointer is the main window's NSWindow, alive for
        // the life of the app.
        let parent = parent.and_then(|p| unsafe { Retained::retain(p.as_ptr() as *mut NSWindow) });
        match &parent {
            Some(parent) => unsafe {
                parent.addChildWindow_ordered(&window, NSWindowOrderingMode::Above)
            },
            None => window.setLevel(NSFloatingWindowLevel),
        }
        match origin {
            Some((x, y)) => window.setFrameOrigin(NSPoint::new(x, y)),
            None => window.center(),
        }
        window.makeKeyAndOrderFront(None);
        Editor {
            window,
            container,
            content,
            parent,
            resizable,
            open: true,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The window's content size in points.
    pub fn size(&self) -> (f64, f64) {
        let size = self.container.frame().size;
        (size.width, size.height)
    }

    /// Where the window is, to remember for next time.
    pub fn origin(&self) -> Origin {
        let origin = self.window.frame().origin;
        (origin.x, origin.y)
    }

    /// Brings the window to the front.
    pub fn focus(&self) {
        self.window.makeKeyAndOrderFront(None);
    }

    /// Follows the plugin: resizes the window when the view or the
    /// plugin asked for it, hands a user resize on to a CLAP plugin, and
    /// notices the user closing the window. Returns whether the window
    /// is still open.
    pub fn tick(&mut self, _mtm: MainThreadMarker) -> bool {
        if !self.open {
            return false;
        }
        if !self.window.isVisible() && !self.window.isMiniaturized() {
            self.open = false;
            return false;
        }
        let container = self.container.frame().size;
        match &mut self.content {
            Content::Au { view, .. } => {
                let wanted = view.frame().size;
                if !self.resizable
                    && (wanted.width != container.width || wanted.height != container.height)
                {
                    self.window.setContentSize(wanted);
                    view.setFrameOrigin(NSPoint::new(0.0, 0.0));
                }
            }
            Content::Clap { shared, size } => {
                let mut instance = shared.lock();
                let (requested, closed) = instance.access_shared_handler(|s| {
                    (
                        s.gui_resize.swap(0, Ordering::Relaxed),
                        s.gui_closed.swap(false, Ordering::Relaxed),
                    )
                });
                if closed {
                    drop(instance);
                    self.open = false;
                    return false;
                }
                let Some(gui) = instance.plugin_shared_handle().get_extension::<PluginGui>() else {
                    return true;
                };
                let plugin = instance.plugin_handle();
                if requested != 0 {
                    let wanted = GuiSize::unpack_from_u64(requested);
                    self.window.setContentSize(ns_size(wanted));
                    let _ = gui.set_size(&plugin, wanted);
                    *size = wanted;
                } else if self.resizable {
                    let current = gui_size(container);
                    if current != *size {
                        let adjusted = gui.adjust_size(&plugin, current).unwrap_or(current);
                        if gui.set_size(&plugin, adjusted).is_ok() {
                            *size = adjusted;
                        }
                        if adjusted != current {
                            self.window.setContentSize(ns_size(adjusted));
                        }
                    }
                }
            }
        }
        true
    }

    /// Closes the window and, for a CLAP plugin, destroys its GUI.
    pub fn close(mut self, _mtm: MainThreadMarker) {
        self.open = false;
        if let Content::Clap { shared, .. } = &self.content {
            let mut instance = shared.lock();
            if let Some(gui) = instance.plugin_shared_handle().get_extension::<PluginGui>() {
                let plugin = instance.plugin_handle();
                let _ = gui.hide(&plugin);
                gui.destroy(&plugin);
            }
        }
        if let Some(parent) = &self.parent {
            parent.removeChildWindow(&self.window);
        }
        self.window.close();
    }
}

/// The view an Audio Unit ships, from its `CocoaUI` property: a bundle
/// and the name of a factory class in it that makes the view.
fn cocoa_view(mtm: MainThreadMarker, unit: AudioUnit) -> Option<Retained<NSView>> {
    let _ = mtm;
    let mut size = 0u32;
    let status = unsafe {
        AudioUnitGetPropertyInfo(
            unit,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            &mut size,
            null_mut(),
        )
    };
    if status != 0 || (size as usize) < size_of::<AudioUnitCocoaViewInfo>() {
        return None;
    }
    let mut storage = vec![0u64; (size as usize).div_ceil(size_of::<u64>())];
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            NonNull::new(storage.as_mut_ptr()).unwrap().cast(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        return None;
    }
    // The property hands out references we own: the bundle URL and one
    // class name per view, the first being the main one.
    let info = unsafe { &*(storage.as_ptr() as *const AudioUnitCocoaViewInfo) };
    let url: CFRetained<CFURL> = unsafe { CFRetained::from_raw(info.mCocoaAUViewBundleLocation) };
    let class_count =
        (size as usize - size_of::<NonNull<CFURL>>()) / size_of::<NonNull<CFString>>();
    let names: Vec<CFRetained<CFString>> = (0..class_count)
        .map(|i| unsafe {
            let slot = (storage.as_ptr() as *const u8)
                .add(size_of::<NonNull<CFURL>>() + i * size_of::<NonNull<CFString>>())
                as *const NonNull<CFString>;
            CFRetained::from_raw(*slot)
        })
        .collect();
    let class_name = names.first()?;

    // Toll-free bridged: a CFURL is an NSURL, a CFString an NSString.
    let ns_url: &NSURL = unsafe { &*(CFRetained::as_ptr(&url).as_ptr() as *const NSURL) };
    let ns_name: &NSString =
        unsafe { &*(CFRetained::as_ptr(class_name).as_ptr() as *const NSString) };
    let bundle = NSBundle::bundleWithURL(ns_url)?;
    if !unsafe { bundle.load() } {
        return None;
    }
    let class = bundle.classNamed(ns_name)?;
    let factory: Retained<AnyObject> = unsafe { msg_send![class, new] };
    // AUCocoaUIBase: the size is a hint the view may ignore.
    let hint = NSSize::new(0.0, 0.0);
    let view: Option<Retained<NSView>> =
        unsafe { msg_send![&*factory, uiViewForAudioUnit: unit, withSize: hint] };
    view
}
