---
title: Browsers, devices and memory
summary: What differs in BimCraft between Chrome, Edge, Firefox and Safari, on Mac, Windows and Linux, with a mouse, a trackpad or touch, and how big a file it can take.
section: reference
order: 50
keywords: browser, chrome, edge, firefox, safari, brave, mac, windows, linux, ipad, tablet, phone, touch, trackpad, memory, large files, webgl, performance
---

BimCraft runs in current versions of Chrome, Edge, Firefox and Safari, on Mac, Windows and Linux. The 3D view needs WebGL, which every current browser has. See [Browsers and computers](common:browsers) for what the apps need in general.

## Opening a folder

| Browser | {{ui:Open a folder}} does this | Dragging a folder in | {{ui:Recent folders}} |
|---|---|---|---|
| Chrome, Edge | The browser's folder picker, then "Let site view files?": click **View files** | Opens with no prompt | Yes, for folders picked or dragged |
| Firefox | The system's folder chooser | Opens with no prompt | No |
| Safari | The system's folder chooser, with an **Upload** button (nothing is uploaded) | Opens with no prompt | No |

Behind the scenes: Chrome and Edge use the File System Access picker and can remember the folder. Firefox and Safari use the older folder chooser, which hands over every file in the folder at once and cannot be remembered. A dragged folder is read entry by entry in every browser.

:::chromium
Brave and some other Chromium browsers turn the folder picker off. BimCraft then uses the system's folder chooser instead, and the browser may ask "Upload *n* files to this site?". Nothing is uploaded; the files only go to the tab. Dragging the folder in avoids the question.
:::

## Mac, Windows and Linux

- Shortcuts written {{key:Mod+S}} and {{key:Mod+Z}} are Command on a Mac and Ctrl on Windows and Linux.
- {{key:Ctrl+Shift+F}} (screen modes) is Control on every platform, including a Mac.
- File dialogs and the downloads folder are your system's own.

## Mouse, trackpad and touch

| Input | 3D view | 2D drawing |
|---|---|---|
| Mouse | Left-drag orbits, right-drag pans, wheel zooms, click picks | Any drag pans, wheel zooms, double-click fits |
| Trackpad | Click-drag orbits; {{key:Shift}} or {{key:Mod}} and drag pans; two-finger scroll or pinch zooms | Click-drag pans; two-finger scroll or pinch zooms |
| Touch | One finger orbits, two fingers pinch and pan, tap picks | One finger pans; no pinch zoom yet; tap {{ui:Fit}} |

On a Mac trackpad, right-click is a two-finger click (or Control-click). See [Keys, mice and trackpads](common:keys).

Tablets and phones can open single files and look at them, but the screen is small, many mobile browsers cannot choose folders, and large models may not fit in their memory. A computer works best.

## Large files and memory

Everything BimCraft reads lives in the browser tab's memory. How much a tab can use depends on the browser and your computer, more than on BimCraft.

| File | How much is read | What to expect |
|---|---|---|
| IFC | The whole file, plus the 3D geometry built from it | How large depends on your computer and browser; a model of several hundred megabytes can be too much for a laptop. Over 1.5 million triangles, edges are turned off |
| Project and family files | About 40 KB to 100 KB, whatever the size | Even very large central models open in a moment, also from a network drive |
| DWG and DXF | The whole file | Very large drawings show "This drawing is very large; only part of it is shown." |
| Folders | The file list, plus a few kilobytes per model | Up to 20,000 files |

If a tab runs out of memory, the browser shows its own error page ("Aw, Snap!" in Chrome, for example). Close other tabs, reload BimCraft and try again, or use a computer with more memory.

## Related

- [Online and offline](topic:online-offline)
- [Troubleshooting](topic:troubleshooting)
