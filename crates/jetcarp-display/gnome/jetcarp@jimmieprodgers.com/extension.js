// JETCARP's GNOME side (PLAN.md "Live desktop", wave 5).
//
// The background host opens one fullscreen window per wallpaper, with a title that starts with
// TAG. This extension shows a clone of that window inside every background actor (desktop,
// overview, lock screen), minimizes the real one, and keeps it out of the window lists.
//
// It runs in the unlock-dialog session mode so the scene stays live behind the lock screen. There
// it only adds clones to background actors and adjusts how dark and how blurred the backdrop is;
// it has no part in authentication, and if the host dies the clones go and GNOME's own background
// shows. The menu exists only in the normal session.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Background from 'resource:///org/gnome/shell/ui/background.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import * as Util from 'resource:///org/gnome/shell/misc/util.js';
import * as Workspace from 'resource:///org/gnome/shell/ui/workspace.js';
import * as WorkspaceThumbnail from 'resource:///org/gnome/shell/ui/workspaceThumbnail.js';
import {Extension, InjectionManager} from 'resource:///org/gnome/shell/extensions/extension.js';

Gio._promisify(Gio.SocketClient.prototype, 'connect_to_host_async');
Gio._promisify(Gio.OutputStream.prototype, 'write_bytes_async');
Gio._promisify(Gio.DataInputStream.prototype, 'read_line_async');

const TAG = '@jetcarp-wallpaper';
const TICK_MS = 250;
/// The host treats silence longer than a few of these as the extension having gone.
const HEARTBEAT_S = 2;
/// A window is minimized after its clones have been up this many ticks: minimizing during the
/// window's opening animation can leave the clones painting nothing.
const SETTLE_TICKS = 3;
const IDLE_FLAG = 8; // GSM_INHIBITOR_FLAG_IDLE

const isRenderer = window => window?.title?.startsWith(TAG) ?? false;

function tagOf(window) {
    try {
        return JSON.parse(window.title.slice(TAG.length));
    } catch {
        return {};
    }
}

function backgroundActors(root, out = []) {
    for (const child of root.get_children()) {
        if (child instanceof Meta.BackgroundActor)
            out.push(child);
        else
            backgroundActors(child, out);
    }
    return out;
}

const isLockBackground = bg => {
    const parent = bg.get_parent();
    return parent instanceof St.Widget && parent.has_style_class_name('screen-shield-background');
};

/// One JSON-RPC exchange with the host per call, over its loopback link.
async function callHost(args) {
    const file = GLib.build_filenamev([GLib.get_user_runtime_dir(), 'jetcarp', 'host-endpoint.json']);
    const [, bytes] = GLib.file_get_contents(file);
    const info = JSON.parse(new TextDecoder().decode(bytes));

    const client = new Gio.SocketClient({timeout: 3});
    const connection = await client.connect_to_host_async(`127.0.0.1:${info.port}`, 0, null);
    try {
        const output = connection.get_output_stream();
        const input = new Gio.DataInputStream({
            base_stream: connection.get_input_stream(),
            close_base_stream: false,
        });
        const send = value => output.write_bytes_async(
            new GLib.Bytes(new TextEncoder().encode(`${JSON.stringify(value)}\n`)),
            GLib.PRIORITY_DEFAULT, null);
        const receive = async () => {
            const [line] = await input.read_line_async(GLib.PRIORITY_DEFAULT, null);
            if (line === null)
                throw new Error('the host closed the connection');
            return JSON.parse(new TextDecoder().decode(line));
        };

        await send({jetcarp_token: info.token});
        const hello = await receive();
        if (!hello.ok)
            throw new Error(hello.error ?? 'the host refused the connection');

        await send({
            jsonrpc: '2.0',
            id: 1,
            method: 'tools/call',
            params: {name: 'host_control', arguments: args},
        });
        const reply = await receive();
        const text = reply.result?.content?.[0]?.text ?? '';
        if (reply.error || reply.result?.isError)
            throw new Error(reply.error?.message ?? text);
        return JSON.parse(text);
    } finally {
        connection.close(null);
    }
}

const Indicator = GObject.registerClass(
class JetcarpIndicator extends PanelMenu.Button {
    _init(extension) {
        super._init(0.5, 'JETCARP');
        this._extension = extension;
        this.add_child(new St.Icon({
            gicon: Gio.icon_new_for_string(`${extension.path}/icons/jetcarp-symbolic.svg`),
            style_class: 'system-status-icon',
        }));

        const item = (label, action) => {
            const it = new PopupMenu.PopupMenuItem(label);
            it.connect('activate', action);
            this.menu.addMenuItem(it);
            return it;
        };
        const ask = args => () => this._ask(args);

        this._now = new PopupMenu.PopupMenuItem('', {reactive: false});
        this.menu.addMenuItem(this._now);
        this._start = item('Start JETCARP host', () => this._startHost());
        this._next = item('Next', ask({action: 'next'}));
        this._previous = item('Previous', ask({action: 'previous'}));

        this._paused = new PopupMenu.PopupSwitchMenuItem('Paused', false);
        this._paused.connect('toggled', (_, on) => this._ask({action: on ? 'pause' : 'resume'}));
        this.menu.addMenuItem(this._paused);

        this._wallpaper = new PopupMenu.PopupSwitchMenuItem('Show on the desktop', false);
        this._wallpaper.connect('toggled', (_, on) => this._ask({action: 'wallpaper', on}));
        this.menu.addMenuItem(this._wallpaper);

        this._rotations = new PopupMenu.PopupSubMenuMenuItem('Rotation');
        this.menu.addMenuItem(this._rotations);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._open = item('Open JETCARP', ask({action: 'open_app'}));
        this._login = new PopupMenu.PopupSwitchMenuItem('Start at login', false);
        this._login.connect('toggled', (_, on) => this._ask({action: 'login', on}));
        this.menu.addMenuItem(this._login);
        this._quit = item('Quit JETCARP host', ask({action: 'quit'}));

        this.menu.connect('open-state-changed', (_, open) => {
            if (open)
                this._refresh();
        });
        this._show(null);
    }

    async _ask(args) {
        try {
            const status = await callHost(args);
            this._show(args.action === 'quit' ? null : status);
        } catch (e) {
            if (args.action !== 'quit')
                console.warn(`JETCARP: ${args.action}: ${e.message}`);
            this._show(null);
        }
    }

    _refresh() {
        return this._ask({action: 'status'});
    }

    _startHost() {
        try {
            const file = GLib.build_filenamev([GLib.get_user_config_dir(), 'jetcarp', 'host-program']);
            const [, bytes] = GLib.file_get_contents(file);
            // The program on the first line, then one argument per line (`jetcarp host serve`).
            const argv = new TextDecoder().decode(bytes).split('\n').map(l => l.trimEnd()).filter(l => l.length > 0);
            if (argv.length === 0)
                throw new Error(`${file} is empty`);
            Util.spawn(argv);
        } catch (e) {
            Main.notifyError('JETCARP', `Could not start the host: ${e.message}`);
        }
    }

    // v: the host's status, or null when it is not running.
    _show(v) {
        const running = v !== null;
        for (const it of [this._next, this._previous, this._paused, this._wallpaper,
            this._rotations, this._open, this._login, this._quit])
            it.visible = running;
        this._start.visible = !running;
        if (!running) {
            this._now.label.text = 'JETCARP host is not running';
            return;
        }
        const stage = v.screens?.[0]?.stage ?? {};
        this._now.label.text = `${stage.label ?? 'Starting'} (${stage.number} of ${stage.count})`;
        this._paused.setToggleState(v.paused === true);
        this._wallpaper.setToggleState(v.wallpaper === true);
        this._login.setToggleState(v.run_at_login === true);

        const names = v.rotations ?? [];
        const sub = this._rotations.menu;
        sub.removeAll();
        for (const name of names) {
            const it = new PopupMenu.PopupMenuItem(name);
            it.setOrnament(name === stage.rotation ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NONE);
            it.connect('activate', () => this._ask({action: 'rotation', name}));
            sub.addMenuItem(it);
        }
    }
});

export default class JetcarpDesktop extends Extension {
    enable() {
        this._slots = new Map(); // background actor -> Clutter.Clone or null
        this._settled = new Map(); // window actor -> ticks with clones up
        this._holds = {dark: false, covered: false};
        this._heartbeatAt = 0;
        this._calling = false;
        this._inhibit = null;
        this._awakeExpired = false;

        this._injections = new InjectionManager();
        this._installOverrides();

        for (const bg of backgroundActors(Main.uiGroup))
            this._adopt(bg);

        this._sessionId = Main.sessionMode.connect('updated', () => this._syncIndicator());
        this._syncIndicator();

        this._timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, TICK_MS, () => {
            this._tick();
            return GLib.SOURCE_CONTINUE;
        });
    }

    disable() {
        GLib.source_remove(this._timer);
        this._timer = 0;
        Main.sessionMode.disconnect(this._sessionId);
        this._indicator?.destroy();
        this._indicator = null;

        // Host windows go back into the window lists first (InjectionManager can only put
        // methods back), then every overridden method is restored.
        this._undoWindowList?.();
        this._undoWindowList = null;
        this._injections.clear();
        this._injections = null;

        this._setLockLook(false);
        this._releaseInhibit();

        const slots = this._slots;
        this._slots = null;
        for (const clone of slots.values())
            clone?.destroy();

        // The windows come back to normal; a running host sees the heartbeat stop and goes on
        // in a plain window.
        for (const actor of this._renderers()) {
            actor.opacity = 255;
            const win = actor.meta_window;
            win.unstick();
            win.unminimize();
        }
        this._allActors = null;
        this._settled = null;
    }

    _syncIndicator() {
        const wanted = Main.sessionMode.currentMode === 'user';
        if (wanted && !this._indicator) {
            this._indicator = new Indicator(this);
            Main.panel.addToStatusArea(this.uuid, this._indicator);
        } else if (!wanted && this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }
    }

    _installOverrides() {
        const extension = this;
        const injections = this._injections;

        // A host window is one that isRenderer() recognises by the tag in its title; a host
        // actor is the compositor actor of a host window.
        const isHostWindow = metaWindow => Boolean(metaWindow) && isRenderer(metaWindow);
        const isHostActor = actor => isHostWindow(actor?.meta_window);

        // Replace owner[name] through the InjectionManager, but only when that method exists in
        // the running Shell. If a release renames or drops it, that one change is skipped instead
        // of throwing. Returns the original method (or null) for code that still needs the full,
        // unfiltered answer.
        const hook = (owner, name, makeReplacement) => {
            const original = owner?.[name];
            if (typeof original !== 'function') {
                console.debug(`JETCARP: ${name}() not found in this GNOME Shell, not changing it`);
                return null;
            }
            injections.overrideMethod(owner, name, makeReplacement);
            return original;
        };

        // Backgrounds. BackgroundManager builds every background actor (the desktop on each
        // monitor, the overview, the workspace-switch animation and the lock screen) in
        // _createBackgroundActor(), both when the manager is constructed and again whenever the
        // wallpaper setting changes. Each new actor is handed to _adopt() the moment it exists, so
        // a new monitor or the lock screen shows the live picture from its first frame instead of
        // flashing the static wallpaper.
        hook(Background.BackgroundManager?.prototype, '_createBackgroundActor',
            original => function (...args) {
                const actor = original.apply(this, args);
                try {
                    extension._adopt(actor);
                } catch (e) {
                    // A failure here must never stop the shell from building its background; it
                    // just keeps the plain wallpaper.
                    console.error('JETCARP: could not take over a new background', e);
                }
                return actor;
            });

        // The global window-actor list. Most of the shell's window walks get their windows from
        // global.get_window_actors(): the overview, the workspace thumbnails, the workspace-switch
        // animation, the screenshot tool's window picker, the dynamic-workspace bookkeeping (so
        // the host never keeps an otherwise empty workspace alive), and other extensions. Host
        // windows are taken out of that list. The extension itself needs the host actors to clone
        // them, so this._allActors calls the original method, which InjectionManager hands over
        // directly.
        const unfilteredActors = hook(Shell.Global.prototype, 'get_window_actors',
            original => function (...args) {
                return original.apply(this, args).filter(actor => !isHostActor(actor));
            });
        this._allActors = unfilteredActors
            ? () => unfilteredActors.call(global)
            : () => [];

        // Mutter's own window-list flag. Meta.Window.hide_from_window_list() (public since Mutter
        // 49) marks a window skip-taskbar and skip-pager from the compositor side, and the shell
        // honours that by itself: an app whose only windows are host windows is marked stopped (no
        // dash entry, no running dot in the app grid, not in the app switcher), the host never
        // becomes the top bar's focused app, a request for attention from it raises no "is ready"
        // notification, an open overview drops its preview at once, and docks and window lists
        // from other extensions skip it too. The tag is in the title, and a Wayland window exists
        // before its title arrives, so every window is watched for title changes, not just checked
        // once when it is created. The method overrides further down still apply, both for the
        // places this flag does not reach and for Mutter versions without it.
        //
        // Mutter refuses to minimize a skip-taskbar window (meta_window_recalc_features(), Mutter
        // 49 and 50), and the flag makes a window skip-taskbar. So a host window gets the flag only
        // once _settleWindows() has minimized it, and gives it up if it is unminimized, so that it
        // can be minimized again. Flagged first, the window was never minimized: it stayed up as
        // a fullscreen window, and GNOME counted the desktop as covered by a fullscreen app.
        this._undoWindowList = null;
        const windowProto = Meta.Window.prototype;
        if (typeof windowProto.hide_from_window_list === 'function' &&
            typeof windowProto.show_in_window_list === 'function') {
            const display = global.display;
            const flagged = new Set(); // host windows this extension has hidden
            const watched = new Map(); // window -> its signal handler ids

            const sync = win => {
                const host = isHostWindow(win) && win.minimized;
                if (host && !flagged.has(win)) {
                    win.hide_from_window_list();
                    flagged.add(win);
                } else if (!host && flagged.has(win)) {
                    // Unminimized, or the tag was removed again: give the window back.
                    win.show_in_window_list();
                    flagged.delete(win);
                }
            };
            const forget = win => {
                for (const id of watched.get(win) ?? [])
                    win.disconnect(id);
                watched.delete(win);
                flagged.delete(win);
            };
            const watch = win => {
                if (watched.has(win))
                    return;
                watched.set(win, [
                    win.connect('notify::title', () => sync(win)),
                    win.connect('notify::minimized', () => sync(win)),
                    win.connect('unmanaged', () => forget(win)),
                ]);
                sync(win);
            };

            const createdId = display.connect('window-created', (_display, win) => watch(win));
            const existing = display.list_all_windows?.() ??
                this._allActors().map(actor => actor.meta_window);
            existing.forEach(watch);

            // InjectionManager can only put methods back; disable() calls this first to give
            // host windows back to the window lists and disconnect the handlers.
            this._undoWindowList = () => {
                display.disconnect(createdId);
                for (const win of [...watched.keys()]) {
                    if (flagged.has(win))
                        win.show_in_window_list();
                    forget(win);
                }
            };
        }

        // Activities overview. Workspace decides which windows get a preview in the overview's
        // workspace view, and WorkspaceThumbnail which get a miniature in the workspace strip, each
        // through its own _isOverviewWindow(). The filtered actor list above covers the windows
        // present when the overview opens; these cover windows that appear or move to a workspace
        // while it is open. Workspace is given a Meta.Window, WorkspaceThumbnail a Meta.WindowActor.
        hook(Workspace.Workspace?.prototype, '_isOverviewWindow',
            original => function (metaWindow, ...rest) {
                return !isHostWindow(metaWindow) && original.call(this, metaWindow, ...rest);
            });
        hook(WorkspaceThumbnail.WorkspaceThumbnail?.prototype, '_isOverviewWindow',
            original => function (actor, ...rest) {
                return !isHostActor(actor) && original.call(this, actor, ...rest);
            });

        // Window switchers. The app and window switchers (Alt+Tab, Super+Tab, Alt+`, Alt+Esc) and
        // the Ctrl+Alt+Tab accessibility switcher get their windows from
        // global.display.get_tab_list(). This filter is needed even with the Mutter flag above:
        // Ctrl+Alt+Tab lists Mutter's DOCKS tab list, which is exactly where skip-taskbar windows
        // end up.
        hook(Meta.Display.prototype, 'get_tab_list',
            original => function (...args) {
                return original.apply(this, args).filter(win => !isHostWindow(win));
            });

        // Applications. The window tracker files a host window under an app like any other window.
        // Anything that asks an app for its windows (the Alt+F6 same-app window cycler, the app
        // menu's window list, the shell's introspection D-Bus API, docks from other extensions)
        // gets the list without host windows, and per-app window counts leave them out. The count
        // reads the original get_windows() so that it subtracts host windows from the real total.
        const unfilteredAppWindows = hook(Shell.App.prototype, 'get_windows',
            original => function (...args) {
                return original.apply(this, args).filter(win => !isHostWindow(win));
            });
        const allWindowsOf = app => unfilteredAppWindows?.call(app) ?? [];
        hook(Shell.App.prototype, 'get_n_windows',
            original => function (...args) {
                const hosts = allWindowsOf(this).filter(isHostWindow).length;
                return Math.max(0, original.apply(this, args) - hosts);
            });

        // An app whose every window is a host window is not running as far as the user is
        // concerned. The dash and the app switcher both start from AppSystem.get_running(), so
        // such apps are dropped from it. With the Mutter flag the shell already marks these apps
        // stopped; this filter is the fallback for Mutter versions without the flag.
        hook(Shell.AppSystem.prototype, 'get_running',
            original => function (...args) {
                return original.apply(this, args).filter(app => {
                    const windows = allWindowsOf(app);
                    return windows.length === 0 || !windows.every(isHostWindow);
                });
            });
    }

    _adopt(bg) {
        if (!this._slots || this._slots.has(bg))
            return;
        this._slots.set(bg, null);
        bg.connect('destroy', () => this._slots?.delete(bg));
    }

    _renderers() {
        return (this._allActors?.() ?? []).filter(a => isRenderer(a.meta_window));
    }

    // The window to show on `bg`: the one on its monitor, else one that fills every monitor.
    _sourceFor(bg, renderers) {
        const shows = actor => {
            const lock = tagOf(actor.meta_window).lock ?? 'live';
            return !isLockBackground(bg) || lock !== 'off';
        };
        const own = renderers.find(a => !tagOf(a.meta_window).all &&
            a.meta_window.get_monitor() === bg.monitor);
        const source = own ?? renderers.find(a => tagOf(a.meta_window).all) ?? null;
        return source && shows(source) ? source : null;
    }

    // Natural size when the window matches the background; otherwise cover it, cropped to it.
    _fit(clone, bg, source) {
        const [bw, bh] = [bg.width, bg.height];
        const [sw, sh] = [source.width, source.height];
        if (bw <= 0 || bh <= 0 || sw <= 0 || sh <= 0)
            return;
        if (Math.abs(bw - sw) < 1 && Math.abs(bh - sh) < 1) {
            clone.set_position(0, 0);
            clone.set_size(-1, -1);
            clone.remove_clip();
            return;
        }
        const scale = Math.max(bw / sw, bh / sh);
        const [w, h] = [Math.ceil(sw * scale), Math.ceil(sh * scale)];
        const [x, y] = [Math.floor((bw - w) / 2), Math.floor((bh - h) / 2)];
        clone.set_position(x, y);
        clone.set_size(w, h);
        clone.set_clip(-x, -y, bw, bh);
    }

    _syncClones(renderers) {
        for (const [bg, current] of this._slots) {
            const source = this._sourceFor(bg, renderers);
            let clone = current;
            // A clone whose window died has a null source, which a null `source` would match.
            if (clone && (!source || clone.source !== source)) {
                clone.destroy();
                clone = null;
            }
            if (source && !clone) {
                clone = new Clutter.Clone({source});
                bg.add_child(clone);
            }
            if (clone !== current)
                this._slots.set(bg, clone);
            if (clone)
                this._fit(clone, bg, source);
        }
    }

    _settleWindows(renderers) {
        const cloned = new Set(this._slots.values());
        const hasClone = actor => [...cloned].some(c => c?.source === actor);
        for (const actor of renderers) {
            const win = actor.meta_window;
            // On every workspace, so it never keeps a dynamic workspace alive.
            if (!win.on_all_workspaces)
                win.stick();
            // A scene shared by every monitor is drawn at the primary monitor's size.
            const primary = Main.layoutManager.primaryIndex;
            if (tagOf(win).all && win.get_monitor() !== primary)
                win.move_to_monitor(primary);
            if (win.minimized || !hasClone(actor)) {
                this._settled.delete(actor);
                continue;
            }
            const ticks = (this._settled.get(actor) ?? 0) + 1;
            this._settled.set(actor, ticks);
            if (ticks >= SETTLE_TICKS) {
                Main.wm.skipNextEffect(actor);
                win.minimize();
                this._settled.delete(actor);
            }
        }
    }

    // Screensaver mode: no fade to black and no blur until the unlock prompt shows.
    _setLockLook(saver) {
        const shield = Main.screenShield;
        if (!shield)
            return;
        const factor = saver ? 0 : 1;
        for (const box of [shield._shortLightbox, shield._longLightbox]) {
            if (box)
                box._fadeFactor = factor;
        }
        const dialog = shield._dialog;
        const group = dialog?._backgroundGroup;
        if (group) {
            const clear = saver && dialog._adjustment.value === 0;
            for (const widget of group.get_children()) {
                const blur = widget.get_effect('blur');
                if (blur && blur.enabled === clear)
                    blur.enabled = !clear;
            }
        }
    }

    _takeInhibit() {
        if (this._inhibit)
            return;
        const state = {cookie: 0, released: false};
        this._inhibit = state;
        Gio.DBus.session.call('org.gnome.SessionManager', '/org/gnome/SessionManager',
            'org.gnome.SessionManager', 'Inhibit',
            new GLib.Variant('(susu)', ['jetcarp-host', 0, 'JETCARP screensaver', IDLE_FLAG]),
            null, Gio.DBusCallFlags.NONE, -1, null, (bus, result) => {
                try {
                    [state.cookie] = bus.call_finish(result).deepUnpack();
                    if (state.released)
                        this._uninhibit(state.cookie);
                } catch (e) {
                    console.warn(`JETCARP: could not keep the screen awake: ${e.message}`);
                }
            });
    }

    _uninhibit(cookie) {
        Gio.DBus.session.call('org.gnome.SessionManager', '/org/gnome/SessionManager',
            'org.gnome.SessionManager', 'Uninhibit', new GLib.Variant('(u)', [cookie]),
            null, Gio.DBusCallFlags.NONE, -1, null, null);
    }

    _releaseInhibit() {
        const state = this._inhibit;
        this._inhibit = null;
        if (!state)
            return;
        if (state.cookie)
            this._uninhibit(state.cookie);
        else
            state.released = true;
    }

    _syncLock(renderers) {
        const shield = Main.screenShield;
        if (!shield)
            return;
        const tag = renderers.length > 0 ? tagOf(renderers[0].meta_window) : {};
        const active = shield.active;
        if (!active)
            this._awakeExpired = false;

        const minutes = tag.awake ?? 0;
        if (active && minutes > 0 && !this._awakeExpired) {
            const seconds = (GLib.get_monotonic_time() - shield.activationTime) / 1e6;
            if (seconds >= minutes * 60) {
                this._awakeExpired = true;
                this._setLockLook(false);
                // GNOME skipped its fade; do it now so the screen can sleep as usual.
                shield._activateFade(shield._shortLightbox, 3000);
            }
        }

        const saver = tag.lock === 'screensaver' && !this._awakeExpired;
        this._setLockLook(saver);
        if (saver && active)
            this._takeInhibit();
        else
            this._releaseInhibit();

        const black = !saver && (shield._shortLightbox?.active || shield._longLightbox?.active);
        this._holds.dark = active && (tag.lock === 'off' || black === true);
    }

    // The host need not draw for a monitor a fullscreen app covers. A host window is fullscreen
    // itself, so until it is minimized GNOME counts its monitor as covered: nothing is held then.
    _syncCovered(renderers) {
        const display = global.display;
        const monitors = renderers.some(a => tagOf(a.meta_window).all)
            ? [...Array(display.get_n_monitors()).keys()]
            : renderers.map(a => a.meta_window.get_monitor());
        const settled = renderers.every(a => a.meta_window.minimized);
        this._holds.covered = Main.sessionMode.currentMode === 'user' && settled &&
            monitors.length > 0 &&
            monitors.every(m => m >= 0 && display.get_monitor_in_fullscreen(m));
    }

    async _heartbeat(force) {
        const now = GLib.get_monotonic_time() / 1e6;
        if (this._calling || (!force && now - this._heartbeatAt < HEARTBEAT_S))
            return;
        this._calling = true;
        this._heartbeatAt = now;
        try {
            await callHost({action: 'extension', holds: {...this._holds}});
        } catch {
            // No host running: nothing to tell.
        }
        this._calling = false;
    }

    _tick() {
        const renderers = this._renderers();
        const before = JSON.stringify(this._holds);
        // Separately, so a failure in one can't leave the lock screen's look or the keep-awake
        // request stuck.
        for (const step of [
            () => this._syncClones(renderers),
            () => this._settleWindows(renderers),
            () => this._syncLock(renderers),
            () => this._syncCovered(renderers),
        ]) {
            try {
                step();
            } catch (e) {
                console.error(`JETCARP: ${e.message}`);
            }
        }
        this._heartbeat(before !== JSON.stringify(this._holds));
    }

    // For the automated test.
    status() {
        const renderers = this._renderers();
        const shield = Main.screenShield;
        return {
            mode: Main.sessionMode.currentMode,
            renderers: renderers.map(a => ({
                title: a.meta_window.title,
                size: [a.width, a.height],
                monitor: a.meta_window.get_monitor(),
                minimized: a.meta_window.minimized,
                sticky: a.meta_window.on_all_workspaces,
            })),
            backgrounds: [...this._slots].map(([bg, clone]) => ({
                monitor: bg.monitor,
                size: [bg.width, bg.height],
                lock: isLockBackground(bg),
                clone: clone ? {size: [clone.width, clone.height], pos: [clone.x, clone.y]} : null,
            })),
            holds: this._holds,
            inhibit: this._inhibit !== null,
            shield: {
                active: shield.active,
                locked: shield.locked,
                short: shield._shortLightbox.active,
                factor: shield._shortLightbox._fadeFactor,
                opacity: shield._shortLightbox.opacity,
                blur: shield._dialog?._backgroundGroup?.get_children()
                    .map(w => w.get_effect('blur')?.enabled) ?? [],
            },
            listed: {
                actors: global.get_window_actors().filter(a => isRenderer(a.meta_window)).length,
                tab: global.display.get_tab_list(Meta.TabList.NORMAL_ALL, null).filter(isRenderer).length,
                running: Shell.AppSystem.get_default().get_running().map(a => a.get_id()),
            },
            fullscreen: [...Array(global.display.get_n_monitors()).keys()]
                .map(m => global.display.get_monitor_in_fullscreen(m)),
            indicator: this._indicator ? this._indicator.visible : null,
        };
    }
}
